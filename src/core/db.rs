//! 数据库后台任务编排（业务层，不含任何 UI 代码）
//!
//! Turso 页的后台线程协议：`DbMsg` 定义后台线程 → UI 线程的消息，
//! `TablePage` 是查询结果的纯数据快照；`spawn_*` 在独立线程中执行
//! 连接/加载表/SQL，结果经 [`MsgSink`] 投递回 UI 线程（由 UI 层注入实现）。

use crate::core::mysql::{MySqlSource, MySqlViewer, TableRef as MySqlTableRef};
use crate::core::pg::{PgSource, PgViewer, TableRef as PgTableRef};
use crate::core::turso::{TursoSource, TursoViewer};
use crate::core::MsgSink;
use crate::lang;

/// 分级表列表：(组名 → 组内表名列表)——MySQL/PG 左侧树用
pub type GroupedTables = Vec<(String, Vec<String>)>;

/// 数据库后台操作结果（经 channel 回传 UI 线程）
#[derive(Clone)]
pub enum DbMsg {
    /// 连接结果：(表列表, 显示文件名)
    Connected(Result<(Vec<String>, String), String>),
    /// 分级连接结果：(库/schema → 表 列表, 显示名)——MySQL/PG 用
    ConnectedGroups(Result<(GroupedTables, String), String>),
    /// 表数据加载（点击表名/刷新）结果
    TableLoaded(Result<TablePage, String>),
    /// SQL 执行结果
    SqlDone(Result<TablePage, String>),
    /// 导出全表 CSV 进度：done=已完成行数，total=总行数；total==0 时不定
    ExportProgress { done: usize, total: usize },
    /// 导出完成（成功=文件路径）
    ExportDone(Result<String, String>),
}

/// 数据库连接源（三种后端的统一包装，供通用 spawn_* 分发）
#[derive(Clone)]
pub enum DbSource {
    Turso(TursoSource),
    MySql(MySqlSource),
    Pg(PgSource),
}

/// 判断 SQL 是否为写语句（首关键字粗判，只读模式拦截用）。
/// SELECT / WITH(CTE 查询) / EXPLAIN / SHOW / DESCRIBE / DESC / PRAGMA 视为读，
/// 其余（INSERT/UPDATE/DELETE/CREATE/DROP/ALTER/TRUNCATE/REPLACE/MERGE/
/// GRANT/REVOKE/SET/VACUUM/BEGIN/COMMIT/ROLLBACK/CALL/USE…）一律视为写。
pub fn is_write_sql(sql: &str) -> bool {
    let first = sql
        .trim_start()
        .trim_start_matches('(')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(';')
        .to_ascii_lowercase();
    !matches!(
        first.as_str(),
        "select" | "with" | "explain" | "show" | "describe" | "desc" | "pragma" | ""
    )
}

