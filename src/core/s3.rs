//! S3 协议层（业务层，不含任何 UI 代码）
//!
//! 基于 rusty-s3（SigV4 签名 + URL 构造）+ ureq（同步 HTTP，rustls）。
//! 面向「单桶浏览」场景提供：列前缀（目录式浏览）、上传、下载、删除、
//! 建目录（空标记对象）。全部为阻塞调用，UI 层经后台线程调用。

use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use url::Url;

/// S3 对象条目（浏览快照；目录 = 以 / 结尾的 CommonPrefix）
#[derive(Debug, Clone)]
pub struct S3Entry {
    /// 展示名（去掉前缀与结尾斜杠）
    pub name: String,
    /// 目录（CommonPrefix）
    pub is_dir: bool,
    /// 字节数（目录为 0）
    pub size: u64,
    /// 修改时间 Unix 秒（目录/未返回时为 0，UI 不显示）
    pub mtime: i64,
}

/// S3 连接参数（来自 store::S3Site 解密后的明文视图）
#[derive(Debug, Clone)]
pub struct S3Cred {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret: String,
    pub path_style: bool,
}

/// 错误统一文案化
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// 由凭据构造 rusty-s3 Bucket
fn bucket(c: &S3Cred) -> Result<Bucket, String> {
    let url = if c.endpoint.contains("://") {
        Url::parse(&c.endpoint).map_err(err)?
    } else {
        Url::parse(&format!("https://{}", c.endpoint)).map_err(err)?
    };
    let style = if c.path_style {
        UrlStyle::Path
    } else {
        UrlStyle::VirtualHost
    };
    // rusty-s3 Bucket::new 参数顺序：endpoint, path_style, 桶名, region
    Bucket::new(url, style, c.bucket.clone(), c.region.clone()).map_err(err)
}

fn credentials(c: &S3Cred) -> Credentials {
    Credentials::new(c.access_key.clone(), c.secret.clone())
}

/// 生成对象下载的预签名 GET URL（有效期 1 小时，过期自动失效）。
/// 纯本地签名计算，不发网络请求；复制给他人/浏览器即可直接下载。
pub fn presign_get(c: &S3Cred, key: &str) -> Result<String, String> {
    if key.is_empty() {
        return Err(crate::lang::S3_PRESIGN_NO_KEY().to_string());
    }
    let b = bucket(c)?;
    let creds = credentials(c);
    let action = b.get_object(Some(&creds), key);
    let url = action.sign(std::time::Duration::from_secs(3600));
    Ok(url.to_string())
}

/// 列出指定前缀下的对象与子前缀（一层；delimiter=/ 目录式浏览）。
/// `prefix` 为空 = 根目录。返回按「目录在前、名称升序」排序的条目。
pub fn list(c: &S3Cred, prefix: &str) -> Result<Vec<S3Entry>, String> {
    let b = bucket(c)?;
    let creds = credentials(c);
    // ListObjectsV2：delimiter 分层，一次拉满 1000 条（浏览场景够用；超过提示用前缀导航）
    let action = b.list_objects_v2(Some(&creds));
    let mut url = action.sign(std::time::Duration::from_secs(600));
    // rusty-s3 0.4 的 list_objects_v2 默认不带 delimiter，手工补查询参数
    url.query_pairs_mut()
        .append_pair("delimiter", "/")
        .append_pair("prefix", prefix)
        .append_pair("max-keys", "1000");

    let resp = ureq::get(url.as_str())
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("list: {}", e))?;
    let body = resp.into_string().map_err(err)?;
    parse_list_xml(&body)
}

