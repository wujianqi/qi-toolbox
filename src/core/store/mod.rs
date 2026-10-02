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

mod kv;
mod misc;
mod schema;
mod sites;
mod sql;
mod ssh;

pub use kv::*;
pub use misc::*;
pub use schema::*;
pub use sites::*;
pub use sql::*;
pub use ssh::*;

#[cfg(test)]
mod tests;

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
        // PRAGMA wal_checkpoint 返回一行结果：turso 的 execute 遇到有返回行的
        // 语句会报 "unexpected row during execution"，须用 query 消费掉
        let mut rows = conn
            .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
            .await
            .map_err(|e| format!("checkpoint: {}", e))?;
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let _ = row;
        }
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
    match crate::core::master::unprotect(enc.as_bytes()) {
        Ok(plain) => Ok(String::from_utf8_lossy(&plain).into_owned()),
        Err(e) => {
            crate::core::log::warn("store", &format!("unprotect failed: {}", e));
            Err(e)
        }
    }
}

/// 当前 Unix 秒
/// 当前 Unix 秒
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ────────────────────── TOTP 密钥 ──────────────────────
