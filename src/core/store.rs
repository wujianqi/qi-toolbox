//! 本地 Turso(libsql) 存储层：应用自身数据统一入库，不再落 txt 文件
//!
//! - 数据库文件：`%APPDATA%\qi-toolbox\store.db`（`config_dir()` 复用）
//! - 敏感值（SFTP 密码 / TOTP 密钥等）写库前经应用主口令派生密钥加密（core::master，
//!   `v2:` 前缀 base64），读取时解密；未解锁时敏感列拒绝写入（与 settings 一致）
//! - 每次操作在独立后台线程 + 单线程 tokio runtime 中执行（与 turso::run_query 同模式）；
//!   仅缓存本地 `Database` 句柄（turso::DB_CACHE 只服务浏览页，不共用，避免互踢）

use std::sync::Mutex;

use turso::Builder;

use crate::core::settings::config_dir;
use crate::core::turso::value_to_string;

/// 请求表结构（SFTP 多站点管理）
#[derive(Debug, Clone)]
pub struct SftpSite {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}

/// SSH 常用命令条目（id/name 仅供库内排序，UI 只消费分组与命令文本）
#[derive(Debug, Clone)]
pub struct SshCmd {
    pub category: String,
    pub command: String,
}

/// TOTP 密钥条目（多密钥管理）
#[derive(Debug, Clone)]
pub struct TotpKey {
    pub id: i64,
    pub name: String,
    pub secret: String,
    pub algo: i64, // 0=SHA1 1=SHA256 2=SHA512
}

// ────────────────────── 连接管理 ──────────────────────

static STORE_DB: Mutex<Option<turso::Database>> = Mutex::new(None);

/// 数据库文件路径。
/// 测试可设 `QI_STORE_DB` 环境变量指向独立文件——turso `new_local` 默认
/// 单进程独占（multiprocess_wal 关闭），App 运行时持有锁，测试进程再开
/// 同一文件会报 os error 33，故测试必须与真实库隔离。
pub fn db_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("QI_STORE_DB") {
        if !p.trim().is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    config_dir().join("store.db")
}

/// 在专用后台线程上执行一次异步查询（本地文件库），返回闭包产出的结果
fn run<F, Fut, T>(f: F) -> Result<T, String>
where
    F: FnOnce(turso::Connection) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>> + Send + 'static,
    T: Send + 'static,
{
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {}", e))?;
        rt.block_on(async {
            let db = match STORE_DB.lock().ok().and_then(|g| g.clone()) {
                Some(db) => db,
                None => {
                    if let Some(dir) = db_path().parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let db = Builder::new_local(db_path().to_string_lossy().as_ref())
                        .build()
                        .await
                        .map_err(|e| format!("open store: {}", e))?;
                    if let Ok(mut g) = STORE_DB.lock() {
                        *g = Some(db.clone());
                    }
                    db
                }
            };
            let conn = db.connect().map_err(|e| format!("connect: {}", e))?;
            f(conn).await
        })
    });
    handle
        .join()
        .map_err(|_| "store thread panic".to_string())?
}

/// 快照 store.db 到临时文件，返回临时文件路径。
/// 备份用：数据库句柄常开（WAL 模式），直接复制主文件可能缺最新页，
/// 先执行 `wal_checkpoint(TRUNCATE)` 把 WAL 合入主文件再复制，保证快照完整。
pub fn snapshot_db_path() -> Result<std::path::PathBuf, String> {
    run(|conn| async move {
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", ())
            .await
            .map_err(|e| format!("checkpoint: {}", e))?;
        Ok(())
    })?;
    let snap = std::env::temp_dir().join(format!("qi-toolbox-backup-{}.db", std::process::id()));
    std::fs::copy(db_path(), &snap).map_err(|e| format!("snapshot: {}", e))?;
    Ok(snap)
}

// ────────────────────── 敏感值加解密 ──────────────────────

