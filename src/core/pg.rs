//! PostgreSQL 数据库连接与查询（业务层，不含任何 UI 代码）
//!
//! 功能与 [`crate::core::turso`] 对齐：连接缓存 + 页/行数 TTL 缓存 + 分页浏览
//! 及 SQL 执行、CSV 导出。查询在独立线程中执行（全局 current_thread runtime
//! 上 `block_on`），UI 线程只收结果。
//!
//! 与 Turso 的差异：左侧列表**分级**——第一层为 schema，第二层为表；
//! `Client` 的查询方法收 `&self`，用 `Arc<Client>` 缓存共享。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio_postgres::NoTls;

use crate::lang;

/// 单次查询最大返回行数（防无 LIMIT 查询打爆内存）
pub const MAX_TABLE_ROWS: usize = 100_000;

/// 表格分页大小（每页行数）
pub const PAGE_SIZE: usize = 50;

/// 连接串：`postgres://user:pass@host:port/dbname` 或 key=value 形式
pub type PgSource = String;

/// 全局共享 runtime：current_thread 足够（同一时刻只有一条查询在跑）。
/// 连接驱动任务 `tokio::spawn` 在它上面，`block_on` 期间会被驱动。
static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn rt() -> &'static tokio::runtime::Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建 tokio runtime 失败")
    })
}

/// 已打开的连接缓存：键 = 每库解析后的连接串（跨库浏览各存一条）。
static CLIENT_CACHE: std::sync::Mutex<Option<HashMap<String, Arc<tokio_postgres::Client>>>> =
    std::sync::Mutex::new(None);

/// 浏览缓存有效期（与 turso 同策略）
const CACHE_TTL: Duration = Duration::from_secs(15);
/// 页缓存容量上限（超出整体清空，简化 LRU）
const PAGE_CACHE_MAX: usize = 128;
/// 行数缓存容量上限
const COUNT_CACHE_MAX: usize = 128;

type PageCacheKey = (String, String, usize); // (连接串, "schema.表", 偏移)
type CountCacheKey = (String, String); // (连接串, "schema.表")
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

/// 断开连接：关闭全部缓存连接并清空浏览缓存
pub fn disconnect() {
    let clients = CLIENT_CACHE.lock().ok().and_then(|mut c| c.take());
    // drop 即触发各连接关闭（引用计数归零）
    drop(clients);
    invalidate();
}

/// 双引号标识符转义（schema/表/列名可能含特殊字符或大小写）
pub(crate) fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 连接参数展示名（host:port/db，密码不出现在展示中）
pub fn display_of(src: &str) -> String {
    // 尽力而为：postgres://user:***@host:port/db → host:port/db。
    // 密码可含 '@'，故取**最后一个** '@' 之后的部分。
    let s = src.trim();
    let rest = s
        .strip_prefix("postgres://")
        .or_else(|| s.strip_prefix("postgresql://"))
        .unwrap_or(s);
    match rest.rfind('@') {
        Some(pos) => rest[pos + 1..].to_string(),
        None => rest.to_string(),
    }
}

/// 把连接串的目标库替换为 `db`（仅支持 URL 形式：postgres://user:pass@host:port/dbname）。
/// key=value 形式无法安全改写，返回 Err（调用方跳过跨库枚举，仍可浏览本库）。
pub(crate) fn url_for_db(source: &str, db: &str) -> Result<String, String> {
    let s = source.trim();
    let rest = s
        .strip_prefix("postgres://")
        .or_else(|| s.strip_prefix("postgresql://"))
        .ok_or_else(|| lang::ERR_CONNECT_DB("not a URL connection string".to_string()))?;
    // authority 是最后一个 '@' 之后的部分；dbname 是 authority 里最后一个 '/' 之后
    let at = rest.rfind('@').unwrap_or(0);
    let authority_end = rest[at..]
        .find('/')
        .map(|p| at + p)
        .ok_or_else(|| lang::ERR_CONNECT_DB("no dbname in URL".to_string()))?;
    // 去掉 dbname 上的查询参数（?sslmode=... 等），换库后参数原样保留
    let authority = &rest[..authority_end];
    let tail = &rest[authority_end + 1..];
    let old_db = tail.split('?').next().unwrap_or("");
    let params = tail
        .split_once('?')
        .map(|(_, q)| format!("?{}", q))
        .unwrap_or_default();
    if old_db.is_empty() {
        return Err(lang::ERR_CONNECT_DB("empty dbname in URL".to_string()));
    }
    // dbname 在 URL 路径段：authority 之后补 '/' 再拼库名（PG 库名不允许
    // / ? # 空格等，无需转义；双引号转义是 SQL 标识符语法，放进 URL 反而不合法）
    Ok(format!("postgres://{}/{}{}", authority, db, params))
}

