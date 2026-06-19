//! Turso 数据库查看器模块

use turso::Builder;
use crate::strings::lang;

/// 安全地将 turso Value 转为 String，不 panic
/// 使用 catch_unwind 防止 turso crate 内部 unreachable panic 传播
fn value_to_string(row: &turso::Row, idx: usize) -> String {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        row.get_value(idx)
    }));
    match result {
        Ok(Ok(turso::Value::Null)) => "NULL".to_string(),
        Ok(Ok(turso::Value::Integer(v))) => v.to_string(),
        Ok(Ok(turso::Value::Real(v))) => v.to_string(),
        Ok(Ok(turso::Value::Text(v))) => v,
        Ok(Ok(turso::Value::Blob(v))) => format!("[{} bytes]", v.len()),
        Ok(Err(_)) => "ERR".to_string(),
        Err(_) => "PANIC".to_string(),
    }
}

pub struct TursoViewer {
    pub db_path: String,
    pub tables: Vec<String>,
    pub selected_table: Option<String>,
    pub table_data: Vec<Vec<String>>,
    pub column_names: Vec<String>,
    pub visible_columns: Vec<bool>,
    pub selected_row: Option<usize>,
    pub page_offset: usize,
    pub page_size: usize,
    pub row_count: usize,
    pub error_message: Option<String>,
}

impl Clone for TursoViewer {
    fn clone(&self) -> Self {
        Self {
            db_path: self.db_path.clone(),
            tables: self.tables.clone(),
            selected_table: self.selected_table.clone(),
            table_data: self.table_data.clone(),
            column_names: self.column_names.clone(),
            visible_columns: self.visible_columns.clone(),
            selected_row: self.selected_row,
            page_offset: self.page_offset,
            page_size: self.page_size,
            row_count: self.row_count,
            error_message: self.error_message.clone(),
        }
    }
}

impl Default for TursoViewer {
    fn default() -> Self {
        Self {
            db_path: String::from("server/resources/cms.db"),
            tables: Vec::new(),
            selected_table: None,
            table_data: Vec::new(),
            column_names: Vec::new(),
            visible_columns: Vec::new(),
            selected_row: None,
            page_offset: 0,
            page_size: 500,
            row_count: 0,
            error_message: None,
        }
    }
}

type QueryResult = (
    Vec<String>,
    Option<String>,
    Vec<Vec<String>>,
    Vec<String>,
    usize,
    Option<String>,
);

/// 在独立线程中执行数据库查询，返回结果
fn run_query<F>(db_path: &str, f: F) -> Result<QueryResult, String>
where
    F: FnOnce(turso::Connection) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<QueryResult, String>> + Send>>
        + Send
        + 'static,
{
    let path = if std::path::Path::new(db_path).is_absolute() {
        db_path.to_string()
    } else {
        std::env::current_dir()
            .map_err(|e| lang::ERR_DIR.replace("{}", &e.to_string()))?
            .join(db_path)
            .to_string_lossy()
            .to_string()
    };

    let normalized = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path
    };

    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| lang::ERR_RUNTIME.replace("{}", &e.to_string()))?;

        rt.block_on(async {
            let db = Builder::new_local(&normalized)
                .experimental_index_method(true)
                .build()
                .await
                .map_err(|e| lang::ERR_CONNECT_DB.replace("{}", &e.to_string()))?;

            let conn = db.connect()
                .map_err(|e| lang::ERR_GET_CONN.replace("{}", &e.to_string()))?;

            f(conn).await
        })
    });

    match handle.join() {
        Ok(r) => r,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                lang::ERR_THREAD_PANIC.replace("{}", s)
            } else if let Some(s) = e.downcast_ref::<String>() {
                lang::ERR_THREAD_PANIC.replace("{}", s)
            } else {
                lang::ERR_THREAD_PANIC_UNKNOWN.to_string()
            };
            Err(msg)
        }
    }
}

