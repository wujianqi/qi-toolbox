//! store::ssh —— SSH 常用命令与主机指纹（TOFU）

use super::run;
use crate::core::turso::value_to_string;

/// SSH 常用命令条目（id/name 仅供库内排序，UI 只消费分组与命令文本）
#[derive(Debug, Clone)]
pub struct SshCmd {
    pub category: String,
    pub command: String,
}

// ────────────────────── SSH 常用命令 / 主机指纹（TOFU） ──────────────────────

/// 读取主机指纹（无记录 = 首连，返回 None）
pub fn host_fp_get(host: &str, port: u16) -> Result<Option<String>, String> {
    let (host, port) = (host.to_string(), port as i64);
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            let mut rows = conn
                .query(
                    "SELECT fingerprint FROM ssh_hosts WHERE host = ?1 AND port = ?2",
                    (host.as_str(), port),
                )
                .await
                .map_err(|e| e.to_string())?;
            match rows.next().await.map_err(|e| e.to_string())? {
                Some(row) => Ok(Some(value_to_string(&row, 0))),
                None => Ok(None),
            }
        })
    }))
}

/// 写入/更新主机指纹（首连记录或用户确认后重置）
pub fn host_fp_set(host: &str, port: u16, fingerprint: &str) -> Result<(), String> {
    let (host, fp) = (host.to_string(), fingerprint.to_string());
    let port = port as i64;
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            conn.execute(
                "INSERT INTO ssh_hosts(host, port, fingerprint) VALUES(?1, ?2, ?3)
                 ON CONFLICT(host, port) DO UPDATE SET fingerprint = ?3",
                (host.as_str(), port, fp.as_str()),
            )
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
