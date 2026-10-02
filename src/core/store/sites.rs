//! store::sites —— 连接站点管理（SFTP / S3 / MySQL / PG / Turso / 目录书签）

use super::{protect, run, unprotect};
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
    /// 私钥文件路径（空 = 密码认证）；认证时 pass 作为私钥口令复用
    pub key_path: String,
}

// ────────────────────── 连接站点（SFTP / S3 / MySQL / PG / Turso / 书签） ──────────────────────

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
    // 老库无 key_path 列（迁移前/查询未带该列）时回落空串
    let key_path = value_to_string(row, 6);
    Ok(SftpSite {
        id,
        name,
        host,
        port: port.clamp(1, 65535) as u16,
        user,
        pass,
        key_path,
    })
}

/// 列出全部站点（按名称排序；密码已解密）
pub fn sftp_list() -> Result<Vec<SftpSite>, String> {
    run(Box::new(|conn: turso::Connection| {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut rows = conn
                .query(
                    "SELECT id, name, host, port, user, pass_enc, key_path FROM sftp_sites ORDER BY name",
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
    let (name, host, user, key_path) = (
        site.name.clone(),
        site.host.clone(),
        site.user.clone(),
        site.key_path.clone(),
    );
    let port = site.port as i64;
    let id = site.id;
    run(Box::new(move |conn: turso::Connection| {
        Box::pin(async move {
            if id <= 0 {
                conn.execute(
                    "INSERT INTO sftp_sites(name, host, port, user, pass_enc, key_path) VALUES(?1,?2,?3,?4,?5,?6)",
                    (name.as_str(), host.as_str(), port, user.as_str(), enc.as_str(), key_path.as_str()),
                )
                .await.map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE sftp_sites SET name=?1, host=?2, port=?3, user=?4, pass_enc=?5, key_path=?6 WHERE id=?7",
                    (name.as_str(), host.as_str(), port, user.as_str(), enc.as_str(), key_path.as_str(), id),
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

// ────────────────────── 保存的 SQL（按连接源区分）──────────────────────

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