/// CSV 单元格转义：含逗号/引号/换行的值用双引号包裹，内部引号翻倍；
/// 公式前缀（= + @ 或负号开头的纯公式）加单引号前缀中和，防电子表格打开时被当公式执行
pub(crate) fn csv_field(v: &str) -> String {
    // 防公式注入：Excel/LibreOffice 会把 =、+、@ 开头的单元格当公式执行；
    // '-' 单独放开（负数是合法数据），但 - 后跟字母视为公式前缀同样中和
    let formula_lead = matches!(v.chars().next(), Some('=') | Some('+') | Some('@'))
        || (v.starts_with('-')
            && v[1..]
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false));
    if formula_lead {
        return format!("'{}", v);
    }
    if v.contains(',') || v.contains('"') || v.contains('\n') || v.contains('\r') {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

/// 导出批量大小：每批拉取行数（远大于浏览的 PAGE_SIZE，减少远程往返）
const EXPORT_BATCH: usize = 2000;

/// SQL 字面量转义：NULL 原样，其余单引号包裹（内部引号翻倍）。
/// 值均来自后端取数的文本化结果（不可注入来源），仅做 SQL 语法层转义
pub(crate) fn sql_literal(v: &str) -> String {
    if v == "NULL" {
        "NULL".to_string()
    } else {
        format!("'{}'", v.replace('\'', "''"))
    }
}

/// 单条 INSERT 语句合成（列名已引号包裹，行值经 sql_literal 转义）
fn insert_stmt(qualified: &str, cols: &[String], row: &[String]) -> String {
    let vals: Vec<String> = row.iter().map(|v| sql_literal(v)).collect();
    format!(
        "INSERT INTO {} ({}) VALUES ({});",
        qualified,
        cols.join(", "),
        vals.join(", ")
    )
}

/// 导出统一取行器：keyset 优先（单列主键游标，O(n)），失败/无主键回退 OFFSET。
/// MySQL/PG 用；Turso 已有 rowid keyset 版。
enum BatchFetcher {
    MySql(MySqlViewer, MySqlTableRef),
    Pg(PgViewer, PgTableRef),
}

impl BatchFetcher {
    /// 取下一批：返回 None = 该后端不支持 keyset（走 OFFSET）
    fn next_batch(
        &mut self,
        pk: Option<&str>,
        cursor: &mut Option<String>,
        offset: usize,
        keyset_ok: &mut bool,
    ) -> Result<Vec<Vec<String>>, String> {
        match self {
            BatchFetcher::MySql(v, tref) => match pk {
                Some(col) if *keyset_ok => {
                    match v.export_batch_keyset(tref, col, cursor.clone(), EXPORT_BATCH) {
                        Ok(rows) => {
                            // 游标推进：取主键列在本批末行的值
                            if let Some(last) = rows.last() {
                                if let Some(idx) = v.column_names.iter().position(|c| c == col) {
                                    *cursor = last.get(idx).cloned();
                                }
                            }
                            Ok(rows)
                        }
                        Err(_) => {
                            *keyset_ok = false;
                            Ok(Vec::new())
                        }
                    }
                }
                _ => v.export_batch(tref, offset, EXPORT_BATCH),
            },
            BatchFetcher::Pg(v, tref) => match pk {
                Some(col) if *keyset_ok => {
                    match v.export_batch_keyset(
                        &tref.schema,
                        &tref.table,
                        col,
                        cursor.clone(),
                        EXPORT_BATCH,
                    ) {
                        Ok(rows) => {
                            if let Some(last) = rows.last() {
                                if let Some(idx) = v.column_names.iter().position(|c| c == col) {
                                    *cursor = last.get(idx).cloned();
                                }
                            }
                            Ok(rows)
                        }
                        Err(_) => {
                            *keyset_ok = false;
                            Ok(Vec::new())
                        }
                    }
                }
                _ => v.export_batch(tref, offset, EXPORT_BATCH),
            },
        }
    }
}

/// 后台线程：整库导出建表结构 DDL（建表语句，schema-only），结果经 sink 回传。
/// MySQL/PG 整库 = 该库/schema 全部表 DDL + 视图；Turso = sqlite_master 全部条目。
pub fn spawn_export_schema(
    sink: MsgSink<DbMsg>,
    source: DbSource,
    group: String,
    dest: std::path::PathBuf,
) {
    std::thread::spawn(move || {
        let sink = std::sync::Arc::new(std::sync::Mutex::new(sink));
        let emit = std::sync::Arc::new(move |msg: DbMsg| {
            if let Ok(s) = sink.lock() {
                s(msg);
            }
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let ddl: Vec<String> = match &source {
                DbSource::Turso(_) => {
                    let mut viewer = TursoViewer {
                        source: match source {
                            DbSource::Turso(s) => s,
                            _ => unreachable!(),
                        },
                        ..Default::default()
                    };
                    viewer.export_ddl(None)?
                }
                DbSource::MySql(_) => {
                    let mut viewer = MySqlViewer {
                        source: match source {
                            DbSource::MySql(s) => s,
                            _ => unreachable!(),
                        },
                        ..Default::default()
                    };
                    viewer.export_ddl(&group, None)?
                }
                DbSource::Pg(_) => {
                    let mut viewer = PgViewer {
                        source: match source {
                            DbSource::Pg(s) => s,
                            _ => unreachable!(),
                        },
                        ..Default::default()
                    };
                    viewer.export_ddl(&group, None)?
                }
            };
            if ddl.is_empty() {
                return Err(lang::TURSO_EXPORT_NO_COLS());
            }
            std::fs::write(&dest, ddl.join("\n"))
                .map_err(|e| lang::ERR_EXPORT_OPEN(e.to_string()))?;
            Ok::<_, String>(dest.to_string_lossy().to_string())
        }));
        let msg = match result {
            Ok(Ok(path)) => DbMsg::ExportDone(Ok(path)),
            Ok(Err(e)) => DbMsg::ExportDone(Err(e)),
            Err(_) => DbMsg::ExportDone(Err(lang::MYSQL_UNKNOWN_PANIC())),
        };
        emit(msg);
    });
}

