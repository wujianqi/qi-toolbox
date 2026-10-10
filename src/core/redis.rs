//! Redis/Valkey 数据浏览（业务层，不含任何 UI 代码）
//!
//! 与 [`crate::core::pg`] 对齐的骨架：连接缓存 + 键 TTL 缓存 + SCAN 分页。
//! Redis 无表结构,左侧列表为**数据库编号(db0..dbN)**,键按 `SCAN` 游标分页
//! 浏览;值按类型读取(STRING/LIST/HASH/SET/ZSET/STREAM)。
//! 命令在独立线程的全局 current_thread runtime 上 `block_on`,UI 线程只收结果。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use redis::AsyncCommands;

use crate::lang;

/// 每页键数量（SCAN COUNT 建议 + 返回截断上限）
pub const PAGE_SIZE: usize = 100;
/// 单次读取值的最大字节（超过截断展示,防止大 value 打爆 UI）
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

/// 连接串：`redis://[:password@]host:port[/db]`
#[allow(dead_code)] // 供 UI/后续模块引用（lib 单测目标下暂无调用点）
pub type RedisSource = String;

/// 全局共享 runtime（与 pg 同策略：同一时刻只有一条命令在跑）
static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn rt() -> &'static tokio::runtime::Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建 tokio runtime 失败")
    })
}

/// 已打开的多路复用连接缓存：键 = 连接串（含 db 编号）。
/// `MultiplexedConnection` 可 Clone,命令在其上串行排队。
static CONN_CACHE: std::sync::Mutex<Option<HashMap<String, redis::aio::MultiplexedConnection>>> =
    std::sync::Mutex::new(None);

/// 键列表页缓存（SCAN 结果,连接串+db+游标偏移为键）
type ScanCacheKey = (String, usize); // (连接串, 页序号)
type ScanCacheMap = HashMap<ScanCacheKey, (Instant, Vec<String>, bool)>; // (时间, 键名, 是否还有更多)

static SCAN_CACHE: std::sync::OnceLock<std::sync::Mutex<ScanCacheMap>> = std::sync::OnceLock::new();

fn scan_cache() -> &'static std::sync::Mutex<ScanCacheMap> {
    SCAN_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 浏览缓存有效期（与 pg/turso 同策略）
const CACHE_TTL: Duration = Duration::from_secs(15);
/// 页缓存容量上限
const SCAN_CACHE_MAX: usize = 128;

/// 清空浏览缓存（断开/刷新/写命令成功后调用）
pub fn invalidate() {
    if let Ok(mut c) = scan_cache().lock() {
        c.clear();
    }
}

/// 断开连接：关闭全部缓存连接并清空浏览缓存
pub fn disconnect() {
    let conns = CONN_CACHE.lock().ok().and_then(|mut c| c.take());
    drop(conns);
    invalidate();
}

/// 连接参数展示名（host:port/db,密码不出现在展示中）
pub fn display_of(src: &str) -> String {
    let s = src.trim();
    let rest = s.strip_prefix("redis://").unwrap_or(s);
    match rest.rfind('@') {
        Some(pos) => rest[pos + 1..].to_string(),
        None => rest.to_string(),
    }
}

/// 把连接串的目标 db 替换为 `db`（redis://host:port/0 → /db）。
pub fn url_for_db(source: &str, db: i64) -> String {
    let s = source.trim();
    let (prefix, rest) = match s.strip_prefix("redis://") {
        Some(r) => ("redis://", r),
        None => ("", s),
    };
    // authority 是最后一个 '@' 之后的部分；db 是 authority 里第一个 '/' 之后；
    // db 段上的查询参数（?foo=1 等）原样保留
    let at = rest.rfind('@').unwrap_or(0);
    let Some(slash) = rest[at..].find('/') else {
        return format!("{}{}{}/{}", prefix, &rest[..at], &rest[at..], db);
    };
    let authority_end = at + slash;
    let authority = &rest[..authority_end];
    let tail = &rest[authority_end + 1..]; // "0?foo=1" / "0"
    let query = tail.find('?').map(|q| &tail[q..]).unwrap_or("");
    format!("{}{}/{}{}", prefix, authority, db, query)
}

