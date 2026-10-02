//! MySQL 数据库连接与查询（业务层，不含任何 UI 代码）
//!
//! 功能与 [`crate::core::turso`] 对齐：连接缓存 + 页/行数 TTL 缓存 + 分页浏览
//! 及 SQL 执行、CSV 导出。查询在独立线程中执行（每条命令一个 current_thread
//! runtime + 连接池缓存），UI 线程只收结果。
//!
//! 与 Turso 的差异：左侧列表**分级**——第一层为数据库（schema），第二层为表。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use mysql_async::prelude::Queryable;
use mysql_async::{OptsBuilder, Pool};

use crate::lang;

/// 单次查询最大返回行数（防无 LIMIT 查询打爆内存）
pub const MAX_TABLE_ROWS: usize = 100_000;

/// 表格分页大小（每页行数）
pub const PAGE_SIZE: usize = 50;

/// 连接参数（不走 URL：密码含特殊字符无需转义）
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MySqlSource {
    pub host: String,
    pub port: String,
    pub user: String,
    pub pass: String,
}

impl MySqlSource {
    /// 缓存键（不含密码——缓存仅进程内使用）
    fn key(&self) -> String {
        format!("mysql://{}@{}:{}", self.user, self.host, self.port)
    }

    fn port_num(&self) -> u16 {
        self.port.trim().parse().unwrap_or(3306)
    }
}

/// 已打开的连接池缓存：同一连接参数只 build 一次，后续查询复用。
/// Pool 可克隆（内部 Arc），clone 不产生新连接。
static POOL_CACHE: std::sync::Mutex<Option<(String, Pool)>> = std::sync::Mutex::new(None);

/// 浏览缓存有效期（与 turso 同策略）
const CACHE_TTL: Duration = Duration::from_secs(15);
/// 页缓存容量上限（超出整体清空，简化 LRU）
const PAGE_CACHE_MAX: usize = 128;
/// 行数缓存容量上限
const COUNT_CACHE_MAX: usize = 128;

type PageCacheKey = (String, String, usize); // (缓存键, "库.表", 偏移)
type CountCacheKey = (String, String); // (缓存键, "库.表")
type PageCacheMap = HashMap<PageCacheKey, (Instant, Vec<String>, Vec<Vec<String>>, usize)>;
type CountCacheMap = HashMap<CountCacheKey, (Instant, usize)>;

static PAGE_CACHE: std::sync::OnceLock<std::sync::Mutex<PageCacheMap>> = std::sync::OnceLock::new();
static COUNT_CACHE: std::sync::OnceLock<std::sync::Mutex<CountCacheMap>> =
    std::sync::OnceLock::new();

fn page_cache() -> &'static std::sync::Mutex<PageCacheMap> {
    PAGE_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn count_cache() -> &'static std::sync::Mutex<CountCacheMap> {
    COUNT_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 清空浏览缓存（断开/重连/刷新/写语句成功后调用）
pub fn invalidate() {
    if let Ok(mut c) = page_cache().lock() {
        c.clear();
    }
    if let Ok(mut c) = count_cache().lock() {
        c.clear();
    }
}

/// 断开连接：释放连接池并清空浏览缓存。
/// 池的驱动任务在全局 runtime 上，关闭也须在同一 runtime 执行（新建会挂死）。
pub fn disconnect() {
    let pool = POOL_CACHE
        .lock()
        .ok()
        .and_then(|mut c| c.take().map(|(_, p)| p));
    if let Some(pool) = pool {
        let _ = rt().block_on(pool.disconnect());
    }
    invalidate();
}

/// 标识符反引号转义（库名/表名/列名可能含特殊字符）
pub(crate) fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// 取（或新建）当前连接参数对应的连接池
fn pool_of(src: &MySqlSource) -> Result<Pool, String> {
    let key = src.key();
    if let Ok(cache) = POOL_CACHE.lock() {
        if let Some((p, pool)) = &*cache {
            if *p == key {
                return Ok(pool.clone());
            }
        }
    }
    let opts = OptsBuilder::default()
        .ip_or_hostname(src.host.trim().to_string())
        .tcp_port(src.port_num())
        .user(Some(src.user.trim().to_string()))
        .pass(Some(src.pass.clone()));
    let pool = Pool::new(opts);
    if let Ok(mut cache) = POOL_CACHE.lock() {
        *cache = Some((key, pool.clone()));
    }
    Ok(pool)
}

