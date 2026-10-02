//! store::sql —— 保存的 SQL 与执行历史

use super::{now_secs, run};
use crate::core::turso::value_to_string;

// ────────────────────── 保存的 SQL / SQL 执行历史 ──────────────────────

/// 保存的 SQL 条目
pub struct SavedSql {
    pub id: i64,
    pub name: String,
    pub sql: String,
}

/// 列出某连接源保存的 SQL（新→旧）
pub fn saved_sql_list(source: &str) -> Result<Vec<SavedSql>, String> {
    let source = source.to_string();
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, name, sql FROM saved_sql WHERE source = ?1 ORDER BY id DESC LIMIT 100",
                    (source.as_str(),),
                )
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                out.push(SavedSql {
                    id: row.get(0).map_err(|e| e.to_string())?,
                    name: value_to_string(&row, 1),
                    sql: value_to_string(&row, 2),
                });
            }
            Ok(out)
        })
    }))
}

/// 新增保存的 SQL
pub fn saved_sql_add(source: &str, name: &str, sql: &str) -> Result<(), String> {
    let (source, name, sql) = (source.to_string(), name.to_string(), sql.to_string());
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT INTO saved_sql(source, name, sql) VALUES(?1, ?2, ?3)",
                (source.as_str(), name.as_str(), sql.as_str()),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

/// 删除保存的 SQL
pub fn saved_sql_del(id: i64) -> Result<(), String> {
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute("DELETE FROM saved_sql WHERE id = ?1", (id,))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }))
}

// ────────────────────── SSH 主机指纹（known_hosts，TOFU）──────────────────────


/// 历史保留条数（每类连接源各留最近 N 条）
pub(crate) const SQL_HISTORY_KEEP: usize = 50;

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
