//! 临时诊断（用完即删）：带约束的表浏览复现
//! 建 PK/FK/CHECK/UNIQUE/DEFAULT 表 → PgViewer connect / set_selected_table
use qi_toolbox::core::pg::{PgViewer, TableRef};

fn main() {
    let url = "postgres://sa:q@127.0.0.1:5432/kitxtest";

    // 准备：建带约束的表
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (c, conn) = tokio_postgres::connect(url, tokio_postgres::NoTls)
                .await
                .expect("连接失败");
            tokio::spawn(conn);
            for sql in [
                "DROP TABLE IF EXISTS _qi_child",
                "DROP TABLE IF EXISTS _qi_parent",
                "CREATE TABLE _qi_parent (id serial PRIMARY KEY, code varchar(10) UNIQUE NOT NULL, price numeric(8,2) CHECK (price >= 0) DEFAULT 0)",
                "CREATE TABLE _qi_child (id serial PRIMARY KEY, pid integer NOT NULL REFERENCES _qi_parent(id) ON DELETE CASCADE, note text, CONSTRAINT uq_child UNIQUE(pid, note))",
                "INSERT INTO _qi_parent(code, price) VALUES ('A1', 9.9), ('B2', 0)",
                "INSERT INTO _qi_child(pid, note) VALUES (1, 'x'), (1, 'y')",
            ] {
                if let Err(e) = c.simple_query(sql).await {
                    panic!("准备失败 {:?}: {}", sql.lines().next().unwrap_or(""), e);
                }
            }
            println!("[prep] 带约束表已建（PK/FK/CHECK/UNIQUE/DEFAULT）");
        })
    })
    .join()
    .unwrap();

    // 走 UI 同款调用：connect → 逐表 set_selected_table（用 kitxtest 真实表）
    let url_owned = url.to_string();
    let handle = std::thread::spawn(move || {
        let mut v = PgViewer {
            source: url_owned,
            ..Default::default()
        };
        v.connect().expect("connect 失败");
        let list: Vec<(String, Vec<String>)> = v.schemas.clone();
        for (schema, tables) in &list {
            for t in tables {
                let tref = TableRef {
                    db: String::new(),
                    schema: schema.clone(),
                    table: t.clone(),
                };
                match v.set_selected_table(tref, 0) {
                    Ok(()) => println!(
                        "[ok] {}.{}：{} 行 / {} 列",
                        schema,
                        t,
                        v.row_count,
                        v.column_names.len()
                    ),
                    Err(e) => println!("[FAIL] {}.{}：{}", schema, t, e),
                }
            }
        }
    });
    handle.join().unwrap();
}