/// 全局共享 runtime：current_thread 足够（同一时刻只有一条查询在跑）。
/// 连接池的驱动任务绑定在首次使用的 runtime 上，跨线程新建 runtime 会
/// 取不到连接（ERR_GET_CONN），故必须全局复用（与 core::pg 同策略）。
static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn rt() -> &'static tokio::runtime::Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建 tokio runtime 失败")
    })
}

/// 后台线程中执行查询：在全局 runtime 上 `block_on` + 复用/新建连接池。
/// `f` 收到连接，产出与 UI 无关的纯数据。
fn run_query<T, F>(source: &MySqlSource, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(
            mysql_async::Conn,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send>>
        + Send
        + 'static,
{
    let src = source.clone();
    let handle = std::thread::spawn(move || {
        rt().block_on(async move {
            let conn = pool_of(&src)?
                .get_conn()
                .await
                .map_err(|e| lang::ERR_GET_CONN(e.to_string()))?;
            f(conn).await
        })
    });
    match handle.join() {
        Ok(r) => r,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                lang::ERR_THREAD_PANIC(*s)
            } else if let Some(s) = e.downcast_ref::<String>() {
                lang::ERR_THREAD_PANIC(s)
            } else {
                lang::ERR_THREAD_PANIC_UNKNOWN().to_string()
            };
            Err(msg)
        }
    }
}

/// SQL 值 → 文本（NULL/二进制安全降级；UI 展示用原始文本，不加 SQL 引号）
fn value_to_string(v: &mysql_async::Value) -> String {
    match v {
        mysql_async::Value::NULL => "NULL".to_string(),
        mysql_async::Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        // Date/Time 的 as_sql 恒带引号，须自行格式化为裸文本
        mysql_async::Value::Date(y, m, d, h, mi, s, us) => {
            if *us > 0 {
                format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")
            } else {
                format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
            }
        }
        mysql_async::Value::Time(neg, days, h, m, s, us) => {
            let sign = if *neg { "-" } else { "" };
            let h_total = *h as u32 + days * 24;
            if *us > 0 {
                format!("{sign}{h_total:02}:{m:02}:{s:02}.{us:06}")
            } else {
                format!("{sign}{h_total:02}:{m:02}:{s:02}")
            }
        }
        other => other.as_sql(false),
    }
}

/// 表节点：库 + 表（MySQL 一级即库，无 schema 概念）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub database: String,
    pub table: String,
}

/// MySQL 浏览器数据视图状态（业务侧快照，供 UI 层做表格渲染数据源）
#[derive(Clone)]
pub struct MySqlViewer {
    pub source: MySqlSource,
    /// 分级列表：库 → 该库下的表
    pub databases: Vec<(String, Vec<String>)>,
    pub selected: Option<TableRef>,
    pub table_data: Vec<Vec<String>>,
    pub column_names: Vec<String>,
    pub page_offset: usize,
    pub row_count: usize,
}

impl Default for MySqlViewer {
    fn default() -> Self {
        Self {
            source: MySqlSource {
                host: String::new(),
                port: String::from("3306"),
                user: String::new(),
                pass: String::new(),
            },
            databases: Vec::new(),
            selected: None,
            table_data: Vec::new(),
            column_names: Vec::new(),
            page_offset: 0,
            row_count: 0,
        }
    }
}