/// 后台线程：单表导出为 SQL INSERT 脚本（分批流式写盘 + 进度汇报），
/// 支持三种后端（Turso 走 rowid 游标，MySQL/PG 走主键游标、OFFSET 回退），结果经 sink 回传
pub fn spawn_export_sql(
    sink: MsgSink<DbMsg>,
    source: DbSource,
    group: String,
    table: String,
    dest: std::path::PathBuf,
) {
    std::thread::spawn(move || {
        let sink = std::sync::Arc::new(std::sync::Mutex::new(sink));
        let emit = std::sync::Arc::new(move |msg: DbMsg| {
            if let Ok(s) = sink.lock() {
                s(msg);
            }
        });
        let emit_progress = emit.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let display_name = format!("{}.{}", group, table);
            // 统一取行器 + 列名/总数（Turso 保留自身 rowid keyset 路径）
            let (cols, total, mut fetcher, mut turso) = match &source {
                DbSource::Turso(src) => {
                    let mut viewer = TursoViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    let (cols, total) = viewer.export_begin(&table)?;
                    (cols, total, None, Some(viewer))
                }
                DbSource::MySql(src) => {
                    let mut viewer = MySqlViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    let tref = MySqlTableRef {
                        database: group.clone(),
                        table: table.clone(),
                    };
                    let (cols, total) = viewer.export_begin(&tref)?;
                    let pk = viewer.export_pk(&tref).unwrap_or(None);
                    (
                        cols,
                        total,
                        Some((BatchFetcher::MySql(viewer, tref), pk)),
                        None,
                    )
                }
                DbSource::Pg(src) => {
                    let mut viewer = PgViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    let tref = PgTableRef {
                        db: String::new(),
                        schema: group.clone(),
                        table: table.clone(),
                    };
                    let (cols, total) = viewer.export_begin(&tref)?;
                    let pk = viewer.export_pk(&group, &table).unwrap_or(None);
                    (
                        cols,
                        total,
                        Some((BatchFetcher::Pg(viewer, tref), pk)),
                        None,
                    )
                }
            };
            if cols.is_empty() {
                return Err(lang::TURSO_EXPORT_NO_COLS());
            }
            // 列名引号包裹（sql_literal 只管值；列名按各自方言引号包裹已由上层做不了——
            // 这里统一用双引号，MySQL 在 ANSI_QUOTES 外的默认模式下不识别，
            // 故 MySQL 单独用反引号）
            let quoted_cols: Vec<String> = match &source {
                DbSource::MySql(_) => cols
                    .iter()
                    .map(|c| format!("`{}`", c.replace('`', "``")))
                    .collect(),
                _ => cols
                    .iter()
                    .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
                    .collect(),
            };
            let qualified = match &source {
                DbSource::MySql(_) => format!(
                    "`{}`.`{}`",
                    group.replace('`', "``"),
                    table.replace('`', "``")
                ),
                DbSource::Pg(_) => format!(
                    "\"{}\".\"{}\"",
                    group.replace('"', "\"\""),
                    table.replace('"', "\"\"")
                ),
                DbSource::Turso(_) => format!("\"{}\"", table.replace('"', "\"\"")),
            };

            use std::io::Write;
            let file =
                std::fs::File::create(&dest).map_err(|e| lang::ERR_EXPORT_OPEN(e.to_string()))?;
            let mut w = std::io::BufWriter::new(file);
            // 事务包裹：目标库导入更快且失败可整体回滚（SQLite 方言 BEGIN 也通用）
            writeln!(w, "BEGIN TRANSACTION;").map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;

            let mut done = 0usize;
            let mut last_report = std::time::Instant::now();
            let mut cursor: Option<String> = None; // 主键/rowid 游标
            let mut keyset_ok = true; // keyset 查询失败后回退 OFFSET
            let mut turso_rowid: i64 = 0;
            let mut turso_keyset = true;
            while done < total {
                let rows = if let Some(v) = &mut turso {
                    // Turso：rowid keyset 优先，失败回退 OFFSET（同 CSV 导出逻辑）
                    if turso_keyset {
                        match v.export_batch_keyset(&table, turso_rowid, EXPORT_BATCH) {
                            Ok((rows, Some(last))) => {
                                turso_rowid = last;
                                rows
                            }
                            Ok((rows, None)) => {
                                if rows.is_empty() {
                                    break;
                                }
                                rows
                            }
                            Err(_) => {
                                turso_keyset = false;
                                continue;
                            }
                        }
                    } else {
                        let rows = v.export_batch(&table, done, EXPORT_BATCH)?;
                        if rows.is_empty() {
                            return Err(lang::TURSO_EXPORT_TRUNCATED(format!(
                                "{}/{}",
                                done, total
                            )));
                        }
                        rows
                    }
                } else if let Some((fetcher, pk)) = &mut fetcher {
                    let rows =
                        fetcher.next_batch(pk.as_deref(), &mut cursor, done, &mut keyset_ok)?;
                    if rows.is_empty() {
                        if keyset_ok {
                            // keyset 空批 = 到表尾（COUNT 与数据的自然偏差视为完成）
                            break;
                        }
                        // OFFSET 空批：数据比 COUNT 少，报错而非静默导出残缺文件
                        return Err(lang::TURSO_EXPORT_TRUNCATED(format!("{}/{}", done, total)));
                    }
                    rows
                } else {
                    break;
                };
                for row in &rows {
                    writeln!(w, "{}", insert_stmt(&qualified, &quoted_cols, row))
                        .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;
                }
                done += rows.len();
                if last_report.elapsed() >= std::time::Duration::from_millis(100) || done >= total {
                    emit_progress(DbMsg::ExportProgress { done, total });
                    last_report = std::time::Instant::now();
                }
            }
            writeln!(w, "COMMIT;").map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;
            w.flush()
                .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;
            Ok::<_, String>(format!("{} ({})", dest.to_string_lossy(), display_name))
        }));
        let msg = match result {
            Ok(Ok(path)) => DbMsg::ExportDone(Ok(path)),
            Ok(Err(e)) => DbMsg::ExportDone(Err(e)),
            Err(_) => DbMsg::ExportDone(Err(lang::MYSQL_UNKNOWN_PANIC())),
        };
        emit(msg);
    });
}