/// 建立连接并 PING 验证（不复用缓存,连接校验用）
fn ping(source: &str) -> Result<(), String> {
    let client = redis::Client::open(source).map_err(|e| lang::ERR_CONNECT_DB(e.to_string()))?;
    let mut conn = rt()
        .block_on(client.get_multiplexed_async_connection())
        .map_err(|e| lang::ERR_CONNECT_DB(e.to_string()))?;
    let pong: String = rt()
        .block_on(redis::cmd("PING").query_async(&mut conn))
        .map_err(|e| lang::ERR_CONNECT_DB(e.to_string()))?;
    if pong != "PONG" {
        return Err(lang::ERR_CONNECT_DB(format!("PING → {}", pong)));
    }
    Ok(())
}

/// 测试连接（「连接」按钮）：成功后写入连接缓存
pub fn connect(source: &str) -> Result<(), String> {
    ping(source)?;
    // 预热缓存连接
    get_conn(source)?;
    Ok(())
}

/// 取（或建）缓存的多路复用连接
fn get_conn(source: &str) -> Result<redis::aio::MultiplexedConnection, String> {
    if let Some(c) = CONN_CACHE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(source).cloned()))
    {
        return Ok(c);
    }
    let client = redis::Client::open(source).map_err(|e| lang::ERR_CONNECT_DB(e.to_string()))?;
    let conn = rt()
        .block_on(client.get_multiplexed_async_connection())
        .map_err(|e| lang::ERR_CONNECT_DB(e.to_string()))?;
    if let Ok(mut g) = CONN_CACHE.lock() {
        g.get_or_insert_with(HashMap::new)
            .insert(source.to_string(), conn.clone());
    }
    Ok(conn)
}

/// 枚举可用数据库编号（CONFIG GET databases / 默认 16,逐库 PING 太重,直接返回 0..N）
pub fn list_databases(source: &str) -> Result<Vec<String>, String> {
    let conn = get_conn(source)?;
    let n: usize = rt()
        .block_on(async {
            let mut conn = conn.clone();
            let _: () = redis::cmd("SELECT").arg(0).query_async(&mut conn).await?;
            let res: HashMap<String, String> = redis::cmd("CONFIG")
                .arg("GET")
                .arg("databases")
                .query_async(&mut conn)
                .await
                .unwrap_or_default();
            Ok::<_, redis::RedisError>(
                res.get("databases")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(16),
            )
        })
        .unwrap_or(16);
    Ok((0..n).map(|i| format!("db{}", i)).collect())
}

/// 从键名提取前缀（首个 `:` 前的段；无冒号键自成一项）。
/// 抽成纯函数：list_prefixes 的分层规则在此，单测直接覆盖。
fn prefix_of(key: &str) -> String {
    match key.split_once(':') {
        Some((p, _)) => p.to_string(),
        None => key.to_string(),
    }
}

/// 枚举逻辑"库"（键前缀）：SCAN 全量键取首个 `:` 前的段去重（无冒号键自成一项）。
/// 上限 1 万键防止大库卡死；Redis 无库/表概念,前缀即约定俗成的分层。
pub fn list_prefixes(source: &str) -> Result<Vec<String>, String> {
    let conn = get_conn(source)?;
    let set: Vec<String> = rt().block_on(async {
        let mut conn = conn.clone();
        let mut cursor = 0u64;
        // HashSet 去重 O(1) 探测,末尾一次 sort 出有序 Vec（前缀数远小于扫描键数）
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut scanned = 0usize;
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("COUNT")
                .arg(1000)
                .query_async(&mut conn)
                .await
                .map_err(|e| e.to_string())?;
            cursor = next;
            for k in batch {
                scanned += 1;
                seen.insert(prefix_of(&k));
            }
            if cursor == 0 || scanned >= 10_000 {
                break;
            }
        }
        let mut set: Vec<String> = seen.into_iter().collect();
        set.sort();
        Ok::<_, String>(set)
    })?;
    Ok(set)
}