/// 加密为 `enc` 列值：主口令派生密钥（`v2:` 前缀 base64，随库走换机可解）。
/// 主口令未解锁时返回 Err，调用方拒绝写入（宁可丢失也不落明文）。
fn protect(value: &str) -> Result<String, String> {
    let cipher = crate::core::master::protect(value.as_bytes())?;
    Ok(String::from_utf8_lossy(&cipher).into_owned())
}

/// 解密 `enc` 列值（主口令 `v2:` 密文；口令错误/损坏返回 Err）
fn unprotect(enc: &str) -> Result<String, String> {
    let plain = crate::core::master::unprotect(enc.as_bytes())?;
    Ok(String::from_utf8_lossy(&plain).into_owned())
}

// ────────────────────── Schema ──────────────────────

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS kv (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sftp_sites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    user TEXT NOT NULL,
    pass_enc TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS ssh_cmds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    category TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    sort INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS totp_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    secret_enc TEXT NOT NULL,
    algo INTEGER NOT NULL DEFAULT 0,
    sort INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS s3_sites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    endpoint TEXT NOT NULL,
    region TEXT NOT NULL DEFAULT '',
    bucket TEXT NOT NULL,
    access_key TEXT NOT NULL,
    secret_enc TEXT NOT NULL,
    path_style INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS mysql_sites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 3306,
    user TEXT NOT NULL,
    pass_enc TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS turso_dbs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    path TEXT NOT NULL DEFAULT '',
    url_enc TEXT NOT NULL DEFAULT '',
    token_enc TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS pg_sites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    url_enc TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS sftp_bmks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS sql_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    sql TEXT NOT NULL,
    ts INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS saved_passwords (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    used_for TEXT NOT NULL DEFAULT '',
    pwd_enc TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS remote_urls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS master_cred (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    salt TEXT NOT NULL,
    verifier TEXT NOT NULL
);
";

/// 建 schema（应用启动时调用一次；失败返回错误文案，由调用方决定是否提示）
pub fn init() -> Result<(), String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            conn.execute_batch(SCHEMA)
                .await
                .map_err(|e| format!("init store: {}", e))?;
            // 旧版 pg_sites 表的 url 列是明文连接串：改名为 url_enc（内容下次保存时自动升级为密文）。
            // 全新库无 url 列，此句报错属预期，静默忽略。
            let _ = conn
                .execute("ALTER TABLE pg_sites RENAME COLUMN url TO url_enc", ())
                .await;
            Ok(())
        })
    }))
}

// ────────────────────── kv（设置 / 记忆输入） ──────────────────────

/// 读主口令校验记录（None = 尚未设置）
pub fn master_cred_get() -> Result<Option<(String, String)>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut rows = conn
                .query("SELECT salt, verifier FROM master_cred WHERE id = 1", ())
                .await
                .map_err(|e| e.to_string())?;
            if let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                let salt = value_to_string(&row, 0);
                let verifier = value_to_string(&row, 1);
                return Ok(Some((salt, verifier)));
            }
            Ok(None)
        })
    }))
}

/// 写主口令校验记录（upsert，单行）
pub fn master_cred_set(salt: &str, verifier: &str) -> Result<(), String> {
    let (salt, verifier) = (salt.to_string(), verifier.to_string());
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT INTO master_cred (id, salt, verifier) VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET salt = ?1, verifier = ?2",
                (salt.as_str(), verifier.as_str()),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 读全部 kv（value 为明文）
pub fn kv_all() -> Result<Vec<(String, String)>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query("SELECT key, value FROM kv", ())
                .await
                .map_err(|e| e.to_string())?;
            while let Some(group) = rows.next().await.map_err(|e| e.to_string())? {
                let key = value_to_string(&group, 0);
                let value = value_to_string(&group, 1);
                out.push((key, value));
            }
            Ok(out)
        })
    }))
}