/// 后台线程：分批导出整表为 CSV（每批 EXPORT_BATCH 行，经 sink 汇报进度），结果经 sink 回传
pub fn spawn_export_csv(
    sink: MsgSink<DbMsg>,
    source: TursoSource,
    table: String,
    dest: std::path::PathBuf,
) {
    std::thread::spawn(move || {
        // MsgSink 不可克隆：用 Arc<Mutex> 共享；emit 也包成 Arc 便于闭包多处捕获
        let sink = std::sync::Arc::new(std::sync::Mutex::new(sink));
        let emit = std::sync::Arc::new(move |msg: DbMsg| {
            if let Ok(s) = sink.lock() {
                s(msg);
            }
        });
        let emit_progress = emit.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut viewer = TursoViewer {
                source,
                ..Default::default()
            };
            // 导出专用入口：列名 + 实时 COUNT(*)，不读浏览缓存（缓存可能过期/被 MAX_TABLE_ROWS 截断）
            let (cols, total) = viewer.export_begin(&table)?;
            if cols.is_empty() {
                return Err(lang::TURSO_EXPORT_NO_COLS().to_string());
            }

            use std::io::Write;
            let file =
                std::fs::File::create(&dest).map_err(|e| lang::ERR_EXPORT_OPEN(e.to_string()))?;
            let mut w = std::io::BufWriter::new(file);

            // 表头
            let header: Vec<String> = cols.iter().map(|c| csv_field(c)).collect();
            writeln!(w, "{}", header.join(","))
                .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;

            let mut done = 0usize;
            let mut last_report = std::time::Instant::now();
            // 分批拉取：优先 keyset（rowid 游标，O(n) 且不受中途增删行影响）；
            // WITHOUT ROWID 表等场景 keyset 查询失败时回退 OFFSET 定位。
            let mut cursor: i64 = 0; // keyset 游标：已读到的最大 rowid
            let mut keyset = true; // false = 已回退 OFFSET 模式
            while done < total {
                let rows = if keyset {
                    match viewer.export_batch_keyset(&table, cursor, EXPORT_BATCH) {
                        Ok((rows, Some(last))) => {
                            cursor = last;
                            rows
                        }
                        // 空批 = keyset 已到表尾（正常结束；done<total 属 COUNT 与数据
                        // 自然偏差，keyset 语义下视为导出完成）
                        Ok((rows, None)) => {
                            if rows.is_empty() {
                                break;
                            }
                            rows
                        }
                        // 查询失败（如 WITHOUT ROWID 表无 rowid 列）：回退 OFFSET
                        Err(_) => {
                            keyset = false;
                            continue;
                        }
                    }
                } else {
                    let rows = viewer.export_batch(&table, done, EXPORT_BATCH)?;
                    // OFFSET 模式数据比 COUNT 少（并发删行/页被截断）：报错而非静默导出残缺文件
                    if rows.is_empty() {
                        return Err(lang::TURSO_EXPORT_TRUNCATED(format!("{}/{}", done, total)));
                    }
                    rows
                };
                for row in &rows {
                    let line: Vec<String> = row.iter().map(|v| csv_field(v)).collect();
                    writeln!(w, "{}", line.join(","))
                        .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;
                }
                done += rows.len();
                // 进度节流：最多每 100ms 发一条，避免 UI 消息洪泛
                if last_report.elapsed() >= std::time::Duration::from_millis(100) || done >= total {
                    emit_progress(DbMsg::ExportProgress { done, total });
                    last_report = std::time::Instant::now();
                }
            }
            w.flush()
                .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;
            Ok::<_, String>(dest.to_string_lossy().to_string())
        }));
        let msg = match result {
            Ok(Ok(path)) => DbMsg::ExportDone(Ok(path)),
            Ok(Err(e)) => DbMsg::ExportDone(Err(e)),
            Err(_) => DbMsg::ExportDone(Err(lang::TURSO_UNKNOWN_PANIC().to_string())),
        };
        emit(msg);
    });
}