/// 解析 ListObjectsV2 XML（手拉解析，避免引入 xml crate 依赖树）
fn parse_list_xml(xml: &str) -> Result<Vec<S3Entry>, String> {
    let mut out: Vec<S3Entry> = Vec::new();
    // 子前缀（目录）
    for seg in slice_all(xml, "<CommonPrefixes>", "</CommonPrefixes>") {
        if let Some(p) = slice(&seg, "<Prefix>", "</Prefix>") {
            let name = p.trim_end_matches('/').to_string();
            let name = name.rsplit('/').next().unwrap_or(&name).to_string();
            if !name.is_empty() {
                out.push(S3Entry {
                    name,
                    is_dir: true,
                    size: 0,
                    mtime: 0,
                });
            }
        }
    }
    // 对象（跳过目录标记对象自身）
    for seg in slice_all(xml, "<Contents>", "</Contents>") {
        let key = slice(&seg, "<Key>", "</Key>").unwrap_or_default();
        let size: u64 = slice(&seg, "<Size>", "</Size>")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        // LastModified（ISO8601 UTC，如 2024-01-02T03:04:05.000Z）→ Unix 秒
        let mtime = slice(&seg, "<LastModified>", "</LastModified>")
            .and_then(parse_iso8601_secs)
            .unwrap_or(0);
        if key.ends_with('/') && size == 0 {
            continue; // 目录标记对象不作为文件显示
        }
        let name = key.rsplit('/').next().unwrap_or(key).to_string();
        if !name.is_empty() {
            out.push(S3Entry {
                name,
                is_dir: false,
                size,
                mtime,
            });
        }
    }
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    Ok(out)
}

/// 取 XML 中所有 `<tag>..</tag>` 片段
fn slice_all(xml: &str, open: &str, close: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len()..];
        if let Some(j) = after.find(close) {
            out.push(after[..j].to_string());
            rest = &after[j + close.len()..];
        } else {
            break;
        }
    }
    out
}

/// 取单个 `<tag>..</tag>` 内容
fn slice<'a>(xml: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let i = xml.find(open)? + open.len();
    let rest = &xml[i..];
    let j = rest.find(close)?;
    Some(&rest[..j])
}

/// ISO8601 UTC 时间（S3 LastModified，如 `2024-01-02T03:04:05.000Z`）→ Unix 秒。
/// 手拉解析避免引入时间 crate；解析失败返回 None。
fn parse_iso8601_secs(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // 民用日期 → Unix 天数（Howard Hinnant days_from_civil）
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec)
}

/// 完整对象键（prefix + name；目录名补结尾斜杠）
pub fn join_key(prefix: &str, name: &str, is_dir: bool) -> String {
    let base = prefix.trim_end_matches('/');
    let mut k = if base.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", base, name)
    };
    if is_dir {
        k.push('/');
    }
    k
}

/// 上传本地文件（PUT；超过 64MB 自动 Multipart，5MB 分片）。
/// Multipart 支持断点续传：进度记录在本地文件旁的 `.qtbpart` 状态文件，
/// 失败后重传同一文件时从已完成的分片继续，成功后自动清除状态文件。
pub fn upload(c: &S3Cred, key: &str, local: &std::path::Path) -> Result<(), String> {
    let data = std::fs::read(local).map_err(err)?;
    if data.len() > 64 * 1024 * 1024 {
        upload_multipart(c, key, &data, local)
    } else {
        let b = bucket(c)?;
        let creds = credentials(c);
        let action = b.put_object(Some(&creds), key);
        let url = action.sign(std::time::Duration::from_secs(600));
        let resp = ureq::put(url.as_str())
            .timeout(std::time::Duration::from_secs(600))
            .send_bytes(&data)
            .map_err(|e| format!("upload: {}", e))?;
        if resp.status() < 300 {
            Ok(())
        } else {
            Err(format!("upload: HTTP {}", resp.status()))
        }
    }
}