/// 合并写 kv（幂等 upsert）
pub fn kv_set(entries: &[(&str, &str)]) -> Result<(), String> {
    let owned: Vec<(String, String)> = entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            for (k, v) in &owned {
                conn.execute(
                    "INSERT INTO kv(key, value) VALUES(?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    (k.as_str(), v.as_str()),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 删除 kv 键
pub fn kv_del(key: &str) -> Result<(), String> {
    let key = key.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM kv WHERE key = ?1", (key.as_str(),))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── SFTP 多站点 ──────────────────────

fn row_to_site(row: &turso::Row) -> Result<SftpSite, String> {
    let id: i64 = row.get(0).map_err(|e| e.to_string())?;
    let name = value_to_string(row, 1);
    let host = value_to_string(row, 2);
    let port: i64 = row.get(3).map_err(|e| e.to_string())?;
    let user = value_to_string(row, 4);
    let enc = value_to_string(row, 5);
    let pass = if enc.is_empty() {
        String::new()
    } else {
        unprotect(&enc).unwrap_or_default()
    };
    Ok(SftpSite {
        id,
        name,
        host,
        port: port.clamp(1, 65535) as u16,
        user,
        pass,
    })
}

/// 列出全部站点（按名称排序；密码已解密）
pub fn sftp_list() -> Result<Vec<SftpSite>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, name, host, port, user, pass_enc FROM sftp_sites ORDER BY name",
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(row_to_site(&row)?);
            }
            Ok(out)
        })
    }))
}

/// 新增 / 更新站点（id=0 新增；密码加密存 `pass_enc`）
pub fn sftp_upsert(site: &SftpSite) -> Result<(), String> {
    let enc = if site.pass.is_empty() {
        String::new()
    } else {
        protect(&site.pass)?
    };
    let (name, host, user) = (site.name.clone(), site.host.clone(), site.user.clone());
    let port = site.port as i64;
    let id = site.id;
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            if id <= 0 {
                conn.execute(
                    "INSERT INTO sftp_sites(name, host, port, user, pass_enc) VALUES(?1,?2,?3,?4,?5)",
                    (name.as_str(), host.as_str(), port, user.as_str(), enc.as_str()),
                )
                .await.map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE sftp_sites SET name=?1, host=?2, port=?3, user=?4, pass_enc=?5 WHERE id=?6",
                    (name.as_str(), host.as_str(), port, user.as_str(), enc.as_str(), id),
                )
                .await.map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 删除站点
pub fn sftp_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM sftp_sites WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── SSH 常用命令 ──────────────────────

fn row_to_cmd(row: &turso::Row) -> Result<SshCmd, String> {
    Ok(SshCmd {
        category: value_to_string(row, 1),
        command: value_to_string(row, 3),
    })
}

/// 列出全部命令（按 sort, id）
pub fn ssh_list() -> Result<Vec<SshCmd>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, category, name, command FROM ssh_cmds ORDER BY sort, id",
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(row_to_cmd(&row)?);
            }
            Ok(out)
        })
    }))
}

