//! 临时集成验证（测完即删）：走 core::mysql 真实路径验证
//! 连接 / connect 分级列表 / 建表插数 / set_selected_table 分页与缓存 / execute_sql / 导出取数。
//! i18n 需先 install（lang::* 依赖 Catalog）。本机 MySQL：root / 空密码 / 3306。

#[path = "../src/i18n/lang.rs"]
mod lang;

use mysql_async::prelude::Queryable;

use qi_toolbox::core::mysql::{MySqlSource, MySqlViewer, TableRef, PAGE_SIZE};

fn main() {
    lang::install();
    // 连接参数优先取 QI_MYSQL_URL（CI 传入），默认为本机开发实例
    let src = match std::env::var("QI_MYSQL_URL")
        .ok()
        .and_then(|u| parse_mysql_url(&u))
    {
        Some(s) => s,
        None => MySqlSource {
            host: "127.0.0.1".into(),
            port: "3306".into(),
            user: "root".into(),
            pass: String::new(),
        },
    };

    // ── 1. 直连冒烟：服务可达、凭据正确 ──
    let s = src.clone();
    let ver = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime 失败");
        rt.block_on(async move {
            let url = format!(
                "mysql://{}:{}@{}:{}/mysql",
                s.user,
                urlencoding_pass(&s.pass),
                s.host,
                s.port
            );
            let opts = mysql_async::Opts::from_url(&url).expect("URL 解析失败");
            let mut conn = mysql_async::Conn::new(opts)
                .await
                .expect("MySQL 连接失败（服务未启动或凭据错误）");
            let v: String = conn
                .query_first("SELECT VERSION()")
                .await
                .expect("SELECT VERSION 失败")
                .expect("无结果");
            v
        })
    });
    let v = ver.join().expect("冒烟线程 panic");
    println!("[1] 连接 OK: MySQL {}", v);

    // ── 2. 建测试库 + 测试表 + 插数 ──
    let s = src.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime 失败");
        rt.block_on(async move {
            // 与步骤 1 一致携带密码（CI 服务 root 有密码，空密码仅本机开发场景）
            let url = format!(
                "mysql://{}:{}@{}:{}/mysql",
                s.user,
                urlencoding_pass(&s.pass),
                s.host,
                s.port
            );
            let opts = mysql_async::Opts::from_url(&url).expect("URL 解析失败");
            let mut conn = mysql_async::Conn::new(opts).await.expect("MySQL 连接失败");
            use mysql_async::prelude::Queryable;
            for sql in [
                "DROP DATABASE IF EXISTS _qi_mysql_itest",
                "CREATE DATABASE _qi_mysql_itest CHARACTER SET utf8mb4",
            ] {
                conn.query_drop(sql).await.expect("建库失败");
            }
            conn.query_drop(
                "CREATE TABLE _qi_mysql_itest._qi_itest (\
                     id INT AUTO_INCREMENT PRIMARY KEY,\
                     name VARCHAR(64),\
                     score DOUBLE,\
                     flag BOOLEAN,\
                     ts DATETIME,\
                     amount DECIMAL(10,2),\
                     extra JSON)",
            )
            .await
            .expect("建表失败");
            for i in 0..120 {
                let score = if i % 7 == 0 {
                    format!("{:.1}", -1.5 + i as f64)
                } else {
                    format!("{}", i)
                };
                let flag = if i % 3 == 0 { "1" } else { "0" };
                let ts = format!("2026-01-{} 08:30:00", (i % 28) + 1);
                let amount = format!("{}.{}", i * 3, i % 100);
                let extra = format!("{{\"k\":{}}}", i);
                conn.exec_drop(
                    "INSERT INTO _qi_mysql_itest._qi_itest(name, score, flag, ts, amount, extra) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                    (format!("row-{}", i), score, flag, ts, amount, extra),
                )
                .await
                .expect("insert 失败");
            }
        })
    })
    .join()
    .expect("建表线程 panic");
    println!("[2] 建库 _qi_mysql_itest + 表 _qi_itest + 120 行 OK");

    // ── 3. 走 core::mysql 全流程（独立线程，模拟 UI 调用方式）──
    let handle = std::thread::spawn(move || {
        let mut viewer = MySqlViewer {
            source: src,
            ..Default::default()
        };

        // 3a. connect：分级列表应含 _qi_mysql_itest → _qi_itest
        viewer.connect().expect("MySqlViewer::connect 失败");
        let found = viewer
            .databases
            .iter()
            .any(|(db, tables)| db == "_qi_mysql_itest" && tables.iter().any(|t| t == "_qi_itest"));
        assert!(
            found,
            "分级列表未找到 _qi_mysql_itest._qi_itest: {:?}",
            viewer.databases
        );
        println!(
            "[3a] connect 分级列表 OK（{} 个库）",
            viewer.databases.len()
        );

        // 3b. set_selected_table 第 0 页：50 行、列名、总行数 120
        let tref = TableRef {
            database: "_qi_mysql_itest".into(),
            table: "_qi_itest".into(),
        };
        viewer
            .set_selected_table(tref.clone(), 0)
            .expect("set_selected_table(0) 失败");
        assert_eq!(viewer.table_data.len(), PAGE_SIZE);
        assert_eq!(viewer.row_count, 120);
        assert_eq!(
            viewer.column_names,
            vec!["id", "name", "score", "flag", "ts", "amount", "extra"]
        );
        println!(
            "[3b] 第 0 页 OK：{} 行 / 总 {} / 列 {:?}",
            viewer.table_data.len(),
            viewer.row_count,
            viewer.column_names
        );

        // 3c. 末页（offset=100）：剩余 20 行；值文本化抽查
        let page0 = viewer.table_data.clone();
        viewer
            .set_selected_table(tref.clone(), 100)
            .expect("set_selected_table(100) 失败");
        assert_eq!(viewer.table_data.len(), 20);
        assert_eq!(viewer.table_data[0][1], "row-100");
        let row0 = &viewer.table_data[0];
        // DATETIME / DECIMAL / JSON 应为规范文本而非占位
        assert!(
            row0[4].contains("2026-01-") && row0[4].contains("08:30:00"),
            "datetime 未文本化: {:?}",
            row0[4]
        );
        assert!(!row0[5].starts_with('<'), "decimal 未文本化: {:?}", row0[5]);
        assert!(
            row0[6].starts_with('{') && row0[6].contains("\"k\":"),
            "json 未文本化: {:?}",
            row0[6]
        );
        println!(
            "[3c] 末页 OK：{} 行；ts={:?} amount={:?} extra={:?}",
            viewer.table_data.len(),
            row0[4],
            row0[5],
            row0[6]
        );

        // 3d. 页缓存命中：同 viewer 回取第 0 页应命中缓存，与首读完全一致
        viewer
            .set_selected_table(tref.clone(), 0)
            .expect("缓存回读失败");
        assert_eq!(viewer.table_data, page0);
        println!("[3d] 页缓存命中 OK（同参数二次读取与首读一致）");

        // 3e. execute_sql 写语句：UPDATE + 受影响行数提示
        let status = viewer
            .execute_sql("UPDATE _qi_mysql_itest._qi_itest SET flag = 1 WHERE id <= 10")
            .expect("execute_sql(UPDATE) 失败");
        assert!(!status.is_empty(), "UPDATE 应返回受影响行数提示");
        println!("[3e] execute_sql(UPDATE) OK：{}", status);

        // 3f. execute_sql 查询：SELECT 带结果集
        viewer
            .execute_sql("SELECT id, name FROM _qi_mysql_itest._qi_itest WHERE id <= 3 ORDER BY id")
            .expect("execute_sql(SELECT) 失败");
        assert_eq!(viewer.table_data.len(), 3);
        assert_eq!(viewer.column_names, vec!["id", "name"]);
        println!(
            "[3f] execute_sql(SELECT) OK：3 行，列 {:?}",
            viewer.column_names
        );

        // 3g. 导出取数：export_begin + export_batch 覆盖 120 行
        let (cols, total) = viewer.export_begin(&tref).expect("export_begin 失败");
        assert_eq!(total, 120);
        assert_eq!(
            cols,
            vec!["id", "name", "score", "flag", "ts", "amount", "extra"]
        );
        let mut rows = 0;
        let mut off = 0;
        loop {
            let batch = viewer
                .export_batch(&tref, off, 50)
                .expect("export_batch 失败");
            let n = batch.len();
            if n == 0 {
                break;
            }
            rows += n;
            off += n;
            if off >= total {
                break;
            }
        }
        assert_eq!(rows, 120, "导出应覆盖全部 120 行");
        println!("[3g] 导出取数 OK：{} 行全覆盖", rows);

        // 3h. 清理
        viewer
            .execute_sql("DROP DATABASE _qi_mysql_itest")
            .expect("drop 清理失败");
        println!("[4] 清理 _qi_mysql_itest OK");
    });
    handle.join().expect("验证线程 panic");

    // ── 5. disconnect（连接池关闭不 panic）──
    qi_toolbox::core::mysql::disconnect();
    println!("\n=== MySQL 集成验证全部通过 ===");
}

/// 密码 URL 转义（冒烟直连用；空密码原样）
fn urlencoding_pass(pass: &str) -> String {
    pass.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-.~_".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{:02X}", b)
            }
        })
        .collect()
}

/// 解析 mysql://user:pass@host:port/db 形式的连接串（QI_MYSQL_URL 用）
fn parse_mysql_url(url: &str) -> Option<MySqlSource> {
    let rest = url.strip_prefix("mysql://")?;
    let (authority, _db) = rest.split_once('/').unwrap_or((rest, ""));
    // 密码可能含 '@'：按最后一个 '@' 定位
    let at = authority.rfind('@')?;
    let (userinfo, hostport) = authority.split_at(at);
    let (user, pass) = userinfo.split_once(':').unwrap_or((userinfo, ""));
    let (host, port) = hostport
        .strip_prefix('@')?
        .split_once(':')
        .unwrap_or((hostport.strip_prefix('@')?, "3306"));
    Some(MySqlSource {
        host: host.into(),
        port: port.into(),
        user: user.into(),
        pass: pass.into(),
    })
}

#[test]
fn integration_smoke() {
    main();
}