impl MySqlViewer {
    /// 连接数据库并加载库 → 表分级列表（不自动加载任何表数据）
    pub fn connect(&mut self) -> Result<(), String> {
        let source = self.source.clone();
        let dbs = run_query(&source, |mut conn| {
            Box::pin(async move {
                // 所有库（information_schema/mysql 等系统库也照列）
                let rows: Vec<String> = conn
                    .exec("SHOW DATABASES", ())
                    .await
                    .map_err(|e| lang::ERR_QUERY_TABLES(e.to_string()))?;
                let mut dbs = Vec::new();
                for db in rows {
                    // 每库拉表列表；个别库无权限时跳过该库（不阻塞整体连接）
                    let tables: Vec<String> = match conn
                        .exec(format!("SHOW TABLES FROM {}", quote_ident(&db)), ())
                        .await
                    {
                        Ok(rows) => rows,
                        Err(_) => continue,
                    };
                    if tables.is_empty() {
                        continue;
                    }
                    dbs.push((db, tables));
                }
                Ok::<_, String>(dbs)
            })
        })
        .map_err(|e| {
            crate::core::log::warn(
                "mysql",
                &format!("connect {}:{} failed: {}", source.host, source.port, e),
            );
            e
        })?;

        self.databases = dbs;
        self.selected = None;
        self.table_data = Vec::new();
        self.column_names = Vec::new();
        self.page_offset = 0;
        self.row_count = 0;
        Ok(())
    }

    /// 设置选定的表并加载指定页数据（页/行数带 TTL 缓存：命中零查询）
    pub fn set_selected_table(&mut self, table: TableRef, offset: usize) -> Result<(), String> {
        let key = format!("{}.{}", table.database, table.table);
        let qualified = format!(
            "{}.{}",
            quote_ident(&table.database),
            quote_ident(&table.table)
        );
        let src_key = self.source.key();

        // ── 页缓存命中：翻回已看过的页直接回填，零查询 ──
        if let Ok(cache) = page_cache().lock() {
            if let Some((at, cols, rows, row_count)) =
                cache.get(&(src_key.clone(), key.clone(), offset))
            {
                if at.elapsed() < CACHE_TTL {
                    let (cols, rows, row_count) = (cols.clone(), rows.clone(), *row_count);
                    self.selected = Some(table);
                    self.table_data = rows;
                    self.column_names = cols;
                    self.page_offset = offset;
                    self.row_count = row_count;
                    return Ok(());
                }
            }
        }

        // ── 表总行数缓存 ──
        let cached_count = match count_cache().lock() {
            Ok(c) => c.get(&(src_key.clone(), key.clone())).and_then(|(at, n)| {
                if at.elapsed() < CACHE_TTL {
                    Some(*n)
                } else {
                    None
                }
            }),
            Err(_) => None,
        };

        let (data, cols, row_count) = {
            let key_c = key.clone();
            let qualified_c = qualified.clone();
            let db_c = table.database.clone();
            let tbl_c = table.table.clone();
            let src_key_c = src_key.clone();
            run_query(&self.source, move |mut conn| {
                Box::pin(async move {
                    // 列名：information_schema 查列（按 ORDINAL_POSITION 排序）
                    let col_rows: Vec<String> = conn
                        .exec(
                            "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
                             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                             ORDER BY ORDINAL_POSITION",
                            (db_c.clone(), tbl_c.clone()),
                        )
                        .await
                        .map_err(|e| lang::ERR_TABLE_INFO(e.to_string()))?;

                    // 行数：命中缓存直接用；未命中才 COUNT 并回填
                    let row_count: usize = match cached_count {
                        Some(n) => n,
                        None => {
                            let n: usize = conn
                                .query_first(format!("SELECT COUNT(*) FROM {}", qualified_c))
                                .await
                                .unwrap_or(Some(0))
                                .unwrap_or(0);
                            if let Ok(mut cc) = count_cache().lock() {
                                cc.insert((src_key_c.clone(), key_c.clone()), (Instant::now(), n));
                                if cc.len() > COUNT_CACHE_MAX {
                                    cc.clear();
                                }
                            }
                            n
                        }
                    };

                    // 分页加载
                    let rows: Vec<mysql_async::Row> = conn
                        .exec(
                            format!(
                                "SELECT * FROM {} LIMIT {} OFFSET {}",
                                qualified_c, PAGE_SIZE, offset
                            ),
                            (),
                        )
                        .await
                        .map_err(|e| lang::ERR_QUERY_DATA(e.to_string()))?;
                    let data: Vec<Vec<String>> = rows
                        .iter()
                        .map(|r| (0..r.len()).map(|i| value_to_string(&r[i])).collect())
                        .collect();
                    Ok::<_, String>((data, col_rows, row_count))
                })
            })
        }?;

        // ── 回填页缓存 ──
        if let Ok(mut cache) = page_cache().lock() {
            cache.insert(
                (src_key, key, offset),
                (Instant::now(), cols.clone(), data.clone(), row_count),
            );
            if cache.len() > PAGE_CACHE_MAX {
                cache.clear();
            }
        }

        self.selected = Some(table);
        self.table_data = data;
        self.column_names = cols;
        self.page_offset = offset;
        self.row_count = row_count;
        Ok(())
    }