/// Multipart 上传：initiate → 逐片 PUT → complete。
/// 断点续传：`state_path` 记录 upload_id 与已完成分片的 ETag，
/// 重传同一文件时跳过已完成分片；complete 成功后删除状态文件。
fn upload_multipart(
    c: &S3Cred,
    key: &str,
    data: &[u8],
    state_path: &std::path::Path,
) -> Result<(), String> {
    let b = bucket(c)?;
    let creds = credentials(c);

    // 断点续传：读取上次残留的 upload_id 与已传分片 ETag（文件损坏则忽略）
    let mut upload_id: Option<String> = None;
    let mut done_etags: Vec<String> = Vec::new();
    if let Ok(s) = std::fs::read_to_string(state_path) {
        let mut lines = s.lines();
        if let Some(id) = lines.next() {
            if !id.trim().is_empty() {
                upload_id = Some(id.trim().to_string());
                done_etags = lines.map(|l| l.trim().to_string()).collect();
            }
        }
    }

    // 无残留进度则 initiate 新上传
    if upload_id.is_none() {
        let create = b.create_multipart_upload(Some(&creds), key);
        let url = create.sign(std::time::Duration::from_secs(600));
        let resp = ureq::post(url.as_str())
            .timeout(std::time::Duration::from_secs(60))
            .call()
            .map_err(|e| format!("multipart init: {}", e))?;
        let body = resp.into_string().map_err(err)?;
        upload_id = Some(
            slice(&body, "<UploadId>", "</UploadId>")
                .ok_or_else(|| "multipart init: no UploadId".to_string())?
                .to_string(),
        );
    }
    let upload_id = upload_id.unwrap();

    // 逐片上传（失败保留进度文件，下次续传）
    let upload = match upload_parts(c, key, &upload_id, data, &done_etags, state_path) {
        Ok(parts) => parts,
        Err(e) => {
            let _ = abort_multipart(c, key, &upload_id);
            let _ = std::fs::remove_file(state_path);
            return Err(e);
        }
    };

    // complete（etags 经闭包生成，无需手工拼 XML）
    let etags_iter = upload.iter().map(|s| s.as_str());
    let complete = b.complete_multipart_upload(Some(&creds), key, &upload_id, etags_iter);
    let url = complete.sign(std::time::Duration::from_secs(600));
    let resp = ureq::post(url.as_str())
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("multipart complete: {}", e))?;
    if resp.status() < 300 {
        let _ = std::fs::remove_file(state_path);
        Ok(())
    } else {
        let _ = abort_multipart(c, key, &upload_id);
        let _ = std::fs::remove_file(state_path);
        Err(format!("multipart complete: HTTP {}", resp.status()))
    }
}

/// 逐片 PUT，返回各片 ETag。
/// `done` 为上次已成功分片的 ETag（按分片顺序），直接复用并跳过；
/// 每新完成一片就把 ETag 追加写入 `state_path`（写崩最多重传当前片）。
fn upload_parts(
    c: &S3Cred,
    key: &str,
    upload_id: &str,
    data: &[u8],
    done: &[String],
    state_path: &std::path::Path,
) -> Result<Vec<String>, String> {
    let b = bucket(c)?;
    let creds = credentials(c);
    const PART: usize = 5 * 1024 * 1024;
    let mut etags: Vec<String> = done.to_vec();
    let mut offset = etags.len() * PART;
    let mut num = etags.len() as u16 + 1;
    // 残留进度文件按行重写（保留已完成片），后续逐片追加
    if !done.is_empty() {
        let _ = std::fs::write(
            state_path,
            format!("{}\n{}", upload_id, done.join("\n")),
        );
    }
    while offset < data.len() {
        let end = (offset + PART).min(data.len());
        let chunk = &data[offset..end];
        let action = b.upload_part(Some(&creds), key, num, upload_id);
        let url = action.sign(std::time::Duration::from_secs(600));
        let resp = ureq::put(url.as_str())
            .timeout(std::time::Duration::from_secs(600))
            .send_bytes(chunk)
            .map_err(|e| format!("part {}: {}", num, e))?;
        if resp.status() >= 300 {
            return Err(format!("part {}: HTTP {}", num, resp.status()));
        }
        let etag = resp
            .header("ETag")
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        etags.push(etag.clone());
        // 进度落盘：upload_id 首行 + 各片 ETag
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(state_path)
            .and_then(|mut f| {
                use std::io::Write;
                if done.is_empty() && etags.len() == 1 {
                    writeln!(f, "{}", upload_id)?;
                }
                writeln!(f, "{}", etag)
            });
        offset = end;
        num += 1;
    }
    Ok(etags)
}

/// 中止 Multipart（尽力而为）
fn abort_multipart(c: &S3Cred, key: &str, upload_id: &str) -> Result<(), String> {
    let b = bucket(c)?;
    let creds = credentials(c);
    let action = b.abort_multipart_upload(Some(&creds), key, upload_id);
    let url = action.sign(std::time::Duration::from_secs(600));
    let _ = ureq::delete(url.as_str()).call();
    Ok(())
}