/// 表格数据快照：整表数据（全量行，虚拟滚动按需渲染）+ 元信息
#[derive(Clone)]
pub struct TablePage {
    /// 当前表名；None = SQL 查询结果
    pub table_name: Option<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// 各列是否在列表视图显示（列设置面板勾选）
    pub visible: Vec<bool>,
    /// 详情视图行号
    pub selected_row: Option<usize>,
    pub row_count: usize,
    /// 当前分页偏移（0 起，每页 PAGE_SIZE 行）
    pub page_offset: usize,
    /// SQL 写语句执行后的状态提示（受影响行数等；None=无）
    pub sql_status: Option<String>,
}

impl TablePage {
    pub fn empty() -> Self {
        Self {
            table_name: None,
            columns: Vec::new(),
            rows: Vec::new(),
            visible: Vec::new(),
            selected_row: None,
            row_count: 0,
            page_offset: 0,
            sql_status: None,
        }
    }
}

impl Default for TablePage {
    fn default() -> Self {
        Self::empty()
    }
}

/// 从 viewer 快照表格状态
fn snapshot(viewer: &TursoViewer) -> TablePage {
    TablePage {
        table_name: viewer.selected_table.clone(),
        columns: viewer.column_names.clone(),
        rows: viewer.table_data.clone(),
        visible: vec![true; viewer.column_names.len()],
        selected_row: None,
        row_count: viewer.row_count,
        page_offset: viewer.page_offset,
        sql_status: None,
    }
}

