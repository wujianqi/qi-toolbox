//! Redis 模块集成测试：连真实 Redis（默认 redis://127.0.0.1:6379/0，可用 QI_REDIS_URL 覆盖）。
//! 服务不可达时所有测试自动跳过（打印 skip 不算失败），CI 无服务也能绿。
//! 测试键统一挂在 `qi_tb_test:` 前缀下，结束后清空自己的前缀，不碰业务数据。

#[path = "../src/i18n/lang.rs"]
mod lang;

use qi_toolbox::core::redis;

/// 真机地址；连不上返回 None（测试跳过）
fn live() -> Option<String> {
    let url = std::env::var("QI_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/9".into());
    match redis::connect(&url) {
        Ok(()) => Some(url),
        Err(e) => {
            eprintln!("skip: Redis 不可达（{}）: {}", url, e);
            None
        }
    }
}

/// 所有测试共用一个 db：写键类测试必须串行,否则互相 DEL 对方的种子键
static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 该 db 的测试键总数（含任意键名）
fn test_keys(url: &str) -> Vec<String> {
    let (keys, _more) = redis::scan_keys(url, Some("qi_tb_test"), 0).unwrap_or_default();
    keys
}

/// 清掉本测试前缀的全部键（DEL 循环,量小无所谓）
fn cleanup(url: &str) {
    for k in test_keys(url) {
        let _ = redis::execute_command(url, &format!("DEL {}", k));
    }
}

/// ── 1. connect：PING 通过，重复调用（缓存路径）也成功 ──
#[test]
fn connect_and_reconnect() {
    let Some(url) = live() else { return };
    redis::connect(&url).expect("首次连接");
    redis::connect(&url).expect("二次连接（走缓存路径）");
}

/// ── 2. list_databases：至少 db0，且都是 dbN 形式 ──
#[test]
fn list_databases_shape() {
    let Some(url) = live() else { return };
    let dbs = redis::list_databases(&url).expect("list_databases");
    assert!(!dbs.is_empty(), "至少应有 db0");
    assert_eq!(dbs[0], "db0");
    assert!(dbs.iter().enumerate().all(|(i, d)| *d == format!("db{i}")));
}

/// ── 3. list_prefixes：写入多前缀键后应枚举出这些逻辑库 ──
#[test]
fn list_prefixes_finds_test_prefix() {
    let Some(url) = live() else { return };
    let _g = DB_LOCK.lock().unwrap();
    cleanup(&url);
    // 种子：三个前缀 + 一个无冒号键（exec 用同一连接,顺序执行）
    for cmd in [
        "SET qi_tb_test_alpha:1 a",
        "SET qi_tb_test_beta:2 b",
        "SET qi_tb_test_gamma:deep:key c",
        "SET qi_tb_test_standalone x",
    ] {
        redis::execute_command(&url, cmd).expect("seed SET");
    }
    let prefixes = redis::list_prefixes(&url).expect("list_prefixes");
    // qi_tb_test 前缀（无冒号键自成一项）+ 各带冒号的子前缀
    assert!(
        prefixes.contains(&"qi_tb_test_alpha".to_string()),
        "应含 qi_tb_test_alpha，实际: {prefixes:?}"
    );
    assert!(prefixes.contains(&"qi_tb_test_beta".to_string()));
    assert!(prefixes.contains(&"qi_tb_test_gamma".to_string()));
    assert!(
        prefixes.contains(&"qi_tb_test_standalone".to_string()),
        "无冒号键自成一项"
    );
    // 已排序
    let mut sorted = prefixes.clone();
    sorted.sort();
    assert_eq!(prefixes, sorted, "list_prefixes 结果应有序");
    cleanup(&url);
}

/// ── 4. scan_keys 前缀过滤 + 分页：PAGE_SIZE=100，种 105 键 → 第 0 页 100 条有更多，第 1 页 5 条无更多 ──
#[test]
fn scan_keys_prefix_filter_and_paging() {
    let Some(url) = live() else { return };
    let _g = DB_LOCK.lock().unwrap();
    cleanup(&url);
    for i in 0..105 {
        redis::execute_command(&url, &format!("SET qi_tb_test_page:{i:04} v{i}")).expect("seed");
    }
    let (p0, more0) = redis::scan_keys(&url, Some("qi_tb_test_page"), 0).expect("page0");
    assert_eq!(p0.len(), 100, "第 0 页应满 PAGE_SIZE");
    assert!(more0, "105 键应有下一页");
    assert!(
        p0.iter().all(|k| k.starts_with("qi_tb_test_page:")),
        "前缀过滤：不应混入其它键"
    );
    let (p1, more1) = redis::scan_keys(&url, Some("qi_tb_test_page"), 1).expect("page1");
    assert_eq!(p1.len(), 5, "第 1 页应为剩余 5 条");
    assert!(!more1, "取尽后不应再有下一页");
    // 两页键不重叠
    assert!(p0.iter().all(|k| !p1.contains(k)), "翻页不重叠");
    cleanup(&url);
}