/// 键列表页：SCAN 游标翻页（按 offset 取第 page 页,页大小 PAGE_SIZE）
/// prefix 为 Some 时只取 `prefix:*` 的键。返回 (键名列表, 是否还有下一页)
pub fn scan_keys(
    source: &str,
    prefix: Option<&str>,
    page: usize,
) -> Result<(Vec<String>, bool), String> {
    let cache_src = format!("{}|{}", source, prefix.unwrap_or(""));
    // 命中缓存
    let cache_key = (cache_src.clone(), page);
    {
        let cache = scan_cache().lock().ok();
        if let Some((t, keys, more)) = cache.as_ref().and_then(|c| c.get(&cache_key)) {
            if t.elapsed() < CACHE_TTL {
                return Ok((keys.clone(), *more));
            }
        }
    }
    let conn = get_conn(source)?;
    // 从第 page 页重放 SCAN：游标不持久,简单起见每次从 0 扫到目标页
    // （键空间通常不大,SCAN COUNT = PAGE_SIZE 折中）
    let match_pat = prefix.map(|p| format!("{}:*", p));
    let (keys, more): (Vec<String>, bool) = rt().block_on(async {
        let mut conn = conn.clone();
        let mut cursor = 0u64;
        let mut collected: Vec<String> = Vec::new();
        loop {
            let (next, batch): (u64, Vec<String>) = {
                let mut cmd = redis::cmd("SCAN");
                cmd.arg(cursor).arg("MATCH");
                match &match_pat {
                    Some(p) => cmd.arg(p.as_str()),
                    None => cmd.arg("*"),
                };
                cmd.arg("COUNT")
                    .arg(PAGE_SIZE)
                    .query_async(&mut conn)
                    .await
                    .map_err(|e| e.to_string())?
            };
            cursor = next;
            collected.extend(batch);
            // 足够填满目标页 + 1 判断 more
            if collected.len() > page * PAGE_SIZE + PAGE_SIZE || cursor == 0 {
                break;
            }
        }
        let start = page * PAGE_SIZE;
        let has_more =
            collected.len() > start + PAGE_SIZE || (cursor != 0 && collected.len() > start);
        let more = has_more && collected.len() - start >= PAGE_SIZE;
        let page_keys: Vec<String> = collected.into_iter().skip(start).take(PAGE_SIZE).collect();
        Ok::<_, String>((page_keys, more))
    })?;
    if let Ok(mut c) = scan_cache().lock() {
        if c.len() >= SCAN_CACHE_MAX {
            c.clear();
        }
        c.insert(cache_key, (Instant::now(), keys.clone(), more));
    }
    Ok((keys, more))
}

/// 值条目（表格行）：键 / 类型 / TTL / 值(或元素数)
#[derive(Clone)]
pub struct KeyEntry {
    pub key: String,
    pub typ: String,
    pub ttl: i64,
    pub value: String,
}

/// 读取单个键的类型与值（按类型展开,大值截断）
pub fn read_key(source: &str, key: &str) -> Result<KeyEntry, String> {
    let conn = get_conn(source)?;
    let key = key.to_string();
    let (typ, ttl, value) = rt().block_on(async {
        let mut conn = conn.clone();
        let typ: String = redis::cmd("TYPE")
            .arg(&key)
            .query_async(&mut conn)
            .await
            .map_err(|e| e.to_string())?;
        let typ = typ.strip_prefix('+').unwrap_or(&typ).to_string();
        let ttl: i64 = conn.ttl(&key).await.unwrap_or(-2);
        // 按类型读值
        let value = match typ.as_str() {
            "string" => {
                let v: Vec<u8> = conn.get(&key).await.unwrap_or_default();
                truncate_bytes(&v)
            }
            "list" => {
                let len: i64 = conn.llen(&key).await.unwrap_or(0);
                let items: Vec<Vec<u8>> = conn.lrange(&key, 0, 9).await.unwrap_or_default();
                format_list("list", len, &items)
            }
            "set" => {
                let len: i64 = conn.scard(&key).await.unwrap_or(0);
                let items: Vec<Vec<u8>> = redis::cmd("SRANDMEMBER")
                    .arg(&key)
                    .arg(10)
                    .query_async(&mut conn)
                    .await
                    .unwrap_or_default();
                format_list("set", len, &items)
            }
            "zset" => {
                let len: i64 = conn.zcard(&key).await.unwrap_or(0);
                let items: Vec<(Vec<u8>, f64)> =
                    conn.zrange_withscores(&key, 0, 9).await.unwrap_or_default();
                let head: Vec<String> = items
                    .iter()
                    .map(|(m, s)| format!("{} = {}", truncate_bytes(m), s))
                    .collect();
                if len > 10 {
                    format!("zset({}) [{}; …]", len, head.join(", "))
                } else {
                    format!("zset({}) [{}]", len, head.join(", "))
                }
            }
            "hash" => {
                let len: i64 = conn.hlen(&key).await.unwrap_or(0);
                let items: Vec<(Vec<u8>, Vec<u8>)> = redis::cmd("HSCAN")
                    .arg(&key)
                    .arg(0)
                    .arg("COUNT")
                    .arg(10)
                    .query_async::<(u64, Vec<(Vec<u8>, Vec<u8>)>)>(&mut conn)
                    .await
                    .map(|(_, it)| it.into_iter().take(10).collect())
                    .unwrap_or_default();
                let head: Vec<String> = items
                    .iter()
                    .map(|(f, v)| format!("{} = {}", truncate_bytes(f), truncate_bytes(v)))
                    .collect();
                if len > 10 {
                    format!("hash({}) [{}; …]", len, head.join(", "))
                } else {
                    format!("hash({}) [{}]", len, head.join(", "))
                }
            }
            "stream" => {
                let len: i64 = redis::cmd("XLEN")
                    .arg(&key)
                    .query_async(&mut conn)
                    .await
                    .unwrap_or(0);
                format!("stream({})", len)
            }
            other => format!("<{}>", other),
        };
        Ok::<_, String>((typ, ttl, value))
    })?;
    Ok(KeyEntry {
        key,
        typ,
        ttl,
        value,
    })
}