/// 从 MySqlViewer 快照表格状态（table_name = "库.表" 展示键）
fn mysql_snapshot(viewer: &MySqlViewer) -> TablePage {
    TablePage {
        table_name: viewer
            .selected
            .as_ref()
            .map(|t| format!("{}.{}", t.database, t.table)),
        columns: viewer.column_names.clone(),
        rows: viewer.table_data.clone(),
        visible: vec![true; viewer.column_names.len()],
        selected_row: None,
        row_count: viewer.row_count,
        page_offset: viewer.page_offset,
        sql_status: None,
    }
}

/// 从 PgViewer 快照表格状态（table_name = "schema.表" 展示键）
fn pg_snapshot(viewer: &PgViewer) -> TablePage {
    TablePage {
        table_name: viewer
            .selected
            .as_ref()
            .map(|t| format!("{}.{}", t.schema, t.table)),
        columns: viewer.column_names.clone(),
        rows: viewer.table_data.clone(),
        visible: vec![true; viewer.column_names.len()],
        selected_row: None,
        row_count: viewer.row_count,
        page_offset: viewer.page_offset,
        sql_status: None,
    }
}

/// 后台线程：连接数据库，结果经 sink 回传
pub fn spawn_connect(sink: MsgSink<DbMsg>, source: TursoSource) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            // 展示名：本地=文件名，远程=URL
            let fname = match &source {
                TursoSource::Local(p) => std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.clone()),
                TursoSource::Remote { url, .. } => url.clone(),
            };
            let mut viewer = TursoViewer {
                source,
                ..Default::default()
            };
            viewer.connect()?;
            Ok::<_, String>((viewer.tables.clone(), fname))
        }));
        let msg = match result {
            Ok(Ok(v)) => DbMsg::Connected(Ok(v)),
            Ok(Err(e)) => DbMsg::Connected(Err(e)),
            Err(_) => DbMsg::Connected(Err(lang::TURSO_UNKNOWN_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：加载表数据（点击表名/刷新/翻页），结果经 sink 回传
pub fn spawn_load_table(sink: MsgSink<DbMsg>, source: TursoSource, table: String, offset: usize) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut viewer = TursoViewer {
                source,
                ..Default::default()
            };
            viewer.set_selected_table(table, offset)?;
            Ok::<_, String>(snapshot(&viewer))
        }));
        let msg = match result {
            Ok(Ok(page)) => DbMsg::TableLoaded(Ok(page)),
            Ok(Err(e)) => DbMsg::TableLoaded(Err(e)),
            Err(_) => DbMsg::TableLoaded(Err(lang::TURSO_UNKNOWN_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：执行 SQL，结果经 sink 回传（三种后端统一入口）
pub fn spawn_execute_sql(sink: MsgSink<DbMsg>, source: DbSource, sql: String) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || match source {
            DbSource::Turso(src) => {
                let mut viewer = TursoViewer {
                    source: src,
                    ..Default::default()
                };
                viewer.execute_sql(&sql)?;
                Ok::<_, String>(snapshot(&viewer))
            }
            DbSource::MySql(src) => {
                let mut viewer = MySqlViewer {
                    source: src,
                    ..Default::default()
                };
                viewer.execute_sql(&sql)?;
                Ok::<_, String>(mysql_snapshot(&viewer))
            }
            DbSource::Pg(src) => {
                let mut viewer = PgViewer {
                    source: src,
                    ..Default::default()
                };
                let status = viewer.execute_sql(&sql)?;
                let mut page = pg_snapshot(&viewer);
                page.sql_status = Some(status);
                Ok(page)
            }
        }));
        let msg = match result {
            Ok(Ok(page)) => DbMsg::SqlDone(Ok(page)),
            Ok(Err(e)) => DbMsg::SqlDone(Err(e)),
            Err(_) => DbMsg::SqlDone(Err(lang::TURSO_UNKNOWN_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：连接 MySQL/PG，加载分级列表（库/schema → 表），结果经 sink 回传
pub fn spawn_connect_grouped(sink: MsgSink<DbMsg>, source: DbSource) {
    std::thread::spawn(move || {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || match &source {
                DbSource::MySql(src) => {
                    let disp = format!(
                        "{}:{}/{}",
                        src.host.trim(),
                        src.port.trim(),
                        src.user.trim()
                    );
                    let mut viewer = MySqlViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    viewer.connect()?;
                    Ok::<_, String>((viewer.databases.clone(), disp))
                }
                DbSource::Pg(src) => {
                    let disp = crate::core::pg::display_of(src);
                    let mut viewer = PgViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    viewer.connect()?;
                    Ok((viewer.schemas.clone(), disp))
                }
                DbSource::Turso(_) => Err(lang::ERR_CONNECT_DB(
                    "internal: turso uses spawn_connect".to_string(),
                )),
            }));
        let msg = match result {
            Ok(Ok(v)) => DbMsg::ConnectedGroups(Ok(v)),
            Ok(Err(e)) => DbMsg::ConnectedGroups(Err(e)),
            Err(_) => DbMsg::ConnectedGroups(Err(lang::MYSQL_UNKNOWN_PANIC())),
        };
        sink(msg);
    });
}

/// 后台线程：加载 MySQL/PG 分级列表中选中的表（group=库/schema，table=表名），
/// 结果经 sink 回传。`group.table` 作为 TablePage.table_name 展示。
pub fn spawn_load_table_grouped(
    sink: MsgSink<DbMsg>,
    source: DbSource,
    group: String,
    table: String,
    offset: usize,
) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            match &source {
                DbSource::MySql(src) => {
                    let mut viewer = MySqlViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    viewer.set_selected_table(
                        MySqlTableRef {
                            database: group,
                            table,
                        },
                        offset,
                    )?;
                    Ok::<_, String>(mysql_snapshot(&viewer))
                }
                DbSource::Pg(src) => {
                    let mut viewer = PgViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    viewer.set_selected_table(
                        PgTableRef {
                            db: String::new(), // 严格按连接串的库：不跨库重写
                            schema: group.clone(),
                            table,
                        },
                        offset,
                    )?;
                    Ok(pg_snapshot(&viewer))
                }
                DbSource::Turso(_) => Err(lang::ERR_QUERY_DATA(
                    "internal: turso uses spawn_load_table".to_string(),
                )),
            }
        }));
        let msg = match result {
            Ok(Ok(page)) => DbMsg::TableLoaded(Ok(page)),
            Ok(Err(e)) => DbMsg::TableLoaded(Err(e)),
            Err(_) => DbMsg::TableLoaded(Err(lang::MYSQL_UNKNOWN_PANIC())),
        };
        sink(msg);
    });
}