/// ── 5. scan_keys 无前缀（全库）也能翻页 ──
#[test]
fn scan_keys_all_keys_page() {
    let Some(url) = live() else { return };
    // 全库扫描只验证形状（库里可能有业务键,不断言数量）
    let (keys, _more) = redis::scan_keys(&url, None, 0).expect("scan all");
    assert!(keys.len() <= 100, "单页不超过 PAGE_SIZE");
}

/// ── 6. read_key：string / list / hash / set / 不存在的键 ──
#[test]
fn read_key_all_types() {
    let Some(url) = live() else { return };
    let _g = DB_LOCK.lock().unwrap();
    cleanup(&url);
    redis::execute_command(&url, "SET qi_tb_test_str hello").expect("seed str");
    redis::execute_command(&url, "RPUSH qi_tb_test_list a b c").expect("seed list");
    redis::execute_command(&url, "HSET qi_tb_test_hash f1 v1 f2 v2").expect("seed hash");
    redis::execute_command(&url, "SADD qi_tb_test_set m1 m2").expect("seed set");

    let e = redis::read_key(&url, "qi_tb_test_str").expect("read str");
    assert_eq!(e.typ, "string");
    assert_eq!(e.value, "hello");
    assert!(e.ttl < 0, "无过期应为 -1");

    let e = redis::read_key(&url, "qi_tb_test_list").expect("read list");
    assert_eq!(e.typ, "list");
    assert!(e.value.contains("a"), "list 值应含元素: {}", e.value);

    let e = redis::read_key(&url, "qi_tb_test_hash").expect("read hash");
    assert_eq!(e.typ, "hash");
    assert!(e.value.contains("f1") && e.value.contains("v1"));

    let e = redis::read_key(&url, "qi_tb_test_set").expect("read set");
    assert_eq!(e.typ, "set");

    // 不存在的键：返回 Err（或 Nil 值的 Ok），两种都可接受，但不得 panic
    let _ = redis::read_key(&url, "qi_tb_test_no_such_key");

    // 带 TTL 的键：ttl 应为正
    redis::execute_command(&url, "SET qi_tb_test_ttl v EX 100").expect("seed ttl");
    let e = redis::read_key(&url, "qi_tb_test_ttl").expect("read ttl");
    assert!(e.ttl > 0, "带过期键 ttl 应为正: {}", e.ttl);

    cleanup(&url);
}