/// 字节转可展示文本：UTF-8 直用,否则十六进制前缀,超长截断
fn truncate_bytes(v: &[u8]) -> String {
    let truncated = v.len() > MAX_VALUE_BYTES;
    let slice = if truncated { &v[..MAX_VALUE_BYTES] } else { v };
    let text = String::from_utf8_lossy(slice).into_owned();
    if truncated {
        format!("{}…(共 {} 字节)", text, v.len())
    } else {
        text
    }
}

/// 集合类值摘要：`list(123) [a, b, …]`
fn format_list(kind: &str, len: i64, items: &[Vec<u8>]) -> String {
    let head: Vec<String> = items.iter().take(10).map(|v| truncate_bytes(v)).collect();
    let more = if (len as usize) > items.len() {
        "; …"
    } else {
        ""
    };
    format!("{}({}) [{}{}]", kind, len, head.join(", "), more)
}

/// 执行任意命令（SQL 页对应物：命令行）,返回按行展示的结果
pub fn execute_command(source: &str, cmd: &str) -> Result<Vec<Vec<String>>, String> {
    let parts = split_command(cmd);
    if parts.is_empty() {
        return Err(lang::ERR_EMPTY_SQL());
    }
    // 写命令后失效浏览缓存（尽力而为：SET/DEL/FLUSH… 以首关键字粗判）
    let is_write = !matches!(
        parts[0].to_ascii_lowercase().as_str(),
        "get"
            | "mget"
            | "ttl"
            | "pttl"
            | "exists"
            | "type"
            | "scan"
            | "keys"
            | "llen"
            | "scard"
            | "zcard"
            | "hlen"
            | "xlen"
            | "lrange"
            | "lindex"
            | "hget"
            | "hmget"
            | "hgetall"
            | "smembers"
            | "zrange"
            | "dbsize"
            | "info"
            | "ping"
            | "select"
            | "config"
            | "memory"
            | "strlen"
            | "getrange"
            | "object"
            | "randomkey"
    );
    let conn = get_conn(source)?;
    let res: redis::Value = rt()
        .block_on(async {
            let mut conn = conn.clone();
            let mut c = redis::cmd(&parts[0].to_ascii_uppercase());
            for p in &parts[1..] {
                c.arg(p.as_str());
            }
            c.query_async(&mut conn).await
        })
        .map_err(|e| e.to_string())?;
    if is_write {
        invalidate();
    }
    Ok(value_to_rows(res))
}