impl TursoViewer {
    /// 连接数据库并加载表列表
    pub fn connect(&mut self) -> Result<(), String> {
        let db_path = self.db_path.clone();
        let (tables, selected, data, cols, row_count, err) = run_query(&db_path, |conn| {
            Box::pin(async move {
                // 加载表列表
                let mut rows = conn
                    .query(
                        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                        Vec::<turso::Value>::new(),
                    )
                    .await
                    .map_err(|e| lang::ERR_QUERY_TABLES.replace("{}", &e.to_string()))?;

                let mut tables = Vec::new();
                while let Ok(Some(row)) = rows.next().await {
                    let value = value_to_string(&row, 0);
                    if value != "NULL" && value != "ERR" {
                        tables.push(value);
                    }
                }

                let selected = tables.first().cloned();

                // 加载第一个表
                if let Some(ref name) = selected {
                    let mut rows = conn
                        .query(&format!("PRAGMA table_info({})", name), Vec::<turso::Value>::new())
                        .await
                        .map_err(|e| lang::ERR_TABLE_INFO.replace("{}", &e.to_string()))?;
                    let mut cols = Vec::new();
                    while let Ok(Some(row)) = rows.next().await {
                        let n = value_to_string(&row, 1);
                        if n != "NULL" && n != "ERR" { cols.push(n); }
                    }

                    let row_count: usize = match conn
                        .query(&format!("SELECT COUNT(*) FROM {}", name), Vec::<turso::Value>::new())
                        .await
                    {
                        Ok(mut r) => {
                            if let Ok(Some(row)) = r.next().await {
                                let s = value_to_string(&row, 0);
                                s.parse::<usize>().unwrap_or(0)
                            } else { 0 }
                        }
                        Err(_) => 0,
                    };

                    let mut rows = conn
                        .query(&format!("SELECT * FROM {} LIMIT 500", name), Vec::<turso::Value>::new())
                        .await
                        .map_err(|e| lang::ERR_QUERY_DATA.replace("{}", &e.to_string()))?;
                    let mut data = Vec::new();
                    while let Ok(Some(row)) = rows.next().await {
                        let mut rd = Vec::new();
                        for i in 0..cols.len() {
                            rd.push(value_to_string(&row, i));
                        }
                        data.push(rd);
                    }

                    Ok((tables, selected, data, cols, row_count, None))
                } else {
                    Ok((tables, None, Vec::new(), Vec::new(), 0, None))
                }
            })
        })?;

        self.tables = tables;
        self.selected_table = selected;
        self.table_data = data;
        self.column_names = cols.clone();
        self.visible_columns = vec![true; cols.len()];
        self.selected_row = None;
        self.page_offset = 0;
        self.row_count = row_count;
        self.error_message = err;
        Ok(())
    }

    /// 设置选定的表并加载数据
    pub fn set_selected_table(&mut self, table_name: String) -> Result<(), String> {
        let db_path = self.db_path.clone();
        let name = table_name.clone();
        let (_, selected, data, cols, row_count, err) = run_query(&db_path, move |conn| {
            Box::pin(async move {
                let mut rows = conn
                    .query(&format!("PRAGMA table_info({})", name), Vec::<turso::Value>::new())
                    .await
                    .map_err(|e| lang::ERR_TABLE_INFO.replace("{}", &e.to_string()))?;
                let mut cols = Vec::new();
                while let Ok(Some(row)) = rows.next().await {
                    let n = value_to_string(&row, 1);
                    if n != "NULL" && n != "ERR" { cols.push(n); }
                }

                let row_count: usize = match conn
                    .query(&format!("SELECT COUNT(*) FROM {}", name), Vec::<turso::Value>::new())
                    .await
                {
                    Ok(mut r) => {
                        if let Ok(Some(row)) = r.next().await {
                            let s = value_to_string(&row, 0);
                            s.parse::<usize>().unwrap_or(0)
                        } else { 0 }
                    }
                    Err(_) => 0,
                };

                let mut rows = conn
                    .query(&format!("SELECT * FROM {} LIMIT 500", name), Vec::<turso::Value>::new())
                    .await
                    .map_err(|e| lang::ERR_QUERY_DATA.replace("{}", &e.to_string()))?;
                let mut data = Vec::new();
                while let Ok(Some(row)) = rows.next().await {
                    let mut rd = Vec::new();
                    for i in 0..cols.len() {
                        rd.push(value_to_string(&row, i));
                    }
                    data.push(rd);
                }

                Ok((Vec::new(), Some(name.clone()), data, cols, row_count, None))
            })
        })?;

        self.selected_table = selected;
        self.table_data = data;
        self.column_names = cols.clone();
        self.visible_columns = vec![true; cols.len()];
        self.selected_row = None;
        self.page_offset = 0;
        self.row_count = row_count;
        self.error_message = err;
        Ok(())
    }