/// 首次使用时播种内置常用命令（表内无任何分组命令才写入；category 空串=「我的命令」，不参与判定）
pub fn ssh_seed_if_empty(rows: &[(String, String)]) -> Result<(), String> {
    let owned: Vec<(String, String)> = rows.to_vec();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            let mut rows = conn
                .query("SELECT COUNT(*) FROM ssh_cmds WHERE category <> ''", ())
                .await
                .map_err(|e| e.to_string())?;
            let n: i64 = match rows.next().await.map_err(|e| e.to_string())? {
                Some(row) => row.get(0).map_err(|e| e.to_string())?,
                None => 0,
            };
            if n > 0 {
                return Ok(());
            }
            for (cat, cmd) in &owned {
                conn.execute(
                    "INSERT INTO ssh_cmds(category, name, command) VALUES(?1, ?2, ?2)",
                    (cat.as_str(), cmd.as_str()),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 「我的命令」追加一条（category 空串；按命令文本去重）
pub fn ssh_add_custom(command: &str) -> Result<(), String> {
    let command = command.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT INTO ssh_cmds(category, name, command)
                 SELECT '', ?1, ?1 WHERE NOT EXISTS
                     (SELECT 1 FROM ssh_cmds WHERE command = ?1 AND category = '')",
                (command.as_str(),),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 「我的命令」按命令文本删除一条
pub fn ssh_del_custom(command: &str) -> Result<(), String> {
    let command = command.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "DELETE FROM ssh_cmds WHERE category = '' AND command = ?1",
                (command.as_str(),),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── S3 站点 ──────────────────────

/// S3 站点（endpoint 含协议如 https://...；secret 加密存 secret_enc）
#[derive(Debug, Clone)]
pub struct S3Site {
    pub id: i64,
    pub name: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret: String,
    pub path_style: bool,
}

fn row_to_s3(row: &turso::Row) -> Result<S3Site, String> {
    let id: i64 = row.get(0).map_err(|e| e.to_string())?;
    let enc = value_to_string(row, 6);
    Ok(S3Site {
        id,
        name: value_to_string(row, 1),
        endpoint: value_to_string(row, 2),
        region: value_to_string(row, 3),
        bucket: value_to_string(row, 4),
        access_key: value_to_string(row, 5),
        secret: if enc.is_empty() {
            String::new()
        } else {
            unprotect(&enc).unwrap_or_default()
        },
        path_style: value_to_string(row, 7) == "1",
    })
}

/// 列出全部 S3 站点（按名称排序；secret 已解密）
pub fn s3_list() -> Result<Vec<S3Site>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, name, endpoint, region, bucket, access_key, secret_enc, path_style FROM s3_sites ORDER BY name",
                    (),
                )
                .await.map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(row_to_s3(&row)?);
            }
            Ok(out)
        })
    }))
}

/// 新增 / 更新 S3 站点（id=0 新增；secret 加密存 secret_enc）
pub fn s3_upsert(site: &S3Site) -> Result<(), String> {
    let enc = if site.secret.is_empty() {
        String::new()
    } else {
        protect(&site.secret)?
    };
    let (name, endpoint, region, bucket, access_key) = (
        site.name.clone(),
        site.endpoint.clone(),
        site.region.clone(),
        site.bucket.clone(),
        site.access_key.clone(),
    );
    let (path_style, id) = (site.path_style as i64, site.id);
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            if id <= 0 {
                conn.execute(
                    "INSERT INTO s3_sites(name, endpoint, region, bucket, access_key, secret_enc, path_style)
                     VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    (name.as_str(), endpoint.as_str(), region.as_str(), bucket.as_str(), access_key.as_str(), enc.as_str(), path_style),
                )
                .await.map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE s3_sites SET name=?1, endpoint=?2, region=?3, bucket=?4, access_key=?5, secret_enc=?6, path_style=?7 WHERE id=?8",
                    (name.as_str(), endpoint.as_str(), region.as_str(), bucket.as_str(), access_key.as_str(), enc.as_str(), path_style, id),
                )
                .await.map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 删除 S3 站点
pub fn s3_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM s3_sites WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── MySQL / PG 多连接站点 ──────────────────────

/// MySQL 连接配置（pass 解密后的明文，仅在内存中流转）
#[derive(Clone, Debug, Default)]
pub struct MySqlSite {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}

fn row_to_mysql_site(row: &turso::Row) -> Result<MySqlSite, String> {
    let id: i64 = row.get(0).map_err(|e| e.to_string())?;
    let enc = value_to_string(row, 5);
    let port: i64 = row.get(3).map_err(|e| e.to_string())?;
    Ok(MySqlSite {
        id,
        name: value_to_string(row, 1),
        host: value_to_string(row, 2),
        port: port.max(0) as u16,
        user: value_to_string(row, 4),
        pass: if enc.is_empty() {
            String::new()
        } else {
            unprotect(&enc).unwrap_or_default()
        },
    })
}

/// 列出全部 MySQL 站点（按名称排序；密码已解密）
pub fn mysql_site_list() -> Result<Vec<MySqlSite>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, name, host, port, user, pass_enc FROM mysql_sites ORDER BY name",
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(row_to_mysql_site(&row)?);
            }
            Ok(out)
        })
    }))
}

