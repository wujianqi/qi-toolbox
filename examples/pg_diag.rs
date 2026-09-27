//! 临时诊断（用完即删）：列出服务器上所有非系统 schema 的 relkind 分布
use tokio_postgres::NoTls;

fn main() {
    // 默认 postgres 库；可传参指定其它库（诊断"连了 A 库却找 B 库的表"类问题）
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "postgres://sa:q@127.0.0.1:5432/postgres".into());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (client, conn) = tokio_postgres::connect(&url, NoTls)
                .await
                .expect("连接失败");
            tokio::spawn(conn);
            let rows = client
                .query(
                    "SELECT n.nspname, c.relkind, count(*) \
                     FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname NOT IN ('pg_catalog','information_schema') \
                       AND n.nspname NOT LIKE 'pg_toast%' \
                     GROUP BY 1,2 ORDER BY 1,2",
                    &[],
                )
                .await
                .expect("查询失败");
            println!("schema | relkind | count");
            for r in &rows {
                let k: i8 = r.get(1);
                let kind = match u8::try_from(k).unwrap_or(b'?') {
                    b'r' => "r(普通表)",
                    b'p' => "p(分区表)",
                    b'v' => "v(视图)",
                    b'f' => "f(外表)",
                    b'm' => "m(物化视图)",
                    b'S' => "S(序列)",
                    b'i' => "i(索引)",
                    _ => "?",
                };
                let n: String = r.get(0);
                let c: i64 = r.get(2);
                println!("{} | {} | {}", n, kind, c);
            }
            if rows.is_empty() {
                println!("（该数据库在非系统 schema 下没有任何对象）");
            }
            let dbs = client
                .query(
                    "SELECT datname FROM pg_database WHERE NOT datistemplate",
                    &[],
                )
                .await
                .unwrap();
            println!("\n可用数据库：");
            for d in &dbs {
                let n: String = d.get(0);
                println!("  {}", n);
            }
        })
    })
    .join()
    .unwrap();
}