    /// 执行任意 SQL 查询，将结果写入 table_data / column_names / row_count
    pub fn execute_sql(&mut self, sql: &str) -> Result<(), String> {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return Err(lang::ERR_EMPTY_SQL.to_string());
        }

        let db_path = self.db_path.clone();
        let (_, _, data, cols, row_count, err) = run_query(&db_path, move |conn| {
            Box::pin(async move {
                let upper = sql.to_uppercase();
                let is_select = upper.starts_with("SELECT") || upper.starts_with("PRAGMA") || upper.starts_with("EXPLAIN");

                if is_select {
                    let mut rows = conn
                        .query(&sql, Vec::<turso::Value>::new())
                        .await
                        .map_err(|e| lang::ERR_QUERY_FAIL.replace("{}", &e.to_string()))?;

                    // 尝试从第一行列推断列名
                    let mut cols = Vec::new();
                    let mut data = Vec::new();
                    let mut first = true;
                    while let Ok(Some(row)) = rows.next().await {
                        if first {
                            // 列数 = column_count
                            for i in 0.. {
                                match row.get_value(i) {
                                    Ok(_) => cols.push(format!("col{}", i)),
                                    Err(_) => break,
                                }
                            }
                            first = false;
                        }
                        let mut rd = Vec::new();
                        for i in 0..cols.len() {
                            rd.push(value_to_string(&row, i));
                        }
                        data.push(rd);
                    }
                    let row_count = data.len();
                    Ok((Vec::new(), None, data, cols, row_count, None))
                } else {
                    // 非 SELECT 语句（INSERT/UPDATE/DELETE 等）
                    conn.execute(&sql, Vec::<turso::Value>::new())
                        .await
                        .map_err(|e| lang::ERR_EXEC_FAIL.replace("{}", &e.to_string()))?;
                    Ok((Vec::new(), None, Vec::new(), Vec::new(), 0, None))
                }
            })
        })?;

        self.table_data = data;
        self.column_names = cols;
        self.visible_columns = vec![true; self.column_names.len()];
        self.row_count = row_count;
        self.selected_table = None;
        self.selected_row = None;
        self.page_offset = 0;
        self.error_message = err;
        Ok(())
    }

    /// 翻页加载数据
    pub fn load_page(&mut self, offset: usize) -> Result<(), String> {
        let table_name = match self.selected_table.clone() {
            Some(name) => name,
            None => return Ok(()),
        };
        let db_path = self.db_path.clone();
        let name = table_name;
        let page_size = self.page_size;
        let num_cols = self.column_names.len();
        let (_, _, data, _, _, err) = run_query(&db_path, move |conn| {
            Box::pin(async move {
                let mut rows = conn
                    .query(&format!("SELECT * FROM {} LIMIT {} OFFSET {}", name, page_size, offset), Vec::<turso::Value>::new())
                    .await
                    .map_err(|e| lang::ERR_QUERY_DATA.replace("{}", &e.to_string()))?;

                let mut data = Vec::new();
                while let Ok(Some(row)) = rows.next().await {
                    let mut rd = Vec::new();
                    for i in 0..num_cols {
                        rd.push(value_to_string(&row, i));
                    }
                    data.push(rd);
                }

                Ok((Vec::new(), None, data, Vec::new(), 0, None))
            })
        })?;

        self.table_data = data;
        self.page_offset = offset;
        self.selected_row = None;
        self.error_message = err;
        Ok(())
    }
}