/// 下载对象到本地目录（保持对象名；返回写入的完整路径）。
/// 支持断点续传：本地已有同名且小于远端大小的文件时，经 Range 从断点续写。
pub fn download(c: &S3Cred, key: &str, local_dir: &str) -> Result<String, String> {
    let b = bucket(c)?;
    let creds = credentials(c);
    let action = b.get_object(Some(&creds), key);
    let url = action.sign(std::time::Duration::from_secs(600));
    let name = key.rsplit('/').next().unwrap_or(key);
    let dest = std::path::Path::new(local_dir).join(name);

    // 先探测远端大小（HEAD），决定续传偏移
    let head_url = b.head_object(Some(&creds), key).sign(std::time::Duration::from_secs(600));
    let remote_len = match ureq::head(head_url.as_str())
        .timeout(std::time::Duration::from_secs(60))
        .call()
    {
        Ok(resp) if resp.status() < 300 => resp
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0),
        _ => 0, // HEAD 失败则退化为整体 GET
    };

    // 断点续传：本地已有部分文件（小于远端大小）则从其大小处续写
    let resume_from = match std::fs::metadata(&dest) {
        Ok(m) if remote_len > 0 && m.len() < remote_len => m.len(),
        Ok(m) if remote_len > 0 && m.len() == remote_len => return Ok(dest.to_string_lossy().into_owned()),
        _ => 0,
    };

    let mut req = ureq::get(url.as_str()).timeout(std::time::Duration::from_secs(600));
    if resume_from > 0 {
        req = req.set("Range", &format!("bytes={}-", resume_from));
    }
    let resp = req.call().map_err(|e| format!("download: {}", e))?;
    if resp.status() >= 300 {
        return Err(format!("download: HTTP {}", resp.status()));
    }
    let mut file = if resume_from > 0 {
        // 服务器支持 Range 则 206 追加续写；否则 200 整体重建
        if resp.status() == 206 {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&dest)
                .map_err(err)?
        } else {
            std::fs::File::create(&dest).map_err(err)?
        }
    } else {
        std::fs::File::create(&dest).map_err(err)?
    };
    let mut reader = resp.into_reader();
    std::io::copy(&mut reader, &mut file).map_err(err)?;
    Ok(dest.to_string_lossy().into_owned())
}

/// 删除对象（单个；目录标记对象同样走 DeleteObject）
pub fn delete(c: &S3Cred, key: &str) -> Result<(), String> {
    let b = bucket(c)?;
    let creds = credentials(c);
    let action = b.delete_object(Some(&creds), key);
    let url = action.sign(std::time::Duration::from_secs(600));
    let resp = ureq::delete(url.as_str())
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .map_err(|e| format!("delete: {}", e))?;
    if resp.status() < 300 {
        Ok(())
    } else {
        Err(format!("delete: HTTP {}", resp.status()))
    }
}

/// 删除「目录」：列出前缀下全部对象并逐个删除（S3 无真实目录，须逐键删除）。
/// 返回删除的对象数。
pub fn delete_prefix(c: &S3Cred, prefix: &str) -> Result<usize, String> {
    let b = bucket(c)?;
    let creds = credentials(c);
    let action = b.list_objects_v2(Some(&creds));
    let mut url = action.sign(std::time::Duration::from_secs(600));
    url.query_pairs_mut()
        .append_pair("prefix", prefix)
        .append_pair("max-keys", "1000");
    let resp = ureq::get(url.as_str())
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("list: {}", e))?;
    let body = resp.into_string().map_err(err)?;
    let mut n = 0usize;
    for seg in slice_all(&body, "<Contents>", "</Contents>") {
        if let Some(key) = slice(&seg, "<Key>", "</Key>") {
            delete(c, key)?;
            n += 1;
        }
    }
    Ok(n)
}

