//! store::misc —— TOTP 密钥 / 保存的密码 / 已存网址 / 备忘录

use super::{now_secs, protect, run, unprotect};
use crate::core::password;
use crate::core::turso::value_to_string;

/// TOTP 密钥条目（多密钥管理）
#[derive(Debug, Clone)]
pub struct TotpKey {
    pub id: i64,
    pub name: String,
    pub secret: String,
    pub algo: i64, // 0=SHA1 1=SHA256 2=SHA512
}

// ────────────────────── TOTP 密钥 / 保存的密码 / 已存网址 / 备忘录 ──────────────────────


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
            let mut out: Vec<SavedPassword> = Vec::new();
            let mut legacy: Vec<SavedPassword> = Vec::new();
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
                    // 明文（非哈希格式的旧条目，如生成的随机密码）排到列表末尾
                    if password::detect_hash_algorithm(&password).is_some() {
                        out.push(SavedPassword {
                            id,
                            used_for,
                            password,
                        });
                    } else {
                        legacy.push(SavedPassword {
                            id,
                            used_for,
                            password,
                        });
                    }
                }
            }
            out.append(&mut legacy);
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

// ── 运维备忘（memos）──

/// 备忘条目（纯文本；`day` 为关联日期 "YYYY-MM-DD"，空 = 不关联）
/// （`updated` 时间戳列由 SQL `ORDER BY updated DESC` 排序使用，不映射到本结构体）
#[derive(Debug, Clone)]
pub struct Memo {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub day: String,
}

/// 列出全部备忘（新在前）
pub fn memo_list() -> Result<Vec<Memo>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, title, content, day FROM memos ORDER BY updated DESC, id DESC",
                    (),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(Memo {
                    id: row.get(0).map_err(|e| e.to_string())?,
                    title: value_to_string(&row, 1),
                    content: value_to_string(&row, 2),
                    day: value_to_string(&row, 3),
                });
            }
            Ok(out)
        })
    }))
}

/// 新增备忘，返回自增 id
pub fn memo_add(title: &str, content: &str, day: &str) -> Result<i64, String> {
    let (title, content, day) = (
        title.to_string(),
        content.to_string(),
        day.to_string(),
    );
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT INTO memos(title, content, day, updated) VALUES(?1, ?2, ?3, ?4)",
                (
                    title.as_str(),
                    content.as_str(),
                    day.as_str(),
                    now_secs(),
                ),
            )
            .await
            .map_err(|e| e.to_string())?;
            let mut rows = conn
                .query("SELECT last_insert_rowid()", ())
                .await
                .map_err(|e| e.to_string())?;
            match rows.next().await.map_err(|e| e.to_string())? {
                Some(row) => row.get(0).map_err(|e| e.to_string()),
                None => Err("memo_add: no rowid".to_string()),
            }
        })
    }))
}

/// 更新备忘（按 id 整体覆盖标题/内容/日期）
pub fn memo_update(id: i64, title: &str, content: &str, day: &str) -> Result<(), String> {
    let (title, content, day) = (title.to_string(), content.to_string(), day.to_string());
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "UPDATE memos SET title = ?1, content = ?2, day = ?3, updated = ?4 WHERE id = ?5",
                (
                    title.as_str(),
                    content.as_str(),
                    day.as_str(),
                    now_secs(),
                    id,
                ),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 删除备忘
pub fn memo_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM memos WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}