/// 新增 / 更新 MySQL 站点（id=0 新增；密码加密存 pass_enc）
pub fn mysql_site_upsert(site: &MySqlSite) -> Result<(), String> {
    let enc = if site.pass.is_empty() {
        String::new()
    } else {
        protect(&site.pass)?
    };
    let (name, host, user) = (site.name.clone(), site.host.clone(), site.user.clone());
    let (port, id) = (site.port as i64, site.id);
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            if id <= 0 {
                conn.execute(
                    "INSERT INTO mysql_sites(name, host, port, user, pass_enc) VALUES(?1,?2,?3,?4,?5)",
                    (name.as_str(), host.as_str(), port, user.as_str(), enc.as_str()),
                )
                .await
                .map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE mysql_sites SET name=?1, host=?2, port=?3, user=?4, pass_enc=?5 WHERE id=?6",
                    (name.as_str(), host.as_str(), port, user.as_str(), enc.as_str(), id),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 删除 MySQL 站点
pub fn mysql_site_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM mysql_sites WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// PG 连接配置（整条连接串加密存 `url_enc`：内含密码，视同敏感项）
#[derive(Clone, Debug, Default)]
pub struct PgSite {
    pub id: i64,
    pub name: String,
    pub url: String,
}

fn row_to_pg_site(row: &turso::Row) -> Result<PgSite, String> {
    let id: i64 = row.get(0).map_err(|e| e.to_string())?;
    let enc = value_to_string(row, 2);
    let url = if enc.is_empty() {
        String::new()
    } else {
        match unprotect(&enc) {
            Ok(u) => u,
            // 旧版明文连接串：原样返回（下次保存时自动升级为密文）
            Err(_) => enc,
        }
    };
    Ok(PgSite {
        id,
        name: value_to_string(row, 1),
        url,
    })
}

/// 列出全部 PG 站点（按名称排序；连接串已解密）
pub fn pg_site_list() -> Result<Vec<PgSite>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query("SELECT id, name, url_enc FROM pg_sites ORDER BY name", ())
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(row_to_pg_site(&row)?);
            }
            Ok(out)
        })
    }))
}

/// 新增 / 更新 PG 站点（id=0 新增；连接串加密存 url_enc，旧明文自动升级）
pub fn pg_site_upsert(site: &PgSite) -> Result<(), String> {
    let enc = if site.url.is_empty() {
        String::new()
    } else {
        protect(&site.url)?
    };
    let (name, id) = (site.name.clone(), site.id);
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            if id <= 0 {
                conn.execute(
                    "INSERT INTO pg_sites(name, url_enc) VALUES(?1,?2)",
                    (name.as_str(), enc.as_str()),
                )
                .await
                .map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE pg_sites SET name=?1, url_enc=?2 WHERE id=?3",
                    (name.as_str(), enc.as_str(), id),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 删除 PG 站点
pub fn pg_site_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM pg_sites WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// Turso 数据库连接配置（多库管理）。
/// `kind`: 0=本地文件（`path`），1=网络（`url`+`token`，加密存）。
#[derive(Clone, Debug, Default)]
pub struct TursoDb {
    pub id: i64,
    pub name: String,
    pub kind: i64,
    pub path: String,
    pub url: String,
    pub token: String,
}

fn row_to_turso_db(row: &turso::Row) -> TursoDb {
    let enc_url = value_to_string(row, 3);
    let enc_tok = value_to_string(row, 4);
    let url = if enc_url.is_empty() {
        String::new()
    } else {
        // 旧版明文：原样返回（下次保存时自动升级为密文）
        unprotect(&enc_url).unwrap_or(enc_url)
    };
    let token = if enc_tok.is_empty() {
        String::new()
    } else {
        unprotect(&enc_tok).unwrap_or(enc_tok)
    };
    TursoDb {
        id: row.get(0).unwrap_or(0),
        name: value_to_string(row, 1),
        kind: row.get(2).unwrap_or(0),
        path: value_to_string(row, 5),
        url,
        token,
    }
}

/// 列出全部 Turso 库（按名称排序；敏感字段已解密）
pub fn turso_db_list() -> Result<Vec<TursoDb>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, name, kind, url_enc, token_enc, path FROM turso_dbs ORDER BY name",
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(row_to_turso_db(&row));
            }
            Ok(out)
        })
    }))
}