    /// 执行任意 SQL：有结果集返回列行（状态空），写语句返回受影响行数提示
    pub fn execute_sql(&mut self, sql: &str) -> Result<String, String> {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return Err(lang::ERR_EMPTY_SQL().to_string());
        }

        let (cols, data, affected): (Vec<String>, Vec<Vec<String>>, usize) =
            run_query(&self.source, move |mut conn| {
                Box::pin(async move {
                    let mut result = conn
                        .query_iter(&sql)
                        .await
                        .map_err(|e| lang::ERR_QUERY_FAIL(e.to_string()))?;
                    let ncols = result.columns_ref().len();
                    if ncols > 0 {
                        // 有结果集：按列名收集（行数封顶 MAX_TABLE_ROWS）
                        let mut data: Vec<Vec<String>> = Vec::new();
                        let mut cols: Vec<String> = Vec::new();
                        while let Some(row) = result
                            .next()
                            .await
                            .map_err(|e| lang::ERR_QUERY_FAIL(e.to_string()))?
                        {
                            if cols.is_empty() {
                                cols = (0..row.len())
                                    .map(|i| row.columns_ref()[i].name_str().to_string())
                                    .collect();
                            }
                            if data.len() >= MAX_TABLE_ROWS {
                                break;
                            }
                            data.push((0..row.len()).map(|i| value_to_string(&row[i])).collect());
                        }
                        Ok((cols, data, 0usize))
                    } else {
                        let n = result.affected_rows();
                        Ok((Vec::new(), Vec::new(), n as usize))
                    }
                })
            })?;

