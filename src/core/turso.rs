//! Turso 数据库连接与查询（业务层，不含任何 UI 代码）
//!
//! 连接/查询一律在独立线程中执行（见 [`run_query`]），UI 线程只收结果。
//! 同一连接源复用 [`DB_CACHE`] 缓存，避免重复打开大库造成的卡顿。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use turso::Builder;
use crate::lang;

/// 单次查询最大返回行数（防无 LIMIT 查询打爆内存）
pub const MAX_TABLE_ROWS: usize = 100_000;

/// 表格分页大小（每页行数）
pub const PAGE_SIZE: usize = 50;

/// 单元格值 → 文本（NULL/二进制安全降级）。
/// TEXT 值**完整保留**：行详情视图需要全文；超长文本的展示限长在渲染层做
/// （见 ui/table.rs 的 `cell_display`），业务层不再截断。
fn value_to_string(row: &turso::Row, idx: usize) -> String {
    match row.get_value(idx) {
        Ok(turso::Value::Null) => "NULL".to_string(),
        Ok(turso::Value::Integer(i)) => i.to_string(),
        Ok(turso::Value::Real(f)) => f.to_string(),
        Ok(turso::Value::Text(t)) => t,
        Ok(turso::Value::Blob(b)) => format!("<blob {} bytes>", b.len()),
        Err(_) => "ERR".to_string(),
    }
}

/// 数据库浏览器的数据视图状态（业务侧快照，供 UI 层做表格渲染数据源）
#[derive(Clone)]
pub struct TursoViewer {
    pub source: TursoSource,
    pub tables: Vec<String>,
    pub selected_table: Option<String>,
    pub table_data: Vec<Vec<String>>,
    pub column_names: Vec<String>,
    pub visible_columns: Vec<bool>,
    pub selected_row: Option<usize>,
    pub page_offset: usize,
    pub row_count: usize,
    pub error_message: Option<String>,
}