/// 新增 / 更新 Turso 库（id=0 新增；url/token 加密落库）
pub fn turso_db_upsert(db: &TursoDb) -> Result<(), String> {
    let enc_url = if db.url.is_empty() {
        String::new()
    } else {
        protect(&db.url)?
    };
    let enc_tok = if db.token.is_empty() {
        String::new()
    } else {
        protect(&db.token)?
    };
    let (name, kind, path, id) = (db.name.clone(), db.kind, db.path.clone(), db.id);
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            if id <= 0 {
                conn.execute(
                    "INSERT INTO turso_dbs(name, kind, path, url_enc, token_enc) VALUES(?1,?2,?3,?4,?5)",
                    (
                        name.as_str(),
                        kind,
                        path.as_str(),
                        enc_url.as_str(),
                        enc_tok.as_str(),
                    ),
                )
                .await
                .map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE turso_dbs SET name=?1, kind=?2, path=?3, url_enc=?4, token_enc=?5 WHERE id=?6",
                    (
                        name.as_str(),
                        kind,
                        path.as_str(),
                        enc_url.as_str(),
                        enc_tok.as_str(),
                        id,
                    ),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 删除 Turso 库
pub fn turso_db_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM turso_dbs WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── SFTP 目录书签 ──────────────────────

/// 列出全部 SFTP 目录书签（按 id 排序）
pub fn sftp_bmk_list() -> Result<Vec<String>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query("SELECT path FROM sftp_bmks ORDER BY id", ())
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(value_to_string(&row, 0));
            }
            Ok(out)
        })
    }))
}

/// 新增书签（已存在则忽略）
pub fn sftp_bmk_add(path: &str) -> Result<(), String> {
    let path = path.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT INTO sftp_bmks(path) VALUES(?1) ON CONFLICT(path) DO NOTHING",
                (path.as_str(),),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 删除书签
pub fn sftp_bmk_del(path: &str) -> Result<(), String> {
    let path = path.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM sftp_bmks WHERE path = ?1", (path.as_str(),))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── SQL 执行历史 ──────────────────────

/// 历史保留条数（每类连接源各留最近 N 条）
const SQL_HISTORY_KEEP: usize = 50;

/// 读某连接源的历史（新→旧，最多 SQL_HISTORY_KEEP 条）
pub fn sql_history_list(source: &str) -> Result<Vec<String>, String> {
    let source = source.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT sql FROM sql_history WHERE source = ?1 ORDER BY id DESC LIMIT ?2",
                    (source.as_str(), SQL_HISTORY_KEEP as i64),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(value_to_string(&row, 0));
            }
            Ok(out)
        })
    }))
}

/// 追加一条历史并裁剪旧条目（同 SQL 去重：已存在则先删旧记录提到最新）
pub fn sql_history_add(source: &str, sql: &str) -> Result<(), String> {
    let (source, sql) = (source.to_string(), sql.to_string());
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "DELETE FROM sql_history WHERE source = ?1 AND sql = ?2",
                (source.as_str(), sql.as_str()),
            )
            .await
            .map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO sql_history(source, sql, ts) VALUES(?1, ?2, ?3)",
                (source.as_str(), sql.as_str(), now_secs()),
            )
            .await
            .map_err(|e| e.to_string())?;
            // 裁剪：只保留最近 N 条
            conn.execute(
                "DELETE FROM sql_history WHERE source = ?1 AND id NOT IN (
                     SELECT id FROM sql_history WHERE source = ?1 ORDER BY id DESC LIMIT ?2
                 )",
                (source.as_str(), SQL_HISTORY_KEEP as i64),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 清空某连接源的全部历史
