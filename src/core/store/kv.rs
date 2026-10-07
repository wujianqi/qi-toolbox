//! store::kv —— 键值对（设置 / 记忆输入）

use super::run;
use crate::core::turso::value_to_string;

// ────────────────────── kv（设置 / 记忆输入） ──────────────────────

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