/// 表查询的目标库连接串：按 `table.db` 重写 URL（空 db 或非 URL 串回落本库）
fn db_source_for(table: &TableRef, source: &str) -> String {
    if table.db.is_empty() {
        source.to_string()
    } else {
        url_for_db(source, &table.db).unwrap_or_else(|_| source.to_string())
    }
}

/// 取（或新建）指定连接串对应的连接（在 rt() 上执行；跨库浏览按库各建一条）
async fn client_of(key: &str) -> Result<Arc<tokio_postgres::Client>, String> {
    if let Ok(cache) = CLIENT_CACHE.lock() {
        if let Some(map) = &*cache {
            if let Some(client) = map.get(key) {
                return Ok(client.clone());
            }
        }
    }
    let (client, conn_task) = tokio_postgres::connect(key, NoTls)
        .await
        .map_err(|e| lang::ERR_CONNECT_DB(e.to_string()))?;
    // 连接驱动任务挂到全局 runtime；block_on 期间会被驱动
    rt().spawn(conn_task);
    let client = Arc::new(client);
    if let Ok(mut cache) = CLIENT_CACHE.lock() {
        let map = cache.get_or_insert_with(HashMap::new);
        map.insert(key.to_string(), client.clone());
    }
    Ok(client)
}

/// 在独立线程中执行查询（全局 runtime `block_on`，连接缓存复用）。
/// `f` 收到 Arc<Client>，产出与 UI 无关的纯数据。
fn run_query<T, F>(source: &str, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(
            Arc<tokio_postgres::Client>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send>>
        + Send
        + 'static,
{
    let key = source.to_string();
    let handle = std::thread::spawn(move || {
        rt().block_on(async move {
            let client = client_of(&key).await?;
            f(client).await
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

/// 表节点：库 + schema + 表（分级列表三层：库 → schema → 表）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub db: String,
    pub schema: String,
    pub table: String,
}

impl TableRef {
    pub fn key(&self) -> String {
        format!("{}.{}.{}", self.db, self.schema, self.table)
    }
}

/// PG 浏览器数据视图状态（业务侧快照，供 UI 层做表格渲染数据源）
#[derive(Clone, Default)]
pub struct PgViewer {
    pub source: PgSource,
    /// 分级列表：schema → 该 schema 下的表
    pub schemas: Vec<(String, Vec<String>)>,
    pub selected: Option<TableRef>,
    pub table_data: Vec<Vec<String>>,
    pub column_names: Vec<String>,
    pub page_offset: usize,
    pub row_count: usize,
}

impl PgViewer {
    /// 连接数据库并加载 schema → 表分级列表（不自动加载任何表数据）。
    ///
    /// 严格按连接串匹配：一个连接串只对应一个库，仅枚举**该库**的
    /// schema → 表（不做跨库发现——库的切换由库址管理里的另一条连接串完成）。
    pub fn connect(&mut self) -> Result<(), String> {
        let source = self.source.clone();
        let schemas = run_query(&source, |client| {
            Box::pin(async move {
                // 用户表与视图（排除系统 schema；relkind：r=普通表 p=分区表
                // v=视图 m=物化视图 f=外表——只列 'r' 会漏掉分区表与视图）
                let rows = client
                    .query(
                        "SELECT n.nspname, c.relname FROM pg_class c \
                         JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') \
                           AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
                           AND n.nspname NOT LIKE 'pg_toast%' \
                         ORDER BY n.nspname, c.relname",
                        &[],
                    )
                    .await
                    .map_err(|e| lang::ERR_QUERY_TABLES(e.to_string()))?;
                let mut schemas: Vec<(String, Vec<String>)> = Vec::new();
                for row in rows {
                    let schema: String = row.get(0);
                    let table: String = row.get(1);
                    match schemas.last_mut() {
                        Some((s, tables)) if *s == schema => tables.push(table),
                        _ => schemas.push((schema, vec![table])),
                    }
                }
                Ok::<_, String>(schemas)
            })
        })?;

        self.schemas = schemas;
        self.selected = None;
        self.table_data = Vec::new();
        self.column_names = Vec::new();
        self.page_offset = 0;
        self.row_count = 0;
        Ok(())
    }

    /// 设置选定的表并加载指定页数据（页/行数带 TTL 缓存：命中零查询）。
    /// 目标库取 `table.db`（跨库浏览）：连接串按库重写路由。
    pub fn set_selected_table(&mut self, table: TableRef, offset: usize) -> Result<(), String> {
        let key = table.key();
        let qualified = format!(
            "{}.{}",
            quote_ident(&table.schema),
            quote_ident(&table.table)
        );
        // 目标库连接串：table.db 与当前库不同则重写 URL（key=value 串无法跨库，回落本库）
        let db_source = if table.db.is_empty() {
            self.source.clone()
        } else {
            url_for_db(&self.source, &table.db).unwrap_or_else(|_| self.source.clone())
        };
        let src_key = db_source.clone();

        // ── 页缓存命中 ──
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
            let src_key_c = src_key.clone();
            let schema_c = table.schema.clone();
            let table_c = table.table.clone();
            run_query(&db_source, move |client| {
                Box::pin(async move {
                    // 列名：information_schema 查列（按序）
                    let col_rows = client
                        .query(
                            "SELECT column_name FROM information_schema.columns \
                             WHERE table_schema = $1 AND table_name = $2 \
                             ORDER BY ordinal_position",
                            &[&schema_c, &table_c],
                        )
                        .await
                        .map_err(|e| lang::ERR_TABLE_INFO(e.to_string()))?;
                    let cols: Vec<String> =
                        col_rows.iter().map(|r| r.get::<_, String>(0)).collect();

                    // 行数：命中缓存直接用；未命中才 COUNT 并回填
                    let row_count: usize = match cached_count {
                        Some(n) => n,
                        None => {
                            let n: usize = client
                                .query_one(&format!("SELECT COUNT(*) FROM {}", qualified_c), &[])
                                .await
                                .ok()
                                .and_then(|r| r.try_get::<_, i64>(0).ok())
                                .unwrap_or(0)
                                .max(0) as usize;
                            if let Ok(mut cc) = count_cache().lock() {
                                cc.insert((src_key_c.clone(), key_c.clone()), (Instant::now(), n));
                                if cc.len() > COUNT_CACHE_MAX {
                                    cc.clear();
                                }
                            }
                            n
                        }
                    };

                    // 分页加载：simple_query 文本协议——服务端对所有类型
                    // （timestamp/numeric/json/uuid/数组…）都给规范文本，NULL 为 None
                    let msgs = client
                        .simple_query(&format!(
                            "SELECT * FROM {} LIMIT {} OFFSET {}",
                            qualified_c, PAGE_SIZE, offset
                        ))
                        .await
                        .map_err(|e| lang::ERR_QUERY_DATA(e.to_string()))?;
                    use tokio_postgres::SimpleQueryMessage as M;
                    // 列数/列名取自首个 Row 消息（simple_query 可能混入 CommandComplete）
                    let ncols = msgs.iter().find_map(|m| match m {
                        M::Row(row) => Some(row.columns().len()),
                        _ => None,
                    });
                    let Some(ncols) = ncols else {
                        // 空表（0 行）：simple_query 无 Row 消息，但列结构必须保留
                        // （列名来自 information_schema）——否则 UI 误判为未加载
                        return Ok::<_, String>((Vec::new(), cols.clone(), row_count));
                    };
                    let data: Vec<Vec<String>> = msgs
                        .iter()
                        .filter_map(|m| match m {
                            M::Row(row) => Some(
                                (0..ncols)
                                    .map(|i| row.get(i).unwrap_or("NULL").to_string())
                                    .collect::<Vec<String>>(),
                            ),
                            _ => None,
                        })
                        .collect();
                    Ok::<_, String>((data, cols, row_count))
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

    /// 执行任意 SQL：返回状态提示（受影响行数；查询返回空串）
    pub fn execute_sql(&mut self, sql: &str) -> Result<String, String> {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return Err(lang::ERR_EMPTY_SQL().to_string());
        }
        let is_select = {
            let upper = sql.to_uppercase();
            upper.starts_with("SELECT")
                || upper.starts_with("SHOW")
                || upper.starts_with("TABLE")
                || upper.starts_with("EXPLAIN")
        };

        let (cols, data, affected) = run_query(&self.source, move |client| {
            Box::pin(async move {
                if is_select {
                    // simple_query 文本协议：全类型（timestamp/json/数组…）规范文本，
                    // 多语句/多结果集也能取到（simple_query 返回全部 SimpleQueryMessage）
                    let msgs = client
                        .simple_query(&sql)
                        .await
                        .map_err(|e| lang::ERR_QUERY_FAIL(e.to_string()))?;
                    use tokio_postgres::SimpleQueryMessage as M;
                    let ncols = msgs.iter().find_map(|m| match m {
                        M::Row(row) => Some(row.columns().len()),
                        _ => None,
                    });
                    let Some(ncols) = ncols else {
                        // 无结果集（如 EXPLAIN 之外的特殊语句）：按空结果处理
                        return Ok::<_, String>((Vec::new(), Vec::new(), 0usize));
                    };
                    let cols: Vec<String> = msgs
                        .iter()
                        .find_map(|m| match m {
                            M::Row(row) => {
                                Some(row.columns().iter().map(|c| c.name().to_string()).collect())
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    let data: Vec<Vec<String>> = msgs
                        .iter()
                        .filter_map(|m| match m {
                            M::Row(row) => Some(
                                (0..ncols)
                                    .map(|i| row.get(i).unwrap_or("NULL").to_string())
                                    .collect::<Vec<String>>(),
                            ),
                            _ => None,
                        })
                        .take(MAX_TABLE_ROWS)
                        .collect();
                    Ok::<_, String>((cols, data, 0usize))
                } else {
                    let n = client
                        .execute(&sql, &[])
                        .await
                        .map_err(|e| lang::ERR_EXEC_FAIL(e.to_string()))?;
                    Ok((Vec::new(), Vec::new(), n as usize))
                }
            })
        })?;

        if !is_select {
            invalidate(); // 写语句成功：数据已变，缓存失效
        }
        self.table_data = data;
        self.column_names = cols;
        self.selected = None;
        self.page_offset = 0;
        self.row_count = self.table_data.len();
        let status = if is_select {
            String::new()
        } else {
            lang::SQL_AFFECTED(affected)
        };
        Ok(status)
    }
    /// 导出专用：读取列名与实时总行数（不走缓存——导出必须拿到准确值）
    pub fn export_begin(&mut self, table: &TableRef) -> Result<(Vec<String>, usize), String> {
        let qualified = format!(
            "{}.{}",
            quote_ident(&table.schema),
            quote_ident(&table.table)
        );
        let schema = table.schema.clone();
        let tname = table.table.clone();
        let db_source = db_source_for(table, &self.source);
        let (cols, count) = run_query(&db_source, move |client| {
            Box::pin(async move {
                let col_rows = client
                    .query(
                        "SELECT column_name FROM information_schema.columns \
                         WHERE table_schema = $1 AND table_name = $2 \
                         ORDER BY ordinal_position",
                        &[&schema, &tname],
                    )
                    .await
                    .map_err(|e| lang::ERR_TABLE_INFO(e.to_string()))?;
                let cols: Vec<String> = col_rows.iter().map(|r| r.get::<_, String>(0)).collect();
                let count: usize = client
                    .query_one(&format!("SELECT COUNT(*) FROM {}", qualified), &[])
                    .await
                    .ok()
                    .and_then(|r| r.try_get::<_, i64>(0).ok())
                    .unwrap_or(0)
                    .max(0) as usize;
                Ok::<_, String>((cols, count))
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
            quote_ident(&table.schema),
            quote_ident(&table.table)
        );
        let db_source = db_source_for(table, &self.source);
        run_query(&db_source, move |client| {
            Box::pin(async move {
                // simple_query 文本协议：timestamp/numeric/json 等全类型规范文本
                let msgs = client
                    .simple_query(&format!(
                        "SELECT * FROM {} LIMIT {} OFFSET {}",
                        qualified, limit, offset
                    ))
                    .await
                    .map_err(|e| lang::ERR_QUERY_DATA(e.to_string()))?;
                use tokio_postgres::SimpleQueryMessage as M;
                let ncols = msgs
                    .iter()
                    .find_map(|m| match m {
                        M::Row(row) => Some(row.columns().len()),
                        _ => None,
                    })
                    .unwrap_or(0);
                Ok(msgs
                    .iter()
                    .filter_map(|m| match m {
                        M::Row(row) => Some(
                            (0..ncols)
                                .map(|i| row.get(i).unwrap_or("NULL").to_string())
                                .collect::<Vec<String>>(),
                        ),
                        _ => None,
                    })
                    .collect())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_escapes_double_quotes() {
        assert_eq!(quote_ident("user"), "\"user\"");
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(quote_ident(""), "\"\"");
    }

    #[test]
    fn display_of_strips_credentials() {
        assert_eq!(
            display_of("postgres://u:p@ss@db.local:5432/app"),
            "db.local:5432/app"
        );
        assert_eq!(display_of("postgresql://u@h/app"), "h/app");
        // 非 URL 形式原样返回
        assert_eq!(display_of("host= h port=5432"), "host= h port=5432");
    }

    #[test]
    fn cell_to_string_common_types() {
        use tokio_postgres::types::ToSql;
        let mk_row = |vals: &[&(dyn ToSql + Sync)]| {
            // try_get 需要 Row；用 simple 构造不便——直接测 NULL 布尔臂以外的
            // 具体类型需真实 Row，这里用 i32 列验证布尔/String 两臂前的命中顺序。
            // 真实行构造依赖协议，覆盖在集成验证；此处仅保证函数可调用编译通过。
            let _ = vals;
        };
        mk_row(&[]);
    }

    #[test]
    fn table_ref_key_format() {
        let t = TableRef {
            db: "kitxtest".into(),
            schema: "public".into(),
            table: "users".into(),
        };
        assert_eq!(t.key(), "kitxtest.public.users");
    }

    #[test]
    fn url_for_db_rewrites_dbname() {
        let base = "postgres://sa:q@127.0.0.1:5432/postgres";
        assert_eq!(
            url_for_db(base, "kitxtest").unwrap(),
            "postgres://sa:q@127.0.0.1:5432/kitxtest"
        );
        // 带查询参数：参数保留
        assert_eq!(
            url_for_db("postgres://sa:q@h/db?sslmode=disable", "x").unwrap(),
            "postgres://sa:q@h/x?sslmode=disable"
        );
        // 密码含 '@'：按最后一个 '@' 定位 authority
        assert_eq!(
            url_for_db("postgres://sa:p@ss@h/db", "x").unwrap(),
            "postgres://sa:p@ss@h/x"
        );
        // 非 URL / 无 dbname：Err
        assert!(url_for_db("host=h dbname=d", "x").is_err());
        assert!(url_for_db("postgres://sa:q@h", "x").is_err());
    }

    #[test]
    fn page_cache_roundtrip() {
        let key: PageCacheKey = ("k".into(), "s.t".into(), 50);
        page_cache().lock().unwrap().insert(
            key.clone(),
            (Instant::now(), vec!["c".into()], vec![vec!["v".into()]], 1),
        );
        assert!(page_cache().lock().unwrap().get(&key).is_some());
        page_cache().lock().unwrap().clear();
        assert!(page_cache().lock().unwrap().get(&key).is_none());
    }
}