pub fn sql_history_clear(source: &str) -> Result<(), String> {
    let source = source.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "DELETE FROM sql_history WHERE source = ?1",
                (source.as_str(),),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 当前 Unix 秒
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ────────────────────── TOTP 密钥 ──────────────────────

fn row_to_key(row: &turso::Row) -> Result<TotpKey, String> {
    let id: i64 = row.get(0).map_err(|e| e.to_string())?;
    let enc = value_to_string(row, 2);
    Ok(TotpKey {
        id,
        name: value_to_string(row, 1),
        secret: unprotect(&enc).unwrap_or_default(),
        algo: row.get(3).map_err(|e| e.to_string())?,
    })
}

/// 列出全部密钥（密钥已解密；解密失败/换机的条目跳过）
pub fn totp_list() -> Result<Vec<TotpKey>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, name, secret_enc, algo FROM totp_keys ORDER BY sort, id",
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                // 解密失败的密钥（换用户/换机器）整条丢弃，不阻断其余条目
                if let Ok(k) = row_to_key(&row) {
                    if !k.secret.is_empty() {
                        out.push(k);
                    }
                }
            }
            Ok(out)
        })
    }))
}

/// 新增 / 更新密钥（id=0 新增；密钥加密存 `secret_enc`）
pub fn totp_upsert(key: &TotpKey) -> Result<(), String> {
    let enc = protect(&key.secret)?;
    let (name, algo) = (key.name.clone(), key.algo);
    let id = key.id;
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            if id <= 0 {
                conn.execute(
                    "INSERT INTO totp_keys(name, secret_enc, algo) VALUES(?1,?2,?3)",
                    (name.as_str(), enc.as_str(), algo),
                )
                .await
                .map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE totp_keys SET name=?1, secret_enc=?2, algo=?3 WHERE id=?4",
                    (name.as_str(), enc.as_str(), algo, id),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }))
}

/// 删除密钥
pub fn totp_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM totp_keys WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── 保存的密码（密码页） ──────────────────────

/// 保存的密码条目（密码已解密）
#[derive(Debug, Clone)]
pub struct SavedPassword {
    pub id: i64,
    /// 用途备注（「用于：」自定义文本）
    pub used_for: String,
    pub password: String,
}

/// 列出全部保存的密码（密码已解密；解密失败/换机的条目跳过）
pub fn saved_pwd_list() -> Result<Vec<SavedPassword>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, used_for, pwd_enc FROM saved_passwords ORDER BY id",
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                let id: i64 = row.get(0).map_err(|e| e.to_string())?;
                let used_for = value_to_string(&row, 1);
                let enc = value_to_string(&row, 2);
                // 解密失败的条目（换用户/换机器）整条跳过，不阻断其余条目
                if let Ok(password) = unprotect(&enc) {
                    out.push(SavedPassword {
                        id,
                        used_for,
                        password,
                    });
                }
            }
            Ok(out)
        })
    }))
}

/// 新增一条保存的密码（密码加密存 `pwd_enc`）
pub fn saved_pwd_add(used_for: &str, password: &str) -> Result<(), String> {
    let enc = protect(password)?;
    let used_for = used_for.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT INTO saved_passwords(used_for, pwd_enc) VALUES(?1, ?2)",
                (used_for.as_str(), enc.as_str()),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 删除一条保存的密码
pub fn saved_pwd_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM saved_passwords WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── 远程检测网址管理 ──────────────────────

/// 列出全部已保存网址（按 id 排序）
pub fn remote_url_list() -> Result<Vec<String>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query("SELECT url FROM remote_urls ORDER BY id", ())
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(value_to_string(&row, 0));
            }
            Ok(out)
        })
    }))
}