impl Default for TursoViewer {
    fn default() -> Self {
        Self {
            source: TursoSource::Local(String::from("server/resources/cms.db")),
            tables: Vec::new(),
            selected_table: None,
            table_data: Vec::new(),
            column_names: Vec::new(),
            visible_columns: Vec::new(),
            selected_row: None,
            page_offset: 0,
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

/// 数据库连接来源：本地文件 或 网络（Turso/libSQL 远程库，经 sync 引擎拉取）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TursoSource {
    Local(String),
    Remote { url: String, token: String },
}

impl TursoSource {
    /// 缓存键：本地=规范化绝对路径，远程=url::token。
    pub fn cache_key(&self) -> String {
        match self {
            TursoSource::Local(p) => {
                let abs = if std::path::Path::new(p).is_absolute() {
                    p.clone()
                } else {
                    std::env::current_dir()
                        .map(|d| d.join(p).to_string_lossy().to_string())
                        .unwrap_or_else(|_| p.clone())
                };
                if cfg!(windows) {
                    abs.replace('\\', "/")
                } else {
                    abs
                }
            }
            TursoSource::Remote { url, token } => format!("remote://{}::{}", url, token),
        }
    }
}

/// 已打开的数据库缓存：同一连接源只 build 一次，后续查询复用连接，
/// 避免每次点击表名/翻页/SQL 都重新打开数据库（大库打开慢是卡顿元凶）。
/// 本地与远程（sync）的 Database 类型不同，用 enum 收纳。
enum CachedDb {
    Local(turso::Database),
    Remote(turso::sync::Database),
}

static DB_CACHE: std::sync::Mutex<Option<(String, CachedDb)>> =
    std::sync::Mutex::new(None);

/// 浏览缓存有效期：期间内重复浏览/翻回同一页直接命中（本地查询极快，
/// 15s 的陈旧窗口体感不可感知；远端副本与外部改动以「刷新」为准）
const CACHE_TTL: Duration = Duration::from_secs(15);
/// 页缓存容量上限：超出即整体清空（简化 LRU，浏览场景足够）
const PAGE_CACHE_MAX: usize = 128;
/// 行数缓存容量上限（同上）
const COUNT_CACHE_MAX: usize = 128;

type PageCacheKey = (String, String, usize);
type CountCacheKey = (String, String);
type PageCacheMap = HashMap<PageCacheKey, (Instant, Vec<String>, Vec<Vec<String>>, usize)>;
type CountCacheMap = HashMap<CountCacheKey, (Instant, usize)>;

/// 页快照缓存：(源键, 表名, 偏移) → (时刻, 列, 行, 总行数)。
/// 命中直接回填 viewer，零查询；翻回已看过的页 / 来回翻页秒开。
/// HashMap::new 非 const，故用 OnceLock 惰性初始化（首用时才建表）。
static PAGE_CACHE: std::sync::OnceLock<std::sync::Mutex<PageCacheMap>> =
    std::sync::OnceLock::new();

/// 表总行数缓存：(源键, 表名) → (时刻, 总行数)。
/// 命中时翻页/加载跳过昂贵的 COUNT(*)，大表收益明显。
static COUNT_CACHE: std::sync::OnceLock<std::sync::Mutex<CountCacheMap>> =
    std::sync::OnceLock::new();

fn page_cache() -> &'static std::sync::Mutex<PageCacheMap> {
    PAGE_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn count_cache() -> &'static std::sync::Mutex<CountCacheMap> {
    COUNT_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 清空浏览缓存（断开/重连/刷新/写语句成功后调用；数据可能已变化）
pub fn invalidate() {
    if let Ok(mut c) = page_cache().lock() {
        c.clear();
    }
    if let Ok(mut c) = count_cache().lock() {
        c.clear();
    }
}

/// 断开连接：释放缓存的数据库句柄并清空浏览缓存（下一次查询/连接重新 build）。
pub fn disconnect() {
    if let Ok(mut cache) = DB_CACHE.lock() {
        *cache = None;
    }
    invalidate();
}

/// 在独立线程中执行数据库查询，返回结果
fn run_query<F>(source: &TursoSource, f: F) -> Result<QueryResult, String>
where
    F: FnOnce(turso::Connection) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<QueryResult, String>> + Send>>
        + Send
        + 'static,
{
    // 后台线程内要按 source 构建连接：借用参数不能逃出函数，先克隆为自有值
    let source = source.clone();

    // 缓存键：本地=规范化绝对路径，远程=url::token
    let key = source.cache_key();

    // 优先复用已打开的数据库（取锁克隆，锁不跨查询持有）；锁被污染时按未缓存处理
    let cached = match DB_CACHE.lock() {
        Ok(cache) => match &*cache {
            Some((p, db)) if *p == key => Some(match db {
                CachedDb::Local(d) => CachedDb::Local(d.clone()),
                CachedDb::Remote(d) => CachedDb::Remote(d.clone()),
            }),
            _ => None,
        },
        Err(_) => None,
    };

    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| lang::ERR_RUNTIME().replace("{}", &e.to_string()))?;

        rt.block_on(async {
            let conn = match cached {
                Some(CachedDb::Local(db)) => db
                    .connect()
                    .map_err(|e| lang::ERR_GET_CONN().replace("{}", &e.to_string()))?,
                Some(CachedDb::Remote(db)) => db
                    .connect()
                    .await
                    .map_err(|e| lang::ERR_GET_CONN().replace("{}", &e.to_string()))?,
                None => match source {
                    // 本地文件：直接打开
                    TursoSource::Local(_) => {
                        let db = Builder::new_local(&key)
                            .experimental_index_method(true)
                            .build()
                            .await
                            .map_err(|e| lang::ERR_CONNECT_DB().replace("{}", &e.to_string()))?;
                        if let Ok(mut cache) = DB_CACHE.lock() {
                            *cache = Some((key.clone(), CachedDb::Local(db.clone())));
                        }
                        db.connect()
                            .map_err(|e| lang::ERR_GET_CONN().replace("{}", &e.to_string()))?
                    }
                    // 网络：内存副本 + 远程同步（首次连接自动拉取 schema 与数据）
                    TursoSource::Remote { url, token } => {
                        let mut b = turso::sync::Builder::new_remote(":memory:")
                            .with_remote_url(url.clone())
                            .experimental_index_method(true);
                        if !token.is_empty() {
                            b = b.with_auth_token(token.clone());
                        }
                        let db = b
                            .build()
                            .await
                            .map_err(|e| lang::ERR_CONNECT_DB().replace("{}", &e.to_string()))?;
                        if let Ok(mut cache) = DB_CACHE.lock() {
                            *cache = Some((key.clone(), CachedDb::Remote(db.clone())));
                        }
                        db.connect()
                            .await
                            .map_err(|e| lang::ERR_GET_CONN().replace("{}", &e.to_string()))?
                    }
                },
            };

            f(conn).await
        })
    });

    match handle.join() {
        Ok(r) => r,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                lang::ERR_THREAD_PANIC().replace("{}", s)
            } else if let Some(s) = e.downcast_ref::<String>() {
                lang::ERR_THREAD_PANIC().replace("{}", s)
            } else {
                lang::ERR_THREAD_PANIC_UNKNOWN().to_string()
            };
            Err(msg)
        }
    }
}