/// 新建「目录」：写入 0 字节目录标记对象（key 以 / 结尾）
pub fn make_dir(c: &S3Cred, prefix: &str, name: &str) -> Result<(), String> {
    let key = join_key(prefix, name, true);
    let b = bucket(c)?;
    let creds = credentials(c);
    let action = b.put_object(Some(&creds), &key);
    let url = action.sign(std::time::Duration::from_secs(600));
    let resp = ureq::put(url.as_str())
        .timeout(std::time::Duration::from_secs(60))
        .send_bytes(&[])
        .map_err(|e| format!("mkdir: {}", e))?;
    if resp.status() < 300 {
        Ok(())
    } else {
        Err(format!("mkdir: HTTP {}", resp.status()))
    }
}

/// 父前缀（"a/b/" → "a/"；根的父还是根）
pub fn parent_prefix(prefix: &str) -> String {
    let base = prefix.trim_end_matches('/');
    match base.rfind('/') {
        Some(i) => format!("{}/", &base[..i]),
        None => String::new(),
    }
}

// ────────────────────── 后台工作线程（UI 协议） ──────────────────────

use crate::core::MsgSink;
use std::sync::mpsc;

/// UI → 后台命令
pub enum S3Cmd {
    /// 列前缀（force 预留；当前始终直连服务器）
    List { cred: S3Cred, prefix: String },
    /// 上传本地文件到 key
    Upload {
        cred: S3Cred,
        key: String,
        local: String,
    },
    /// 下载 key 到本地目录
    Download {
        cred: S3Cred,
        key: String,
        local_dir: String,
    },
    /// 删除单个对象
    Delete { cred: S3Cred, key: String },
    /// 删除目录（逐键删除前缀下全部对象）
    DeleteDir { cred: S3Cred, prefix: String },
    /// 新建目录（0 字节标记对象）
    Mkdir {
        cred: S3Cred,
        prefix: String,
        name: String,
    },
}

/// 后台 → UI 消息
pub enum S3Msg {
    /// 列表结果：Ok((prefix, 条目)) 或 Err
    Listed(Result<(String, Vec<S3Entry>), String>),
    /// 操作结果（上传/下载/删除/建目录）
    Done(Result<String, String>),
}

