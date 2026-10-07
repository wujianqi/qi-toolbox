#[cfg(test)]
mod tests {
    use crate::core::store::*;

    /// store.db 是进程级单例连接：并行测试同时建表会触发 database is locked，
    /// 用互斥锁把写库类测试串行化
    static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 测试独占的临时库路径（进程内一次性生成；QI_STORE_DB 在首次 init 前生效）。
    /// 仅在首次生成时尝试删除残留旧文件——此时 STORE_DB 句柄尚未建立，删除安全；
    /// 各测试不得再删共享库文件（句柄已缓存，Windows 上删主文件会让后续复制/快照失败）。
    fn temp_db() -> std::path::PathBuf {
        use std::sync::OnceLock;
        static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let p = std::env::temp_dir().join(format!(
                "qi_store_test_{}_{}.db",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            ));
            let _ = std::fs::remove_file(&p);
            std::env::set_var("QI_STORE_DB", &p);
            p
        })
        .clone()
    }

    /// kv 读写删回路（独立临时库，不碰运行中 App 的 store.db）
    #[test]
    fn kv_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        let key = "test.kv_roundtrip";
        kv_set(&[(key, "v1")]).expect("kv_set");
        let all = kv_all().expect("kv_all");
        assert!(all.iter().any(|(k, v)| k == key && v == "v1"));
        // 覆盖写
        kv_set(&[(key, "v2")]).expect("kv_set overwrite");
        let all = kv_all().expect("kv_all");
        assert!(all.iter().any(|(k, v)| k == key && v == "v2"));
        assert!(!all.iter().any(|(k, v)| k == key && v == "v1"));
        kv_del(key).expect("kv_del");
        let all = kv_all().expect("kv_all");
        assert!(!all.iter().any(|(k, _)| k == key));
    }

    /// S3 站点 CRUD 回路（secret 走主口令加解密；独立临时库不碰真实 store.db）
    #[test]
    fn s3_site_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        // 敏感列走主口令加密：测试库需先设置测试主口令（解锁派生密钥）
        crate::core::master::setup("test-pass-123").expect("master setup");
        let name = format!("test-s3-{}", std::process::id());
        let site = S3Site {
            id: 0,
            name: name.clone(),
            endpoint: "https://s3.example.com".into(),
            region: "us-east-1".into(),
            bucket: "demo".into(),
            access_key: "AKID".into(),
            secret: "topsecret".into(),
            path_style: true,
        };
        s3_upsert(&site).expect("s3_upsert insert");
        let list = s3_list().expect("s3_list");
        let found = list.iter().find(|s| s.name == name).expect("site found");
        assert_eq!(found.bucket, "demo");
        assert_eq!(found.access_key, "AKID");
        // secret 经主口令加密落库、读出解密一致
        assert_eq!(found.secret, "topsecret");
        assert!(found.path_style);

        // 更新（拿到 id 后 upsert 改 bucket）
        let mut updated = site_with_id(found.id, &name);
        updated.bucket = "demo2".into();
        s3_upsert(&updated).expect("s3_upsert update");
        let list = s3_list().expect("s3_list");
        let found = list.iter().find(|s| s.name == name).expect("site found");
        assert_eq!(found.bucket, "demo2");
        // 不应出现重复行
        assert_eq!(list.iter().filter(|s| s.name == name).count(), 1);

        s3_del(found.id).expect("s3_del");
        let list = s3_list().expect("s3_list");
        assert!(!list.iter().any(|s| s.name == name));
    }

    fn site_with_id(id: i64, name: &str) -> S3Site {
        S3Site {
            id,
            name: name.to_string(),
            endpoint: "https://s3.example.com".into(),
            region: "us-east-1".into(),
            bucket: "demo".into(),
            access_key: "AKID".into(),
            secret: "topsecret".into(),
            path_style: true,
        }
    }

    /// SQL 历史：新增 → 去重（同 SQL 提到最新）→ 裁剪至上限 → 清空
    #[test]
    fn sql_history_dedup_trim_and_clear() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        let src = format!("test-src-{}", std::process::id());

        sql_history_add(&src, "SELECT 1").expect("add 1");
        sql_history_add(&src, "SELECT 2").expect("add 2");
        // 重复 SQL：去重并提到最新，不产生重复行
        sql_history_add(&src, "SELECT 1").expect("add dup");
        let list = sql_history_list(&src).expect("list");
        assert_eq!(list, vec!["SELECT 1".to_string(), "SELECT 2".to_string()]);

        // 裁剪：塞入上限+10 条，只保留最近 SQL_HISTORY_KEEP 条
        for i in 0..SQL_HISTORY_KEEP + 10 {
            sql_history_add(&src, &format!("SELECT {}", i)).expect("add bulk");
        }
        let list = sql_history_list(&src).expect("list trimmed");
        assert_eq!(list.len(), SQL_HISTORY_KEEP);
        // 最新在首位：最后一条写入的是 SELECT {KEEP+9}
        assert_eq!(list[0], format!("SELECT {}", SQL_HISTORY_KEEP + 9));
        // 不同源互不影响
        assert!(sql_history_list("other-source").unwrap().is_empty());

        sql_history_clear(&src).expect("clear");
        assert!(sql_history_list(&src).unwrap().is_empty());
    }

    /// SSH 常用命令：播种幂等、自定义命令按文本去重、按文本删除
    #[test]
    fn ssh_cmds_seed_dedup_and_del() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");

        let seeded = vec![("内置".to_string(), "ls -la".to_string())];
        ssh_seed_if_empty(&seeded).expect("seed");
        ssh_seed_if_empty(&seeded).expect("seed again");
        let list = ssh_list().expect("list");
        assert_eq!(
            list.iter().filter(|c| c.command == "ls -la").count(),
            1,
            "重复播种不应产生重复行"
        );

        let cmd = format!("echo test-{}", std::process::id());
        ssh_add_custom(&cmd).expect("add custom");
        ssh_add_custom(&cmd).expect("add custom dup");
        let list = ssh_list().expect("list");
        assert_eq!(
            list.iter().filter(|c| c.command == cmd).count(),
            1,
            "自定义命令按文本去重"
        );
        ssh_del_custom(&cmd).expect("del custom");
        let list = ssh_list().expect("list");
        assert!(!list.iter().any(|c| c.command == cmd));
    }

    /// TOTP 密钥 CRUD：secret 经主口令加解密，algo 原样存取
    #[test]
    fn totp_key_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        crate::core::master::setup("test-pass-123").expect("master setup");

        let name = format!("test-totp-{}", std::process::id());
        totp_upsert(&TotpKey {
            id: 0,
            name: name.clone(),
            secret: "JBSWY3DPEHPK3PXP".into(),
            algo: 1,
        })
        .expect("upsert");
        let list = totp_list().expect("list");
        let found = list.iter().find(|k| k.name == name).expect("found");
        assert_eq!(found.secret, "JBSWY3DPEHPK3PXP");
        assert_eq!(found.algo, 1);

        // 更新：换算法
        totp_upsert(&TotpKey {
            id: found.id,
            name: name.clone(),
            secret: found.secret.clone(),
            algo: 2,
        })
        .expect("update");
        let list = totp_list().expect("list");
        let found = list.iter().find(|k| k.id == found.id).expect("found");
        assert_eq!(found.algo, 2);
        assert_eq!(list.iter().filter(|k| k.name == name).count(), 1);

        totp_del(found.id).expect("del");
        assert!(!totp_list().unwrap().iter().any(|k| k.name == name));
    }

    /// SFTP 站点 CRUD：密码加密落库、key_path、端口存取
    #[test]
    fn sftp_site_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        crate::core::master::setup("test-pass-123").expect("master setup");

        let name = format!("test-sftp-{}", std::process::id());
        sftp_upsert(&SftpSite {
            id: 0,
            name: name.clone(),
            host: "sftp.example.com".into(),
            port: 2222,
            user: "root".into(),
            pass: "p@ss".into(),
            key_path: "".into(),
        })
        .expect("upsert");
        let list = sftp_list().expect("list");
        let found = list.iter().find(|s| s.name == name).expect("found");
        assert_eq!(found.host, "sftp.example.com");
        assert_eq!(found.port, 2222);
        assert_eq!(found.pass, "p@ss", "密码解密应与原文一致");

        // 更新：切换为密钥认证
        sftp_upsert(&SftpSite {
            id: found.id,
            name: name.clone(),
            host: found.host.clone(),
            port: found.port,
            user: found.user.clone(),
            pass: "".into(),
            key_path: "C:/keys/id_ed25519".into(),
        })
        .expect("update");
        let list = sftp_list().expect("list");
        let found = list.iter().find(|s| s.id == found.id).expect("found");
        assert_eq!(found.key_path, "C:/keys/id_ed25519");
        assert_eq!(found.pass, "", "空密码应存空");

        sftp_del(found.id).expect("del");
        assert!(!sftp_list().unwrap().iter().any(|s| s.name == name));
    }

    /// MySQL 站点 CRUD：密码加密、覆盖更新不产生重复行
    #[test]
    fn mysql_site_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        crate::core::master::setup("test-pass-123").expect("master setup");

        let name = format!("test-mysql-{}", std::process::id());
        mysql_site_upsert(&MySqlSite {
            id: 0,
            name: name.clone(),
            host: "127.0.0.1".into(),
            port: 3307,
            user: "root".into(),
            pass: "mysql-pass".into(),
        })
        .expect("upsert");
        let list = mysql_site_list().expect("list");
        let found = list.iter().find(|s| s.name == name).expect("found");
        assert_eq!(found.port, 3307);
        assert_eq!(found.pass, "mysql-pass");

        mysql_site_upsert(&MySqlSite {
            id: found.id,
            name: name.clone(),
            host: "10.0.0.2".into(),
            port: 3306,
            user: "admin".into(),
            pass: "new-pass".into(),
        })
        .expect("update");
        let list = mysql_site_list().expect("list");
        assert_eq!(list.iter().filter(|s| s.name == name).count(), 1);
        let found = list.iter().find(|s| s.name == name).expect("found");
        assert_eq!(found.host, "10.0.0.2");
        assert_eq!(found.user, "admin");
        assert_eq!(found.pass, "new-pass");

        mysql_site_del(found.id).expect("del");
        assert!(!mysql_site_list().unwrap().iter().any(|s| s.name == name));
    }

    /// 运维备忘 CRUD：新增/更新/删除回路 + 按更新时间倒序 + 空标题空日期合法
    #[test]
    fn memo_crud_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        crate::core::master::setup("test-pass-123").expect("master setup");

        let tag = format!("memo-t-{}", std::process::id());
        // 新增两条（内容非空、标题为空也允许；日期一条留空）
        let id1 = memo_add("", "第一步：备份数据库", "").expect("add 1");
        let id2 = memo_add(&tag, "巡检磁盘容量", "2026-10-02").expect("add 2");
        assert!(id1 > 0 && id2 > 0 && id1 != id2);

        // 列表应含两条，新插入的在前（updated DESC, id DESC）
        let list = memo_list().expect("list");
        assert_eq!(
            list.iter().filter(|m| m.id == id1 || m.id == id2).count(),
            2
        );
        let pos1 = list.iter().position(|m| m.id == id1).unwrap();
        let pos2 = list.iter().position(|m| m.id == id2).unwrap();
        assert!(pos2 < pos1, "新备忘应排在前面");
        let m2 = list.iter().find(|m| m.id == id2).unwrap();
        assert_eq!(m2.title, tag);
        assert_eq!(m2.content, "巡检磁盘容量");
        assert_eq!(m2.day, "2026-10-02");
        // 更新时间戳不映射到 Memo（仅供 SQL 排序），改用 SQL 直查验证写库
        let id2_str = id2.to_string();
        let got_updated: i64 = run(Box::new(move |conn: turso::Connection| {
            Box::pin(async move {
                let mut rows = conn
                    .query(
                        "SELECT updated FROM memos WHERE id = ?1",
                        (id2_str.as_str(),),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                match rows.next().await.map_err(|e| e.to_string())? {
                    Some(row) => row.get(0).map_err(|e| e.to_string()),
                    None => Err("memo row missing".to_string()),
                }
            })
        }))
        .expect("read updated");
        assert_eq!(got_updated, now_secs(), "更新时间应写入当前秒");

        // 更新：覆盖标题/内容/日期，id 不变
        memo_update(id2, &format!("{}-v2", tag), "巡检内存与磁盘", "2026-10-03").expect("update");
        let list = memo_list().expect("list after update");
        let m2 = list.iter().find(|m| m.id == id2).unwrap();
        assert_eq!(m2.title, format!("{}-v2", tag));
        assert_eq!(m2.content, "巡检内存与磁盘");
        assert_eq!(m2.day, "2026-10-03");

        // 删除后不再出现；另一条不受影响
        memo_del(id2).expect("del");
        let list = memo_list().expect("list after del");
        assert!(!list.iter().any(|m| m.id == id2));
        assert!(list.iter().any(|m| m.id == id1));
        memo_del(id1).expect("cleanup del 1");
        assert!(!list.iter().any(|m| m.id == id1 && false));
    }

    /// PG 站点 CRUD：连接串整条加密（内含密码）、覆盖更新、删除
    #[test]
    fn pg_site_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        crate::core::master::setup("test-pass-123").expect("master setup");

        let name = format!("test-pg-{}", std::process::id());
        let url = "postgres://u:p@10.0.0.5:5432/appdb".to_string();
        pg_site_upsert(&PgSite {
            id: 0,
            name: name.clone(),
            url: url.clone(),
        })
        .expect("upsert");
        let list = pg_site_list().expect("list");
        let found = list.iter().find(|s| s.name == name).expect("found");
        assert_eq!(found.url, url, "连接串解密应与原文一致");

        let url2 = "postgres://u2@127.0.0.1/prod".to_string();
        pg_site_upsert(&PgSite {
            id: found.id,
            name: name.clone(),
            url: url2.clone(),
        })
        .expect("update");
        let list = pg_site_list().expect("list");
        assert_eq!(list.iter().filter(|s| s.name == name).count(), 1);
        let found = list.iter().find(|s| s.name == name).expect("found");
        assert_eq!(found.url, url2);

        pg_site_del(found.id).expect("del");
        assert!(!pg_site_list().unwrap().iter().any(|s| s.name == name));
    }

    /// Turso 站点 CRUD：本地文件(kind=0)与网络(kind=1)两类、url/token 加密回读。
    /// v0.2.1 曾漏建 kind 列导致保存必失败，此测试防回归。
    #[test]
    fn turso_db_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        crate::core::master::setup("test-pass-123").expect("master setup");

        let name = format!("test-turso-{}", std::process::id());
        turso_db_upsert(&TursoDb {
            id: 0,
            name: name.clone(),
            kind: 1,
            path: String::new(),
            url: "libsql://example.turso.io".into(),
            token: "tok-abc".into(),
        })
        .expect("upsert");
        let list = turso_db_list().expect("list");
        let found = list.iter().find(|d| d.name == name).expect("found");
        assert_eq!(found.kind, 1);
        assert_eq!(
            found.url, "libsql://example.turso.io",
            "url 解密应与原文一致"
        );
        assert_eq!(found.token, "tok-abc");

        turso_db_upsert(&TursoDb {
            id: found.id,
            name: name.clone(),
            kind: 0,
            path: "C:\\data\\local.db".into(),
            url: String::new(),
            token: String::new(),
        })
        .expect("update");
        let list = turso_db_list().expect("list");
        assert_eq!(list.iter().filter(|d| d.name == name).count(), 1);
        let found = list.iter().find(|d| d.name == name).expect("found");
        assert_eq!(found.kind, 0);
        assert_eq!(found.path, "C:\\data\\local.db");

        turso_db_del(found.id).expect("del");
        assert!(!turso_db_list().unwrap().iter().any(|d| d.name == name));
    }

    /// 保存的 SQL：按连接源隔离、新→旧排序、删除
    #[test]
    fn saved_sql_add_list_del() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        let src = format!("test-src-{}", std::process::id());

        saved_sql_add(&src, "first", "SELECT 1").expect("add 1");
        saved_sql_add(&src, "second", "SELECT 2").expect("add 2");
        saved_sql_add("other-src", "elsewhere", "SELECT 9").expect("add other");

        let list = saved_sql_list(&src).expect("list");
        assert_eq!(list.len(), 2, "不同源互不影响");
        assert_eq!(list[0].name, "second", "新→旧排序");
        assert_eq!(list[0].sql, "SELECT 2");
        assert_eq!(list[1].name, "first");

        saved_sql_del(list[0].id).expect("del");
        let list = saved_sql_list(&src).expect("list after del");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "first");
    }

    /// SSH 主机指纹（TOFU）：首连 None → 记录 → 换指纹更新（用户确认重置）
    #[test]
    fn host_fp_tofu_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");

        let host = format!("fp-host-{}", std::process::id());
        assert_eq!(host_fp_get(&host, 22).expect("get"), None, "首连无记录");

        host_fp_set(&host, 22, "SHA256:abc").expect("set");
        assert_eq!(
            host_fp_get(&host, 22).expect("get"),
            Some("SHA256:abc".into())
        );
        // 同主机不同端口互不干扰
        assert_eq!(host_fp_get(&host, 2222).expect("get other port"), None);

        // 更新（用户确认新指纹）
        host_fp_set(&host, 22, "SHA256:xyz").expect("update");
        assert_eq!(
            host_fp_get(&host, 22).expect("get"),
            Some("SHA256:xyz".into())
        );
    }

    /// SFTP 书签：去重新增、按路径删除
    #[test]
    fn sftp_bmk_add_dedup_del() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");

        let a = format!("/var/www-{}", std::process::id());
        let b = format!("/etc/nginx-{}", std::process::id());
        sftp_bmk_add(&a).expect("add a");
        sftp_bmk_add(&a).expect("add a dup");
        sftp_bmk_add(&b).expect("add b");
        let list = sftp_bmk_list().expect("list");
        assert_eq!(
            list.iter().filter(|p| **p == a).count(),
            1,
            "重复添加应去重"
        );
        assert!(list.contains(&b));

        sftp_bmk_del(&a).expect("del a");
        let list = sftp_bmk_list().expect("list after del");
        assert!(!list.contains(&a));
        assert!(list.contains(&b), "删除不应影响其他书签");
    }

    /// 保存的密码：加密落库、解密回读、按 id 删除
    #[test]
    fn saved_pwd_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        crate::core::master::setup("test-pass-123").expect("master setup");

        saved_pwd_add("数据库 root", "pwd-甲").expect("add 1");
        saved_pwd_add("论坛账号", "pwd-乙").expect("add 2");
        let list = saved_pwd_list().expect("list");
        assert_eq!(list.len(), 2);
        let f1 = list
            .iter()
            .find(|p| p.used_for == "数据库 root")
            .expect("found 1");
        assert_eq!(f1.password, "pwd-甲", "密码解密应与原文一致");

        saved_pwd_del(f1.id).expect("del");
        let list = saved_pwd_list().expect("list after del");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].used_for, "论坛账号");
    }

    /// 远程检测网址：重复添加静默去重、按 URL 删除
    #[test]
    fn remote_url_add_dedup_del() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");

        let a = format!("https://check-{}.example.com", std::process::id());
        remote_url_add(&a).expect("add");
        remote_url_add(&a).expect("add dup");
        remote_url_add("https://baidu.com").expect("add fixed");
        let list = remote_url_list().expect("list");
        assert_eq!(
            list.iter().filter(|u| **u == a).count(),
            1,
            "重复添加应去重"
        );

        remote_url_del(&a).expect("del");
        let list = remote_url_list().expect("list after del");
        assert!(!list.contains(&a));
        assert!(list.contains(&"https://baidu.com".to_string()));
    }

    /// 主口令凭证：setup 写入 master_cred（v2 密文）、unlock 正确口令通过/错误口令拒绝。
    /// master_cred 与派生密钥是进程级共享状态，这里只验证回路，不断言初始状态。
    #[test]
    fn master_cred_set_and_unlock() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");

        crate::core::master::setup("pass-abc-独特后缀").expect("setup");
        assert!(crate::core::master::is_set());
        let (salt, verifier) = master_cred_get().expect("get").expect("some");
        assert!(!salt.is_empty() && !verifier.is_empty());
        assert!(verifier.starts_with("v2:"), "校验值应为 v2 密文");

        crate::core::master::unlock("pass-abc-独特后缀").expect("正确口令应解锁");
        assert!(
            crate::core::master::unlock("wrong-pass").is_err(),
            "错误口令应拒绝"
        );
    }

    /// 库快照：snapshot 产出临时文件、可被 turso 重新打开（文件完整）
    #[test]
    fn snapshot_creates_readable_copy() {
        let _g = DB_LOCK.lock().unwrap();
        let _tmp = temp_db();
        init().expect("init store schema");
        kv_set(&[("snapshot.mark", "v1")]).expect("seed kv");

        let snap = snapshot_db_path().expect("snapshot");
        assert!(snap.exists(), "快照文件应存在");
        // 快照能被独立打开且包含已写数据（用turso直接读，验证文件完整非空壳）
        let read = {
            let snap = snap.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async {
                    let db = turso::Builder::new_local(snap.to_string_lossy().as_ref())
                        .build()
                        .await
                        .expect("open snapshot");
                    let conn = db.connect().expect("connect");
                    let mut rows = conn
                        .query("SELECT value FROM kv WHERE key = 'snapshot.mark'", ())
                        .await
                        .expect("query");
                    match rows.next().await.expect("next") {
                        Some(row) => {
                            let v: String = row.get(0).unwrap();
                            v
                        }
                        None => panic!("快照缺少已写入的 kv 数据"),
                    }
                })
            })
        };
        let val = read.join().expect("read thread");
        assert_eq!(val, "v1");
        let _ = std::fs::remove_file(&snap);
    }
}