/// ── 7. execute_command：表格形状（首行表头）/ 错误命令 / 非法引号 ──
#[test]
fn execute_command_shapes() {
    let Some(url) = live() else { return };
    let _g = DB_LOCK.lock().unwrap();
    cleanup(&url);
    redis::execute_command(&url, "SET qi_tb_test_cmd v1").expect("SET");

    // GET：标量 → 表头 value + 一行
    let rows = redis::execute_command(&url, "GET qi_tb_test_cmd").expect("GET");
    assert_eq!(rows[0], vec!["value"]);
    assert_eq!(rows[1], vec!["v1"]);

    // KEYS：数组 → 表头 + 每键一行
    let rows = redis::execute_command(&url, "KEYS qi_tb_test_cmd").expect("KEYS");
    assert_eq!(rows[0], vec!["value"]);
    assert!(rows.len() >= 2);

    // 带引号的值往返
    redis::execute_command(&url, r#"SET qi_tb_test_cmd "hello world""#).expect("SET quoted");
    let rows = redis::execute_command(&url, "GET qi_tb_test_cmd").expect("GET quoted");
    assert_eq!(rows[1], vec!["hello world"]);

    // 对错误命令（对 string 做 LPUSH）：返回 Err 而不是 panic
    assert!(redis::execute_command(&url, "LPUSH qi_tb_test_cmd x").is_err());

    // 不存在的命令：Err
    assert!(redis::execute_command(&url, "NOSUCHCMD x").is_err());

    // DEL 收尾
    assert!(redis::execute_command(&url, "DEL qi_tb_test_cmd").is_ok());
}

/// ── 8. url_for_db 换库后真的落到那个 db：db0 写入,db1 看不到 ──
#[test]
fn db_isolation_via_url_for_db() {
    let Some(url) = live() else { return };
    let _g = DB_LOCK.lock().unwrap();
    // 测试库用 db9（live 已连）；换到 db8 验证隔离
    let u8 = redis::url_for_db(&url, 8);
    let u9 = redis::url_for_db(&url, 9);
    redis::connect(&u8).expect("connect db8");
    redis::connect(&u9).expect("connect db9");
    // 各 db 先清自己的测试键（上次运行可能残留）
    let _ = redis::execute_command(&u8, "DEL qi_tb_test_iso");
    let _ = redis::execute_command(&u9, "DEL qi_tb_test_iso");
    redis::execute_command(&u8, "SET qi_tb_test_iso db8").expect("SET db8");
    // KEYS 空结果走标量分支：表头 + (empty) 行 → len 2 且第二行显示空
    let rows = redis::execute_command(&u9, "KEYS qi_tb_test_iso").expect("KEYS db9");
    assert!(
        rows.len() < 2 || rows[1][0].is_empty() || rows[1][0] == "(empty)",
        "db9 不应看到 db8 的键，实际: {rows:?}"
    );
    // db8 能查到
    let rows = redis::execute_command(&u8, "GET qi_tb_test_iso").expect("GET db8");
    assert_eq!(rows[1], vec!["db8"]);
    let _ = redis::execute_command(&u8, "DEL qi_tb_test_iso");
}

/// ── 9. 连接缓存失效（disconnect）后重连仍可用 ──
#[test]
fn reconnect_after_disconnect() {
    let Some(url) = live() else { return };
    redis::connect(&url).expect("connect");
    redis::disconnect(); // 清缓存
    redis::connect(&url).expect("重连");
    let rows = redis::execute_command(&url, "PING").expect("PING after reconnect");
    assert_eq!(rows[1], vec!["PONG"]);
}

/// ── 10. 错误连接串：快速失败且错误信息非空（不 panic、不长时间挂起） ──
#[test]
fn bad_url_fails_fast() {
    // 端口不存在的本地地址：连接拒绝应在秒级返回
    let t = std::time::Instant::now();
    let r = redis::connect("redis://127.0.0.1:1/0");
    assert!(r.is_err(), "不可达端口必须报错");
    assert!(!r.unwrap_err().is_empty());
    assert!(t.elapsed().as_secs() < 10, "连接拒绝不应长时间挂起");
}

/// ── 11. 站点 CRUD（store.db 临时库）：upsert/list/del 回路 ──
#[test]
fn site_crud_roundtrip() {
    // 独立临时库,不碰真实 store.db
    let p = std::env::temp_dir().join(format!("qi_redis_site_test_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    std::env::set_var("QI_STORE_DB", &p);
    qi_toolbox::core::store::init().expect("init store");
    // 站点表无敏感加密列,但 upsert 走主口令锁检查：先解锁测试口令（幂等）
    if !qi_toolbox::core::master::unlocked() {
        let _ = qi_toolbox::core::master::setup("test-pass-123");
        let _ = qi_toolbox::core::master::unlock("test-pass-123");
    }
    let name = format!("it-site-{}", std::process::id());

    let mut site = qi_toolbox::core::store::RedisSite {
        id: 0,
        name: name.clone(),
        url: "redis://127.0.0.1:6379/0".into(),
    };
    qi_toolbox::core::store::redis_site_upsert(&site).expect("insert");
    let list = qi_toolbox::core::store::redis_site_list().expect("list");
    let inserted = list.iter().find(|s| s.name == name).expect("应能查到");
    assert_eq!(inserted.url, "redis://127.0.0.1:6379/0");
    assert!(inserted.id > 0);

    // 更新
    site.id = inserted.id;
    site.url = "redis://:pw@127.0.0.1:6379/1".into();
    qi_toolbox::core::store::redis_site_upsert(&site).expect("update");
    let list = qi_toolbox::core::store::redis_site_list().expect("list2");
    let updated = list
        .iter()
        .find(|s| s.id == inserted.id)
        .expect("更新后仍在");
    assert_eq!(updated.url, "redis://:pw@127.0.0.1:6379/1");
    assert_eq!(
        list.iter().filter(|s| s.name == name).count(),
        1,
        "无重复行"
    );

    // 删除
    qi_toolbox::core::store::redis_site_del(inserted.id).expect("del");
    let list = qi_toolbox::core::store::redis_site_list().expect("list3");
    assert!(!list.iter().any(|s| s.id == inserted.id));
    let _ = std::fs::remove_file(&p);
}