/// 启动 S3 工作线程：UI 经 mpsc 发命令，阻塞操作在后台执行，结果经 sink 回 UI。
pub fn spawn_worker(sink: MsgSink<S3Msg>) -> mpsc::Sender<S3Cmd> {
    let (tx, rx) = mpsc::channel::<S3Cmd>();
    std::thread::spawn(move || {
        // MsgSink 非 Sync：包成 Arc<Mutex> 共享给每条命令的执行线程（emit 时取锁）
        let sink = std::sync::Arc::new(std::sync::Mutex::new(sink));
        while let Ok(cmd) = rx.recv() {
            // 每条命令独立线程执行（阻塞 HTTP，避免排队命令互相拖延列表刷新）
            let fwd = sink.clone();
            std::thread::spawn(move || {
                let msg = match cmd {
                    S3Cmd::List { cred, prefix } => match list(&cred, &prefix) {
                        Ok(entries) => S3Msg::Listed(Ok((prefix, entries))),
                        Err(e) => S3Msg::Listed(Err(e)),
                    },
                    S3Cmd::Upload { cred, key, local } => S3Msg::Done(
                        upload(&cred, &key, std::path::Path::new(&local))
                            .map(|_| crate::lang::S3_UPLOADED(&key)),
                    ),
                    S3Cmd::Download {
                        cred,
                        key,
                        local_dir,
                    } => S3Msg::Done(
                        download(&cred, &key, &local_dir).map(|p| crate::lang::S3_DOWNLOADED(&p)),
                    ),
                    S3Cmd::Delete { cred, key } => {
                        S3Msg::Done(delete(&cred, &key).map(|_| crate::lang::S3_DELETED(&key)))
                    }
                    S3Cmd::DeleteDir { cred, prefix } => {
                        S3Msg::Done(delete_prefix(&cred, &prefix).map(crate::lang::S3_DIR_DELETED))
                    }
                    S3Cmd::Mkdir { cred, prefix, name } => S3Msg::Done(
                        make_dir(&cred, &prefix, &name).map(|_| crate::lang::S3_DIR_MADE(&name)),
                    ),
                };
                if let Ok(s) = fwd.lock() {
                    s(msg);
                }
            });
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
    <Name>demo</Name>
    <Prefix>docs/</Prefix>
    <Delimiter>/</Delimiter>
    <Contents><Key>docs/a.txt</Key><Size>1024</Size></Contents>
    <Contents><Key>docs/sub/</Key><Size>0</Size></Contents>
    <Contents><Key>docs/b.log</Key><Size>204800</Size></Contents>
    <CommonPrefixes><Prefix>docs/images/</Prefix></CommonPrefixes>
    <CommonPrefixes><Prefix>docs/notes/</Prefix></CommonPrefixes>
</ListBucketResult>"#;

    #[test]
    fn parse_list_dirs_and_files() {
        let entries = parse_list_xml(LIST_XML).unwrap();
        let dirs: Vec<&str> = entries
            .iter()
            .filter(|e| e.is_dir)
            .map(|e| e.name.as_str())
            .collect();
        let files: Vec<&str> = entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.name.as_str())
            .collect();
        // CommonPrefixes 解析为目录，且去掉前缀与结尾斜杠
        assert_eq!(dirs, vec!["images", "notes"]);
        // 对象解析为文件，目录标记对象（size=0 且 key 以 / 结尾）被跳过
        assert_eq!(files, vec!["a.txt", "b.log"]);
        // 大小正确
        let a = entries.iter().find(|e| e.name == "a.txt").unwrap();
        assert_eq!(a.size, 1024);
    }

    #[test]
    fn parse_list_sorted_dirs_first() {
        let entries = parse_list_xml(LIST_XML).unwrap();
        // 目录在前、文件按名称升序
        assert!(entries[0].is_dir);
        let file_pos = entries.iter().position(|e| !e.is_dir).unwrap();
        assert!(entries[..file_pos].iter().all(|e| e.is_dir));
        let mut names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        let (_, files_sorted) = names.split_at(file_pos);
        let mut expect = files_sorted.to_vec();
        expect.sort();
        assert_eq!(files_sorted, expect.as_slice());
        names.clear();
    }

    #[test]
    fn parse_list_empty_and_malformed() {
        assert!(parse_list_xml("").unwrap().is_empty());
        assert!(parse_list_xml("<not-s3/>").unwrap().is_empty());
        // 只有目录标记对象 → 空
        let xml = "<R><Contents><Key>x/</Key><Size>0</Size></Contents></R>";
        assert!(parse_list_xml(xml).unwrap().is_empty());
    }

    #[test]
    fn slice_extracts_first_occurrence() {
        let x = slice("<A>1</A><A>2</A>", "<A>", "</A>");
        assert_eq!(x, Some("1"));
        assert_eq!(slice("nothing", "<A>", "</A>"), None);
    }

    #[test]
    fn slice_all_extracts_every_occurrence() {
        let v = slice_all("<A>1</A>b<A>2</A>", "<A>", "</A>");
        assert_eq!(v, vec!["1", "2"]);
        assert!(slice_all("zzz", "<A>", "</A>").is_empty());
    }

    #[test]
    fn join_key_combinations() {
        // 根前缀 + 文件
        assert_eq!(join_key("", "a.txt", false), "a.txt");
        // 根前缀 + 目录
        assert_eq!(join_key("", "sub", true), "sub/");
        // 子前缀 + 文件（前缀结尾斜杠容忍）
        assert_eq!(join_key("docs/", "a.txt", false), "docs/a.txt");
        assert_eq!(join_key("docs", "a.txt", false), "docs/a.txt");
        // 子前缀 + 目录不双斜杠
        assert_eq!(join_key("docs/", "sub", true), "docs/sub/");
    }

    #[test]
    fn parent_prefix_steps_up() {
        assert_eq!(parent_prefix(""), "");
        assert_eq!(parent_prefix("docs/"), "");
        assert_eq!(parent_prefix("docs/images/"), "docs/");
        assert_eq!(parent_prefix("docs/images/pic/"), "docs/images/");
        // 无结尾斜杠的宽容处理
        assert_eq!(parent_prefix("docs"), "");
    }
}
