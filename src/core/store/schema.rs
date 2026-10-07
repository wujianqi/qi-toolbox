//! store::schema —— 建表、版本迁移、主口令校验凭据

use super::run;
use crate::core::turso::value_to_string;

// ────────────────────── Schema / 迁移 / 主口令凭据 ──────────────────────

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
    pass_enc TEXT NOT NULL DEFAULT '',
    key_path TEXT NOT NULL DEFAULT ''
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
    kind INTEGER NOT NULL DEFAULT 0,
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
CREATE TABLE IF NOT EXISTS saved_sql (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    name TEXT NOT NULL,
    sql TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ssh_hosts (
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY (host, port)
);
CREATE TABLE IF NOT EXISTS memos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    day TEXT NOT NULL DEFAULT '',
    updated INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS schema_version (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL
);
";

/// 当前 schema 版本（每次引入需迁移的结构变更时 +1，并在 [`MIGRATIONS`] 对应
/// 下标处补迁移步骤；全新库直接建最新 SCHEMA 后写入本版本号）
const SCHEMA_VERSION: i64 = 3;

/// 顺序迁移步骤：`MIGRATIONS[v]` 把库从版本 v 升到 v+1（下标 0 = 版本 0，即
/// 早期无 schema_version 表的存量库）。幂等性由各步骤自行保证（失败可重试）。
const MIGRATIONS: &[&str] = &[
    // v0 → v1：把早期 pg_sites 明文 url 列改名为 url_enc（全新库无此列，报错属预期）
    "ALTER TABLE pg_sites RENAME COLUMN url TO url_enc",
    // v1 → v2：sftp_sites 支持 SSH Key 认证（私钥路径；口令复用 pass_enc）
    "ALTER TABLE sftp_sites ADD COLUMN key_path TEXT NOT NULL DEFAULT ''",
    // v2 → v3：turso_dbs 支持本地文件/网络两种类型（v0.2.1 引入 kind 列时漏了
    // 建表与迁移，导致所有库该表均缺列、turso 站点保存必失败）
    "ALTER TABLE turso_dbs ADD COLUMN kind INTEGER NOT NULL DEFAULT 0",
];

/// 建 schema 并按版本顺序迁移（应用启动时调用一次；失败返回错误文案，
/// 由调用方决定是否提示）。
///
/// 流程：建表（IF NOT EXISTS，对老库无副作用）→ 读 `schema_version`
/// （无记录 = 全新库或早期无版本表的存量库）→ 全新库（`kv` 无数据）直接
/// 写当前版本号；存量库从 0 开始逐版执行 [`MIGRATIONS`] 升到当前版本。
pub fn init() -> Result<(), String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            conn.execute_batch(SCHEMA)
                .await
                .map_err(|e| format!("init store: {}", e))?;

            // 读当前版本（无 schema_version 记录 = 0）
            let mut rows = conn
                .query("SELECT version FROM schema_version WHERE id = 1", ())
                .await
                .map_err(|e| format!("read schema_version: {}", e))?;
            let ver: i64 = match rows.next().await.map_err(|e| e.to_string())? {
                Some(row) => row.get(0).map_err(|e| e.to_string())?,
                None => 0,
            };
            drop(rows);

            if ver == 0 {
                // 区分全新库与存量库：全新库 kv 为空，直接记当前版本；
                // 存量库（已有数据）从 0 逐版迁移
                let mut rows = conn
                    .query("SELECT COUNT(*) FROM kv", ())
                    .await
                    .map_err(|e| e.to_string())?;
                let fresh = match rows.next().await.map_err(|e| e.to_string())? {
                    Some(row) => {
                        let n: i64 = row.get(0).map_err(|e| e.to_string())?;
                        n == 0
                    }
                    None => true,
                };
                drop(rows);
                if fresh {
                    set_schema_version(&conn, SCHEMA_VERSION).await?;
                    return Ok(());
                }
            }
            // 逐版迁移到当前版本
            for v in ver..SCHEMA_VERSION {
                let idx = v as usize;
                if let Some(sql) = MIGRATIONS.get(idx) {
                    // 个别步骤对部分存量库不适用（如本就没有旧列）：报错属预期，
                    // 静默跳过，不阻塞升级
                    let _ = conn.execute(*sql, ()).await;
                }
                set_schema_version(&conn, v + 1).await?;
                crate::core::log::info("store", &format!("schema migrated to v{}", v + 1));
            }
            Ok(())
        })
    }))
}

/// 写版本号（upsert 单行）
async fn set_schema_version(conn: &turso::Connection, v: i64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO schema_version (id, version) VALUES (1, ?1) \
         ON CONFLICT(id) DO UPDATE SET version = ?1",
        (v,),
    )
    .await
    .map_err(|e| format!("set schema_version: {}", e))?;
    Ok(())
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