/// 后台线程：分批导出 MySQL/PG 整表为 CSV（复用 turso 版的节流框架；
/// MySQL/PG 走主键游标 keyset 分批（O(n)），无主键/keyset 失败回退 OFFSET），结果经 sink 回传
pub fn spawn_export_csv_grouped(
    sink: MsgSink<DbMsg>,
    source: DbSource,
    group: String,
    table: String,
    dest: std::path::PathBuf,
) {
    std::thread::spawn(move || {
        let sink = std::sync::Arc::new(std::sync::Mutex::new(sink));
        let emit = std::sync::Arc::new(move |msg: DbMsg| {
            if let Ok(s) = sink.lock() {
                s(msg);
            }
        });
        let emit_progress = emit.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let display_name = format!("{}.{}", group, table);
            let (cols, total, fetcher) = match &source {
                DbSource::MySql(src) => {
                    let mut viewer = MySqlViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    let tref = MySqlTableRef {
                        database: group.clone(),
                        table: table.clone(),
                    };
                    let (cols, total) = viewer.export_begin(&tref)?;
                    let pk = viewer.export_pk(&tref).unwrap_or(None);
                    (cols, total, Some((BatchFetcher::MySql(viewer, tref), pk)))
                }
                DbSource::Pg(src) => {
                    let mut viewer = PgViewer {
                        source: src.clone(),
                        ..Default::default()
                    };
                    let tref = PgTableRef {
                        db: String::new(), // 严格按连接串的库：不跨库重写
                        schema: group.clone(),
                        table: table.clone(),
                    };
                    let (cols, total) = viewer.export_begin(&tref)?;
                    let pk = viewer.export_pk(&group, &table).unwrap_or(None);
                    (cols, total, Some((BatchFetcher::Pg(viewer, tref), pk)))
                }
                DbSource::Turso(_) => {
                    return Err(lang::ERR_EXPORT_OPEN(
                        "internal: turso uses spawn_export_csv".to_string(),
                    ))
                }
            };
            if cols.is_empty() {
                return Err(lang::TURSO_EXPORT_NO_COLS());
            }

            use std::io::Write;
            let file =
                std::fs::File::create(&dest).map_err(|e| lang::ERR_EXPORT_OPEN(e.to_string()))?;
            let mut w = std::io::BufWriter::new(file);

            let header: Vec<String> = cols.iter().map(|c| csv_field(c)).collect();
            writeln!(w, "{}", header.join(","))
                .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;

            let mut done = 0usize;
            let mut last_report = std::time::Instant::now();
            let mut cursor: Option<String> = None;
            let mut keyset_ok = true;
            let Some((mut fetcher, pk)) = fetcher else {
                return Err(lang::TURSO_EXPORT_NO_COLS());
            };
            while done < total {
                let rows = fetcher.next_batch(pk.as_deref(), &mut cursor, done, &mut keyset_ok)?;
                if rows.is_empty() {
                    if keyset_ok {
                        // keyset 空批 = 到表尾（COUNT 与数据的自然偏差视为完成）
                        break;
                    }
                    // OFFSET 空批：数据比 COUNT 少（并发删行等）：报错而非静默导出残缺文件
                    return Err(lang::TURSO_EXPORT_TRUNCATED(format!("{}/{}", done, total)));
                }
                for row in &rows {
                    let line: Vec<String> = row.iter().map(|v| csv_field(v)).collect();
                    writeln!(w, "{}", line.join(","))
                        .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;
                }
                done += rows.len();
                if last_report.elapsed() >= std::time::Duration::from_millis(100) || done >= total {
                    emit_progress(DbMsg::ExportProgress { done, total });
                    last_report = std::time::Instant::now();
                }
            }
            w.flush()
                .map_err(|e| lang::ERR_EXPORT_WRITE(e.to_string()))?;
            Ok::<_, String>(format!("{} ({})", dest.to_string_lossy(), display_name))
        }));
        let msg = match result {
            Ok(Ok(path)) => DbMsg::ExportDone(Ok(path)),
            Ok(Err(e)) => DbMsg::ExportDone(Err(e)),
            Err(_) => DbMsg::ExportDone(Err(lang::MYSQL_UNKNOWN_PANIC())),
        };
        emit(msg);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_field_plain_value_unchanged() {
        assert_eq!(csv_field("abc"), "abc");
        assert_eq!(csv_field("123"), "123");
        assert_eq!(csv_field(""), "");
    }

    #[test]
    fn csv_field_quotes_specials() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_field("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn csv_field_neutralizes_formula_prefix() {
        assert_eq!(csv_field("=cmd"), "'=cmd");
        assert_eq!(csv_field("+1+1"), "'+1+1");
        assert_eq!(csv_field("@x"), "'@x");
        // 负号后跟字母才中和（防公式注入），普通负数保留
        assert_eq!(csv_field("-cmd"), "'-cmd");
        // 普通负数不该被中和
        assert_eq!(csv_field("-2"), "-2");
        assert_eq!(csv_field("-2+3"), "-2+3");
    }

    #[test]
    fn sql_literal_null_and_escape() {
        assert_eq!(sql_literal("NULL"), "NULL");
        assert_eq!(sql_literal("abc"), "'abc'");
        assert_eq!(sql_literal("o'clock"), "'o''clock'");
    }
}
