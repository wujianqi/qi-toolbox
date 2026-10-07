//! 临时集成验证（测完即删）：走 core::pg 真实路径验证
//! 连接 / connect 分级列表 / 建表插数 / set_selected_table 分页 / execute_sql / 导出取数。
//! i18n 需先 install（lang::* 依赖 Catalog）。

#[path = "../src/i18n/lang.rs"]
mod lang;

use tokio_postgres::NoTls;

fn main() {
    lang::install();
    // 连接串优先取 QI_PG_URL（CI 传入），默认为本机开发实例
    let url = std::env::var("QI_PG_URL")
        .unwrap_or_else(|_| "postgres://sa:q@127.0.0.1:5432/postgres".into());
    // ── 1. 直连冒烟：服务可达、凭据正确 ──
    // 连接与查询须在同一 runtime 内完成（conn 是连接驱动任务，drop 即断连挂死查询）
    let url_owned = url.to_string();
    let ver = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime 失败");
        rt.block_on(async move {
            let (client, conn) = tokio_postgres::connect(&url_owned, NoTls)
                .await
                .expect("PG 连接失败（服务未启动或凭据错误）");
            tokio::spawn(conn); // 驱动任务挂到本 runtime，block_on 期间被驱动
            let v: String = client
                .query_one("SELECT version()", &[])
                .await
                .expect("SELECT version 失败")
                .get(0);
            v
        })
    });
    let v = ver.join().expect("冒烟线程 panic");
    println!("[1] 连接 OK: {}", v.split(',').next().unwrap_or(""));

    // ── 2. 建测试表 + 插数（同一模式）──
    let url_owned = url.to_string();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime 失败");
        rt.block_on(async move {
            let (client, conn) = tokio_postgres::connect(&url_owned, NoTls)
                .await
                .expect("PG 连接失败");
            tokio::spawn(conn);
            client
                .execute("DROP TABLE IF EXISTS _qi_pg_itest", &[])
                .await
                .expect("drop 失败");
            client
                .execute(
                    "CREATE TABLE _qi_pg_itest (id serial PRIMARY KEY, name text, score double precision, flag boolean, ts timestamp, amount numeric(10,2), extra jsonb)",
                    &[],
                )
                .await
                .expect("create 失败");
            for i in 0..120 {
                let score: f64 = if i % 7 == 0 { -1.5 + i as f64 } else { i as f64 };
                let name = format!("row-{}", i);
                let flag = if i % 3 == 0 { "true" } else { "false" };
                let ts = format!("2026-01-{} 08:30:00", (i % 28) + 1);
                let amount = format!("{}.{}", i * 3, i % 100);
                let extra = format!("{{\"k\":{}}}", i);
                // 字面量 INSERT（simple_query 无参数绑定，不受二进制协议类型限制）
                client
                    .simple_query(&format!(
                        "INSERT INTO _qi_pg_itest(name, score, flag, ts, amount, extra) \
                         VALUES('{}', {}, {}, '{}', {}, '{}')",
                        name, score, flag, ts, amount, extra
                    ))
                    .await
                    .expect("insert 失败");
            }
        })
    })
    .join()
    .expect("建表线程 panic");
    println!("[2] 建表 _qi_pg_itest + 120 行 OK");

    // ── 3. 走 core::pg 全流程（在独立线程 + 块内 runtime，模拟 UI 调用方式）──
    let url_owned = url.to_string();
    let handle = std::thread::spawn(move || {
        let _rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // 3a. connect：严格按连接串匹配——分级列表应含 public → _qi_pg_itest
        let mut viewer = qi_toolbox::core::pg::PgViewer {
            source: url_owned.clone(),
            ..Default::default()
        };
        viewer.connect().expect("PgViewer::connect 失败");
        let found = viewer
            .schemas
            .iter()
            .any(|(s, tables)| s == "public" && tables.iter().any(|t| t == "_qi_pg_itest"));
        assert!(
            found,
            "分级列表未找到 public._qi_pg_itest: {:?}",
            viewer.schemas
        );
        println!(
            "[3a] connect 分级列表 OK（{} 个 schema）",
            viewer.schemas.len()
        );

        // 3b. set_selected_table 第 0 页：50 行、列名、总行数 120
        let tref = qi_toolbox::core::pg::TableRef {
            db: "postgres".into(),
            schema: "public".into(),
            table: "_qi_pg_itest".into(),
        };
        viewer
            .set_selected_table(tref.clone(), 0)
            .expect("set_selected_table(0) 失败");
        assert_eq!(viewer.table_data.len(), qi_toolbox::core::pg::PAGE_SIZE);
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

        // 3c. 第 2 页：剩余 20 行；值文本化抽查（布尔/数值/负数）
        let page0 = viewer.table_data.clone(); // 先留第 0 页快照供 3d 缓存回读比对
        viewer
            .set_selected_table(tref.clone(), 100)
            .expect("set_selected_table(100) 失败");
        assert_eq!(viewer.table_data.len(), 20);
        assert_eq!(viewer.table_data[0][1], "row-100");
        // 文本协议抽查：timestamp/numeric/jsonb 应为规范文本而非 <类型名> 占位
        let row0 = &viewer.table_data[0];
        assert!(
            row0[4].contains("2026-01-") && row0[4].contains("08:30:00"),
            "timestamp 未文本化: {:?}",
            row0[4]
        );
        assert!(!row0[5].starts_with('<'), "numeric 未文本化: {:?}", row0[5]);
        assert!(
            row0[6].starts_with('{') && row0[6].contains("\"k\":"),
            "jsonb 未文本化: {:?}",
            row0[6]
        );
        println!(
            "[3c] 第 2 页 OK：{} 行；ts={:?} amount={:?} extra={:?}",
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
            .execute_sql("UPDATE _qi_pg_itest SET flag = true WHERE id <= 10")
            .expect("execute_sql(UPDATE) 失败");
        assert!(!status.is_empty(), "UPDATE 应返回受影响行数提示");
        println!("[3e] execute_sql(UPDATE) OK：{}", status);

        // 3f. execute_sql 查询：SELECT 带结果集
        viewer
            .execute_sql("SELECT id, name FROM _qi_pg_itest WHERE id <= 3 ORDER BY id")
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
            .execute_sql("DROP TABLE _qi_pg_itest")
            .expect("drop 清理失败");
        println!("[4] 清理 _qi_pg_itest OK");
    });
    handle.join().expect("验证线程 panic");

    // ── 5. disconnect（缓存连接关闭不 panic）──
    qi_toolbox::core::pg::invalidate();
    println!("\n=== PG 集成验证全部通过 ===");
}

#[test]
fn integration_smoke() {
    main();
}