/// 命令拆分：按空白,引号包裹的段保留空格
fn split_command(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote: Option<char> = None;
    for c in cmd.chars() {
        match in_quote {
            Some(q) if c == q => in_quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => in_quote = Some(c),
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// redis::Value → 表格行（首行是表头）
fn value_to_rows(v: redis::Value) -> Vec<Vec<String>> {
    fn cell(v: &redis::Value) -> String {
        match v {
            redis::Value::Nil => "(nil)".to_string(),
            redis::Value::Int(i) => i.to_string(),
            redis::Value::BulkString(b) => truncate_bytes(b),
            redis::Value::SimpleString(s) => s.clone(),
            redis::Value::Okay => "OK".to_string(),
            redis::Value::Array(items) | redis::Value::Set(items) => {
                items.iter().map(cell).collect::<Vec<_>>().join(", ")
            }
            redis::Value::Map(pairs) => pairs
                .iter()
                .map(|(k, v)| format!("{} = {}", cell(k), cell(v)))
                .collect::<Vec<_>>()
                .join(", "),
            redis::Value::Double(d) => d.to_string(),
            redis::Value::Boolean(b) => b.to_string(),
            redis::Value::BigNumber(n) => String::from_utf8_lossy(n).into_owned(),
            redis::Value::VerbatimString { text, .. } => text.clone(),
            redis::Value::ServerError(e) => format!("ERR {}", e.details().unwrap_or("")),
            other => format!("<{:?}>", other),
        }
    }
    match v {
        redis::Value::Array(items) if !items.is_empty() => {
            // 数组结果：首列统一「#」,单列展示（与 redis-cli 的列表输出近似）
            vec![vec!["value".to_string()]]
                .into_iter()
                .chain(items.iter().map(|i| vec![cell(i)]))
                .collect()
        }
        other => vec![vec!["value".to_string()], vec![cell(&other)]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_hides_password() {
        assert_eq!(
            display_of("redis://:secret@127.0.0.1:6379/0"),
            "127.0.0.1:6379/0"
        );
        assert_eq!(display_of("redis://127.0.0.1:6379"), "127.0.0.1:6379");
        assert_eq!(display_of("127.0.0.1:6379"), "127.0.0.1:6379");
        // 密码可含 '@'：取最后一个
        assert_eq!(display_of("redis://:p@ss@host:6379/0"), "host:6379/0");
    }

    #[test]
    fn url_for_db_replaces_db() {
        assert_eq!(url_for_db("redis://:p@h:6379/0", 3), "redis://:p@h:6379/3");
        assert_eq!(url_for_db("redis://h:6379", 2), "redis://h:6379/2");
        assert_eq!(
            url_for_db("redis://h:6379/0?foo=1", 1),
            "redis://h:6379/1?foo=1"
        );
    }

    #[test]
    fn command_split_respects_quotes() {
        assert_eq!(
            split_command(r#"SET key "hello world""#),
            vec!["SET", "key", "hello world"]
        );
        assert_eq!(split_command("  GET   k1 "), vec!["GET", "k1"]);
        assert_eq!(split_command(""), Vec::<String>::new());
        // 引号内空白保留,引号本身吃掉
        assert_eq!(split_command("GET 'a b'"), vec!["GET", "a b"]);
    }

    #[test]
    fn value_rows_shapes() {
        // 标量 → 两行（表头 + 值）
        let rows = value_to_rows(redis::Value::BulkString(b"v".to_vec()));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["value"]);
        assert_eq!(rows[1], vec!["v"]);
        // 数组 → 表头 + 每元素一行
        let rows = value_to_rows(redis::Value::Array(vec![
            redis::Value::Int(1),
            redis::Value::Okay,
        ]));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], vec!["1"]);
        assert_eq!(rows[2], vec!["OK"]);
        // Nil
        let rows = value_to_rows(redis::Value::Nil);
        assert_eq!(rows[1], vec!["(nil)"]);
    }

    #[test]
    fn truncate_bytes_limits_and_keeps_utf8() {
        assert_eq!(truncate_bytes(b"hello"), "hello");
        let big = vec![b'a'; MAX_VALUE_BYTES + 10];
        let out = truncate_bytes(&big);
        assert!(out.contains("共"), "{}", out);
        assert!(out.starts_with('a'));
    }

    #[test]
    fn list_format_variants() {
        let items = vec![b"a".to_vec(), b"b".to_vec()];
        assert_eq!(format_list("list", 2, &items), "list(2) [a, b]");
        assert_eq!(format_list("list", 5, &items), "list(5) [a, b; …]");
        assert_eq!(format_list("set", 0, &[]), "set(0) []");
    }

    // ────────────── prefix_of：逻辑库分层规则 ──────────────

    #[test]
    fn prefix_of_splits_at_first_colon() {
        // 常规业务键：首个 `:` 前是逻辑库
        assert_eq!(prefix_of("hltqh:user:1001"), "hltqh");
        assert_eq!(prefix_of("sys_dict:config"), "sys_dict");
        // 多级前缀只取第一段
        assert_eq!(prefix_of("a:b:c:d"), "a");
    }

    #[test]
    fn prefix_of_no_colon_is_itself() {
        // 无冒号键自成一项（不做分层）
        assert_eq!(prefix_of("standalone_key"), "standalone_key");
        assert_eq!(prefix_of("counter"), "counter");
    }

    #[test]
    fn prefix_of_edge_cases() {
        // 冒号开头：前缀为空串（保留真实行为,UI 显示原样）
        assert_eq!(prefix_of(":value"), "");
        // 空键
        assert_eq!(prefix_of(""), "");
        // 仅冒号
        assert_eq!(prefix_of(":"), "");
    }

    // ────────────── url_for_db 边界补充 ──────────────

    #[test]
    fn url_for_db_edge_cases() {
        // 无协议前缀
        assert_eq!(url_for_db("127.0.0.1:6379", 5), "127.0.0.1:6379/5");
        // 已带 db + 查询参数整体保留
        assert_eq!(
            url_for_db("redis://:p@h:6379/2?foo=1&bar=2", 9),
            "redis://:p@h:6379/9?foo=1&bar=2"
        );
        // 密码里含 '/'：authority 取最后一个 '@' 之后,不受密码干扰
        assert_eq!(
            url_for_db("redis://:a/b@h:6379/0", 1),
            "redis://:a/b@h:6379/1"
        );
        // 密码里含 '@'：同上
        assert_eq!(
            url_for_db("redis://:p@ss@h:6379/0", 7),
            "redis://:p@ss@h:6379/7"
        );
        // 大 db 号
        assert_eq!(url_for_db("redis://h:6379/0", 15), "redis://h:6379/15");
    }

    // ────────────── display_of 边界补充 ──────────────

    #[test]
    fn display_of_edge_cases() {
        // 空串
        assert_eq!(display_of(""), "");
        // 带查询参数
        assert_eq!(display_of("redis://:pw@h:6379/0?x=1"), "h:6379/0?x=1");
        // 无 db 段
        assert_eq!(display_of("redis://:pw@h:6379"), "h:6379");
    }

    // ────────────── split_command 边界补充 ──────────────

    #[test]
    fn split_command_edge_cases() {
        // 未闭合引号：整段进最后一个 token（不 panic 即可,行为是宽松的）
        let out = split_command(r#"GET "unclosed"#);
        assert_eq!(out.first().map(String::as_str), Some("GET"));
        // 嵌套引号内空格
        assert_eq!(split_command("SET k \"a  b\""), vec!["SET", "k", "a  b"]);
        // 多个连续引号段
        assert_eq!(
            split_command(r#"MGET "a b" "c d""#),
            vec!["MGET", "a b", "c d"]
        );
    }

    // ────────────── value_to_rows 复合类型 ──────────────

    #[test]
    fn value_rows_composite_shapes() {
        // 嵌套数组拍平为逗号连接
        let rows = value_to_rows(redis::Value::Array(vec![redis::Value::Array(vec![
            redis::Value::BulkString(b"x".to_vec()),
            redis::Value::BulkString(b"y".to_vec()),
        ])]));
        assert_eq!(rows[1], vec!["x, y"]);
        // Map
        let rows = value_to_rows(redis::Value::Map(vec![(
            redis::Value::BulkString(b"k".to_vec()),
            redis::Value::Int(3),
        )]));
        assert_eq!(rows[1], vec!["k = 3"]);
        // 空数组走标量分支：表头 + 一个空 cell（真实行为,与 redis-cli 的 (empty) 一致）
        let rows = value_to_rows(redis::Value::Array(vec![]));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["value"]);
        // Boolean / Double
        let rows = value_to_rows(redis::Value::Boolean(true));
        assert_eq!(rows[1], vec!["true"]);
    }

    // ────────────── 连接串合法性（离线校验：只测 parse,不连网） ──────────────

    #[test]
    fn url_parse_variants() {
        // 这些都是 redis-rs 接受的合法写法（parse 失败即 panic）
        for url in [
            "redis://127.0.0.1:6379/0",
            "redis://:pass@127.0.0.1:6379/0",
            "redis://user:pass@host:6379",
            "redis://host:6379/0?foo=1",
        ] {
            assert!(redis::Client::open(url).is_ok(), "应能解析: {url}");
        }
        // 非法串 parse 必须报错而不是 panic
        assert!(redis::Client::open("://bad").is_err());
    }
}