/// 新增网址（已存在则忽略，UNIQUE 冲突静默去重）
pub fn remote_url_add(url: &str) -> Result<(), String> {
    let url = url.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT OR IGNORE INTO remote_urls(url) VALUES(?1)",
                (url.as_str(),),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 删除网址
pub fn remote_url_del(url: &str) -> Result<(), String> {
    let url = url.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM remote_urls WHERE url = ?1", (url.as_str(),))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// store.db 是进程级单例连接：并行测试同时建表会触发 database is locked，
    /// 用互斥锁把写库类测试串行化
    static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 测试独占的临时库路径（进程内一次性生成；QI_STORE_DB 在首次 init 前生效）
    fn temp_db() -> std::path::PathBuf {
        use std::sync::OnceLock;
        static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let p = std::env::temp_dir().join(format!("qi_store_test_{}.db", std::process::id()));
            std::env::set_var("QI_STORE_DB", &p);
            p
        })
        .clone()
    }

    /// kv 读写删回路（独立临时库，不碰运行中 App 的 store.db）
    #[test]
    fn kv_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let tmp = temp_db();
        let _ = std::fs::remove_file(&tmp); // 从干净状态开始
        init().expect("init store schema");
        let key = "test.kv_roundtrip";
        kv_set(&[(key, "v1")]).expect("kv_set");
        let all = kv_all().expect("kv_all");
        assert!(all.iter().any(|(k, v)| k == key && v == "v1"));
        // 覆盖写
        kv_set(&[(key, "v2")]).expect("kv_set overwrite");
        let all = kv_all().expect("kv_all");
        assert!(all.iter().any(|(k, v)| k == key && v == "v2"));
        assert!(!all.iter().any(|(k, v)| k == key && v == "v1"));
        kv_del(key).expect("kv_del");
        let all = kv_all().expect("kv_all");
        assert!(!all.iter().any(|(k, _)| k == key));
    }

    /// S3 站点 CRUD 回路（secret 走主口令加解密；独立临时库不碰真实 store.db）
    #[test]
    fn s3_site_roundtrip() {
        let _g = DB_LOCK.lock().unwrap();
        let tmp = temp_db();
        let _ = std::fs::remove_file(&tmp);
        init().expect("init store schema");
        // 敏感列走主口令加密：测试库需先设置测试主口令（解锁派生密钥）
        crate::core::master::setup("test-pass-123").expect("master setup");
        let name = format!("test-s3-{}", std::process::id());
        let site = S3Site {
            id: 0,
            name: name.clone(),
            endpoint: "https://s3.example.com".into(),
            region: "us-east-1".into(),
            bucket: "demo".into(),
            access_key: "AKID".into(),
            secret: "topsecret".into(),
            path_style: true,
        };
        s3_upsert(&site).expect("s3_upsert insert");
        let list = s3_list().expect("s3_list");
        let found = list.iter().find(|s| s.name == name).expect("site found");
        assert_eq!(found.bucket, "demo");
        assert_eq!(found.access_key, "AKID");
        // secret 经主口令加密落库、读出解密一致
        assert_eq!(found.secret, "topsecret");
        assert!(found.path_style);

        // 更新（拿到 id 后 upsert 改 bucket）
        let mut updated = site_with_id(found.id, &name);
        updated.bucket = "demo2".into();
        s3_upsert(&updated).expect("s3_upsert update");
        let list = s3_list().expect("s3_list");
        let found = list.iter().find(|s| s.name == name).expect("site found");
        assert_eq!(found.bucket, "demo2");
        // 不应出现重复行
        assert_eq!(list.iter().filter(|s| s.name == name).count(), 1);

        s3_del(found.id).expect("s3_del");
        let list = s3_list().expect("s3_list");
        assert!(!list.iter().any(|s| s.name == name));
    }

    fn site_with_id(id: i64, name: &str) -> S3Site {
        S3Site {
            id,
            name: name.to_string(),
            endpoint: "https://s3.example.com".into(),
            region: "us-east-1".into(),
            bucket: "demo".into(),
            access_key: "AKID".into(),
            secret: "topsecret".into(),
            path_style: true,
        }
    }
}