        let is_write = affected > 0 || cols.is_empty();
        if is_write {
            invalidate(); // 写语句成功：数据已变，缓存失效
        }
        self.table_data = data;
        self.column_names = cols;
        self.selected = None;
        self.page_offset = 0;
        self.row_count = self.table_data.len();
        let status = if affected > 0 {
            lang::SQL_AFFECTED(affected)
        } else {
            String::new()
        };
        Ok(status)
    }
    /// 导出专用：读取列名与实时总行数（不走缓存——导出必须拿到准确值）
    pub fn export_begin(&mut self, table: &TableRef) -> Result<(Vec<String>, usize), String> {
        let qualified = format!(
            "{}.{}",
            quote_ident(&table.database),
            quote_ident(&table.table)
        );
        let db = table.database.clone();
        let tbl = table.table.clone();
        let (cols, count) = run_query(&self.source, move |mut conn| {
            Box::pin(async move {
                let col_rows: Vec<String> = conn
                    .exec(
                        "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
                         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                         ORDER BY ORDINAL_POSITION",
                        (db, tbl),
                    )
                    .await
                    .map_err(|e| lang::ERR_TABLE_INFO(e.to_string()))?;
                let count: usize = conn
                    .query_first(format!("SELECT COUNT(*) FROM {}", qualified))
                    .await
                    .unwrap_or(Some(0))
                    .unwrap_or(0);
                Ok::<_, String>((col_rows, count))
            })
        })?;
        self.column_names = cols.clone();
        self.row_count = count;
        Ok((cols, count))
    }

    /// 导出专用：大批量取行（不经页缓存）
    pub fn export_batch(
        &mut self,
        table: &TableRef,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Vec<String>>, String> {
        let qualified = format!(
            "{}.{}",
            quote_ident(&table.database),
            quote_ident(&table.table)
        );
        run_query(&self.source, move |mut conn| {
            Box::pin(async move {
                let rows: Vec<mysql_async::Row> = conn
                    .exec(
                        format!(
                            "SELECT * FROM {} LIMIT {} OFFSET {}",
                            qualified, limit, offset
                        ),
                        (),
                    )
                    .await
                    .map_err(|e| lang::ERR_QUERY_DATA(e.to_string()))?;
                Ok(rows
                    .iter()
                    .map(|r| (0..r.len()).map(|i| value_to_string(&r[i])).collect())
                    .collect())
            })
        })
    }

    /// 读取建表 DDL：单表 = SHOW CREATE TABLE 原文；整库 = 各表 DDL + 视图定义
    pub fn export_ddl(
        &mut self,
        database: &str,
        table: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let db = database.to_string();
        let tbl = table.map(|t| t.to_string());
        run_query(&self.source, move |mut conn| {
            Box::pin(async move {
                let targets: Vec<String> = match &tbl {
                    Some(t) => vec![t.clone()],
                    None => conn
                        .exec(
                            "SELECT TABLE_NAME FROM information_schema.TABLES \
                             WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE' \
                             ORDER BY TABLE_NAME",
                            (&db,),
                        )
                        .await
                        .map_err(|e| lang::ERR_TABLE_INFO(e.to_string()))?,
                };
                let mut ddl = Vec::new();
                for t in &targets {
                    // SHOW CREATE TABLE 返回两列（表名 + DDL）；无权限的表跳过不阻塞
                    if let Ok(Some(row)) = conn
                        .exec_first::<mysql_async::Row, _, _>(
                            format!("SHOW CREATE TABLE {}.{}", quote_ident(&db), quote_ident(t)),
                            (),
                        )
                        .await
                    {
                        if let Some(Some(sql)) = row.get::<Option<String>, usize>(1) {
                            ddl.push(format!("{};\n", sql.trim_end_matches(';')));
                        }
                    }
                }
                // 视图定义（整库导出时附带）
                if tbl.is_none() {
                    if let Ok(vrows) = conn
                        .exec::<mysql_async::Row, _, _>(
                            "SELECT TABLE_NAME, VIEW_DEFINITION FROM information_schema.VIEWS \
                             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
                            (&db,),
                        )
                        .await
                    {
                        for r in vrows {
                            let name: String = r.get(0).unwrap_or_default();
                            let def: String = r.get(1).unwrap_or_default();
                            if !def.is_empty() {
                                ddl.push(format!(
                                    "CREATE VIEW {} AS {};\n",
                                    quote_ident(&name),
                                    def
                                ));
                            }
                        }
                    }
                }
                Ok::<_, String>(ddl)
            })
        })
    }

    /// 检测表的单列主键（复合主键/无主键返回 None，导出时回退 OFFSET）
    pub fn export_pk(&mut self, table: &TableRef) -> Result<Option<String>, String> {
        let db = table.database.clone();
        let tbl = table.table.clone();
        run_query(&self.source, move |mut conn| {
            Box::pin(async move {
                let rows: Vec<String> = conn
                    .exec(
                        "SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE \
                         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                           AND CONSTRAINT_NAME = 'PRIMARY' \
                         ORDER BY ORDINAL_POSITION",
                        (db, tbl),
                    )
                    .await
                    .map_err(|e| lang::ERR_TABLE_INFO(e.to_string()))?;
                Ok::<_, String>(if rows.len() == 1 {
                    Some(rows[0].clone())
                } else {
                    None
                })
            })
        })
    }

    /// 主键游标分批取行：WHERE pk > last ORDER BY pk（O(n)，不受中途增删行影响）。
    /// 返回空批 = 已到表尾。仅适用单列主键（export_pk 返回 None 时上层回退 OFFSET）。
    pub fn export_batch_keyset(
        &mut self,
        table: &TableRef,
        pk_col: &str,
        last_pk: Option<String>,
        limit: usize,
    ) -> Result<Vec<Vec<String>>, String> {
        let qualified = format!(
            "{}.{}",
            quote_ident(&table.database),
            quote_ident(&table.table)
        );
        let pk_col = pk_col.to_string();
        run_query(&self.source, move |mut conn| {
            Box::pin(async move {
                // 首批无游标：省略 WHERE（pk > NULL 恒为假，不能拼进去）
                let sql = match &last_pk {
                    None => format!(
                        "SELECT * FROM {} ORDER BY {} LIMIT {}",
                        qualified,
                        quote_ident(&pk_col),
                        limit
                    ),
                    Some(_) => format!(
                        "SELECT * FROM {} WHERE {} > ? ORDER BY {} LIMIT {}",
                        qualified,
                        quote_ident(&pk_col),
                        quote_ident(&pk_col),
                        limit
                    ),
                };
                let rows: Vec<mysql_async::Row> = match &last_pk {
                    None => conn.exec(sql, ()).await,
                    Some(s) => {
                        // 数值主键按整数传参（避免字符串比较破坏数值序）；其余按字符串
                        let val = match s.parse::<i64>() {
                            Ok(n) => mysql_async::Value::Int(n),
                            Err(_) => mysql_async::Value::Bytes(s.clone().into_bytes()),
                        };
                        conn.exec(sql, (val,)).await
                    }
                }
                .map_err(|e| lang::ERR_QUERY_DATA(e.to_string()))?;
                Ok(rows
                    .iter()
                    .map(|r| (0..r.len()).map(|i| value_to_string(&r[i])).collect())
                    .collect())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_escapes_backticks() {
        assert_eq!(quote_ident("user"), "`user`");
        assert_eq!(quote_ident("we`ird"), "`we``ird`");
        assert_eq!(quote_ident(""), "``");
    }

    #[test]
    fn source_key_omits_password() {
        let s = MySqlSource {
            host: "db.local".into(),
            port: "3307".into(),
            user: "root".into(),
            pass: "p@ss w`,rd".into(), // 特殊字符不进缓存键
        };
        assert_eq!(s.key(), "mysql://root@db.local:3307");
    }

    #[test]
    fn source_port_defaults_3306() {
        let s = MySqlSource {
            port: "".into(),
            ..Default::default()
        };
        assert_eq!(s.port_num(), 3306);
        let s = MySqlSource {
            port: "abc".into(),
            ..Default::default()
        };
        assert_eq!(s.port_num(), 3306);
        let s = MySqlSource {
            port: " 3307 ".into(),
            ..Default::default()
        };
        assert_eq!(s.port_num(), 3307);
    }

    #[test]
    fn value_to_string_null_and_bytes() {
        assert_eq!(value_to_string(&mysql_async::Value::NULL), "NULL");
        assert_eq!(
            value_to_string(&mysql_async::Value::Bytes(b"h\xc3\xa9llo".to_vec())),
            "héllo"
        );
    }

    #[test]
    fn page_cache_roundtrip() {
        // 静态缓存读写（键含偏移；写入后覆盖、TTL 内命中）
        let key: PageCacheKey = ("k".into(), "t".into(), 0);
        page_cache().lock().unwrap().insert(
            key.clone(),
            (Instant::now(), vec!["c".into()], vec![vec!["v".into()]], 1),
        );
        let hit = page_cache().lock().unwrap().get(&key).cloned();
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().1, vec!["c"]);
        page_cache().lock().unwrap().clear();
        assert!(page_cache().lock().unwrap().get(&key).is_none());
    }

    #[test]
    fn count_cache_roundtrip() {
        let key: CountCacheKey = ("k".into(), "t".into());
        count_cache()
            .lock()
            .unwrap()
            .insert(key.clone(), (Instant::now(), 42));
        assert_eq!(count_cache().lock().unwrap().get(&key).unwrap().1, 42);
        count_cache().lock().unwrap().clear();
    }
}