impl TursoViewer {
    /// 连接数据库并加载表列表（不自动加载任何表数据，点击表名时再异步加载）
    pub fn connect(&mut self) -> Result<(), String> {
        let source = self.source.clone();
        let (tables, _, _, _, _, err) = run_query(&source, |conn| {
            Box::pin(async move {
                // 只加载表列表，立即返回
                let mut rows = conn
                    .query(
                        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                        Vec::<turso::Value>::new(),
                    )
                    .await
                    .map_err(|e| lang::ERR_QUERY_TABLES().replace("{}", &e.to_string()))?;

                let mut tables = Vec::new();
                while let Ok(Some(row)) = rows.next().await {
                    let value = value_to_string(&row, 0);
                    if value != "NULL" && value != "ERR" {
                        tables.push(value);
                    }
                }
                Ok((tables, None, Vec::new(), Vec::new(), 0, None))
            })
        })?;

        self.tables = tables;
        self.selected_table = None;
        self.table_data = Vec::new();
        self.column_names = Vec::new();
        self.visible_columns = Vec::new();
        self.selected_row = None;
        self.page_offset = 0;
        self.row_count = 0;
        self.error_message = err;
        Ok(())
    }

    /// 设置选定的表并加载指定页数据（页/行数带 TTL 缓存：命中零查询）
    pub fn set_selected_table(&mut self, table_name: String, offset: usize) -> Result<(), String> {
        let source = self.source.clone();
        let name = table_name.clone();
        // 标识符双引号转义：表名来自 sqlite_master，可能含引号/空格等特殊字符
        let quoted = format!("\"{}\"", name.replace('"', "\"\""));
        let src_key = source.cache_key();

        // ── 页缓存命中：翻回已看过的页（同源同表同偏移）直接回填，零查询 ──
        if let Ok(cache) = page_cache().lock() {
            if let Some((at, cols, rows, row_count)) =
                cache.get(&(src_key.clone(), name.clone(), offset))
            {
                if at.elapsed() < CACHE_TTL {
                    let (cols, rows, row_count) = (cols.clone(), rows.clone(), *row_count);
                    self.selected_table = Some(name);
                    self.table_data = rows;
                    self.column_names = cols.clone();
                    self.visible_columns = vec![true; cols.len()];
                    self.selected_row = None;
                    self.page_offset = offset;
                    self.row_count = row_count;
                    self.error_message = None;
                    return Ok(());
                }
            }
        }

        // ── 表总行数缓存：翻页/加载时跳过昂贵的 COUNT(*)（TTL 内）──
        let cached_count = match count_cache().lock() {
            Ok(c) => c
                .get(&(src_key.clone(), name.clone()))
                .and_then(|(at, n)| if at.elapsed() < CACHE_TTL { Some(*n) } else { None }),
            Err(_) => None,
        };

        let (_, selected, data, cols, row_count, err) = {
            let src_key_c = src_key.clone();
            let name_c = name.clone();
            run_query(&source, move |conn| {
                Box::pin(async move {
                    let mut rows = conn
                        .query(
                            &format!("PRAGMA table_info({})", quoted),
                            Vec::<turso::Value>::new(),
                        )
                        .await
                        .map_err(|e| lang::ERR_TABLE_INFO().replace("{}", &e.to_string()))?;
                    let mut cols = Vec::new();
                    while let Ok(Some(row)) = rows.next().await {
                        let n = value_to_string(&row, 1);
                        if n != "NULL" && n != "ERR" { cols.push(n); }
                    }

                    // 行数：命中缓存直接用；未命中才 COUNT 并回填缓存
                    let row_count: usize = match cached_count {
                        Some(n) => n,
                        None => {
                            let n: usize = match conn
                                .query(
                                    &format!("SELECT COUNT(*) FROM {}", quoted),
                                    Vec::<turso::Value>::new(),
                                )
                                .await
                            {
                                Ok(mut r) => {
                                    if let Ok(Some(row)) = r.next().await {
                                        let s = value_to_string(&row, 0);
                                        s.parse::<usize>().unwrap_or(0)
                                    } else {
                                        0
                                    }
                                }
                                Err(_) => 0,
                            };
                            if let Ok(mut cc) = count_cache().lock() {
                                cc.insert(
                                    (src_key_c.clone(), name_c.clone()),
                                    (Instant::now(), n),
                                );
                                if cc.len() > COUNT_CACHE_MAX {
                                    cc.clear();
                                }
                            }
                            n
                        }
                    };

                    // 分页加载：每页 PAGE_SIZE 行，OFFSET 定位页（虚拟滚动只构建视口内的行）
                    let mut rows = conn
                        .query(
                            &format!(
                                "SELECT * FROM {} LIMIT {} OFFSET {}",
                                quoted, PAGE_SIZE, offset
                            ),
                            Vec::<turso::Value>::new(),
                        )
                        .await
                        .map_err(|e| lang::ERR_QUERY_DATA().replace("{}", &e.to_string()))?;
                    let mut data = Vec::new();
                    while let Ok(Some(row)) = rows.next().await {
                        let mut rd = Vec::new();
                        for i in 0..cols.len() {
                            rd.push(value_to_string(&row, i));
                        }
                        data.push(rd);
                    }

                    Ok((Vec::new(), Some(name_c.clone()), data, cols, row_count, None))
                })
            })
        }?;

        // ── 回填页缓存（含列与总行数，翻回即命中）──
        if let Ok(mut cache) = page_cache().lock() {
            cache.insert(
                (src_key, name.clone(), offset),
                (Instant::now(), cols.clone(), data.clone(), row_count),
            );
            if cache.len() > PAGE_CACHE_MAX {
                cache.clear();
            }
        }

        self.selected_table = selected;
        self.table_data = data;
        self.column_names = cols.clone();
        self.visible_columns = vec![true; cols.len()];
        self.selected_row = None;
        self.page_offset = offset;
        self.row_count = row_count;
        self.error_message = err;
        Ok(())
    }

    /// 执行任意 SQL 查询，将结果写入 table_data / column_names / row_count
    pub fn execute_sql(&mut self, sql: &str) -> Result<(), String> {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return Err(lang::ERR_EMPTY_SQL().to_string());
        }

        let source = self.source.clone();
        let is_select = {
            let upper = sql.to_uppercase();
            upper.starts_with("SELECT")
                || upper.starts_with("PRAGMA")
                || upper.starts_with("EXPLAIN")
        };
        let (_, _, data, cols, row_count, err) = run_query(&source, move |conn| {
            Box::pin(async move {
                if is_select {
                    let mut rows = conn
                        .query(&sql, Vec::<turso::Value>::new())
                        .await
                        .map_err(|e| lang::ERR_QUERY_FAIL().replace("{}", &e.to_string()))?;

                    // 尝试从第一行列推断列名；限制返回行数防止无 LIMIT 查询打爆内存
                    let mut cols = Vec::new();
                    let mut data = Vec::new();
                    let mut first = true;
                    while let Ok(Some(row)) = rows.next().await {
                        if data.len() >= MAX_TABLE_ROWS {
                            break;
                        }
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
                        .map_err(|e| lang::ERR_EXEC_FAIL().replace("{}", &e.to_string()))?;
                    Ok((Vec::new(), None, Vec::new(), Vec::new(), 0, None))
                }
            })
        })?;
        // 写语句（非 SELECT/PRAGMA/EXPLAIN）执行成功：数据已变，浏览缓存整体失效
        if !is_select {
            invalidate();
        }

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
}
