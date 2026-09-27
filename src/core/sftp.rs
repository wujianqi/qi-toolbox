//! SFTP 工作线程（业务层，不含任何 UI 代码）
//!
//! SSH/SFTP 会话在独立线程中存活（跨命令复用），UI 层经
//! [`SftpCmd`] 发命令、经 [`SftpMsg`]（投递口 [`MsgSink`]）收结果。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::client::Handle;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::core::MsgSink;
use crate::lang;

/// 文件列表条目（只读快照，供 UI 渲染）
#[derive(Clone, Debug)]
pub struct SftpEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// 后台 → UI 的消息
pub enum SftpMsg {
    /// 连接结果：Ok(当前目录) 或 Err(错误信息)
    Connected(Result<String, String>),
    /// 列目录结果：Ok((当前目录, 条目列表))
    Listed(Result<(String, Vec<SftpEntry>), String>),
    /// 操作结果：Ok(状态文案) 或 Err(错误信息)；成功后会再附带一次 Listed 刷新
    Done(Result<String, String>),
    /// SSH 命令执行结果：Ok(合并的 stdout+stderr 输出) 或 Err(错误信息)
    ExecDone(Result<String, String>),
}

/// UI → 后台的命令
pub enum SftpCmd {
    Connect {
        host: String,
        port: u16,
        user: String,
        pass: String,
    },
    Disconnect,
    /// 列目录：`force=true` 跳过缓存强制向服务器重拉（「刷新」按钮用）；
    /// 否则 15s 内命中内存缓存直接返回（上级/双击进入目录等导航用）
    List {
        path: String,
        force: bool,
    },
    Mkdir {
        path: String,
    },
    Delete {
        path: String,
        is_dir: bool,
    },
    Upload {
        local: String,
        remote: String,
    },
    Download {
        remote: String,
        local_dir: String,
    },
    /// 执行一条远程命令（复用现有 SSH 会话）
    Exec {
        command: String,
    },
}

/// SSH 握手 handler：信任任意服务器密钥（工具类应用，不校验指纹）。
struct SshHandler;

impl russh::client::Handler for SshHandler {
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        _data: &[u8],
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// 工作线程状态：SSH 句柄 + SFTP 会话 + 当前目录（保持连接跨命令存活）。
struct WorkerState {
    ssh: Option<Handle<SshHandler>>,
    sftp: Option<SftpSession>,
    cwd: String,
    /// Exec 中断标志（UI「停止」置位，exec 等待循环轮询；单个命令执行期间有效）
    cancel: Arc<AtomicBool>,
    /// 目录列表内存缓存：路径 →（缓存时刻, 条目）。15s 内重复浏览直接命中；
    /// 变更类操作（建/删/传/收、Exec 成功、重连）会清空，刷新按钮强制重拉
    dir_cache: HashMap<String, (Instant, Vec<SftpEntry>)>,
}

/// 启动 SFTP 工作线程，返回（命令发送端, Exec 中断标志）。线程随进程存活，UI 重建不影响。
pub fn spawn_worker(sink: MsgSink<SftpMsg>) -> (mpsc::Sender<SftpCmd>, Arc<AtomicBool>) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<SftpCmd>();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = cancel.clone();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                sink(SftpMsg::Done(Err(e.to_string())));
                return;
            }
        };
        let mut state = WorkerState {
            ssh: None,
            sftp: None,
            cwd: String::from("."),
            cancel: cancel_worker,
            dir_cache: HashMap::new(),
        };
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                SftpCmd::Connect {
                    host,
                    port,
                    user,
                    pass,
                } => {
                    let r = rt.block_on(connect(&host, port, &user, &pass));
                    match r {
                        Ok((ssh, sftp, cwd)) => {
                            state.ssh = Some(ssh);
                            state.sftp = Some(sftp);
                            state.cwd = cwd.clone();
                            // 新连接：上一会话遗留的目录缓存一律作废
                            state.dir_cache.clear();
                            sink(SftpMsg::Connected(Ok(cwd.clone())));
                            // 连接成功即列一次家目录
                            let listed = rt.block_on(list(&mut state, &cwd, false));
                            sink(SftpMsg::Listed(listed));
                        }
                        Err(e) => {
                            sink(SftpMsg::Connected(Err(e)));
                        }
                    }
                }
                SftpCmd::Disconnect => {
                    state.ssh = None;
                    state.sftp = None;
                    state.cwd = String::from(".");
                    state.dir_cache.clear();
                    sink(SftpMsg::Done(Ok(String::new())));
                }
                SftpCmd::List { path, force } => {
                    let r = rt.block_on(list(&mut state, &path, force));
                    if let Ok((cwd, _)) = &r {
                        state.cwd = cwd.clone();
                    }
                    sink(SftpMsg::Listed(r));
                }
                SftpCmd::Mkdir { path } => {
                    let r = rt.block_on(mkdir(&state, &path));
                    // 目录内容已变更：清缓存，finish_op 收尾时自动重拉最新列表
                    if r.is_ok() {
                        state.dir_cache.clear();
                    }
                    finish_op(
                        &rt,
                        &mut state,
                        &sink,
                        r,
                        lang::SFTP_DONE_MKDIR().to_string(),
                    );
                }
                SftpCmd::Delete { path, is_dir } => {
                    let r = rt.block_on(delete(&state, &path, is_dir));
                    if r.is_ok() {
                        state.dir_cache.clear();
                    }
                    finish_op(
                        &rt,
                        &mut state,
                        &sink,
                        r,
                        lang::SFTP_DONE_DELETE().to_string(),
                    );
                }
                SftpCmd::Upload { local, remote } => {
                    let r = rt.block_on(upload(&state, &local, &remote));
                    if r.is_ok() {
                        state.dir_cache.clear();
                    }
                    let ok_msg = lang::SFTP_DONE_UPLOAD(remote);
                    finish_op(&rt, &mut state, &sink, r, ok_msg);
                }
                SftpCmd::Download { remote, local_dir } => {
                    let r = rt.block_on(download(&state, &remote, &local_dir));
                    if r.is_ok() {
                        state.dir_cache.clear();
                    }
                    let ok_msg = lang::SFTP_DONE_DOWNLOAD(remote);
                    finish_op(&rt, &mut state, &sink, r, ok_msg);
                }
                SftpCmd::Exec { command } => {
                    // 每次执行前复位中断标志，避免上次「停止」的残留误伤本次命令
                    state.cancel.store(false, Ordering::Relaxed);
                    let r = rt.block_on(exec_command(&state, &command, &state.cancel));
                    state.cancel.store(false, Ordering::Relaxed);
                    if r.is_ok() {
                        // 命令可能创建/删改了服务器文件，目录缓存一并作废
                        state.dir_cache.clear();
                    }
                    sink(SftpMsg::ExecDone(r));
                }
            }
        }
    });
    (cmd_tx, cancel)
}

/// 命令成功后的公共收尾：回 Done + 自动刷新当前目录列表。
fn finish_op(
    rt: &tokio::runtime::Runtime,
    state: &mut WorkerState,
    sink: &MsgSink<SftpMsg>,
    r: Result<(), String>,
    ok_msg: String,
) {
    match r {
        Ok(()) => {
            sink(SftpMsg::Done(Ok(ok_msg)));
            // 变更已清空缓存，此处重列必然走服务器拉取最新列表
            let cwd = state.cwd.clone();
            let listed = rt.block_on(list(state, &cwd, false));
            sink(SftpMsg::Listed(listed));
        }
        Err(e) => {
            sink(SftpMsg::Done(Err(e)));
        }
    }
}

/// 未连接时统一错误。
fn need_conn() -> Result<(), String> {
    Err(lang::SFTP_NO_CONN().to_string())
}

/// 执行一条远程命令：新开 session 通道，exec 后收集 stdout+stderr 直至通道关闭。
/// 复用现有 SSH 会话（不需要额外认证），输出按到达顺序合并。
/// Exec 等待循环轮询中断标志的间隔：服务器卡死/命令挂起时最迟 ~100ms 可被停止
const EXEC_CANCEL_POLL: Duration = Duration::from_millis(100);

/// 执行一条远程命令：等待期间每 ~100ms 轮询 `cancel`，UI「停止」可强制中断
/// （best-effort 关闭会话通道后返回中断错误，worker 立即恢复处理后续命令）。
async fn exec_command(
    state: &WorkerState,
    command: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let Some(ssh) = &state.ssh else {
        return Err(lang::SFTP_NO_CONN().to_string());
    };
    let mut channel = ssh
        .channel_open_session()
        .await
        .map_err(|e| lang::SFTP_ERR_EXEC(e.to_string()))?;
    channel
        .exec(true, command.as_bytes())
        .await
        .map_err(|e| lang::SFTP_ERR_EXEC(e.to_string()))?;
    let mut out = String::new();
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        out.push_str(&String::from_utf8_lossy(&data));
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        out.push_str(&String::from_utf8_lossy(&data));
                    }
                    Some(russh::ChannelMsg::Close) => break,
                    Some(_) => {}
                    None => break,
                }
            }
            _ = tokio::time::sleep(EXEC_CANCEL_POLL) => {
                if cancel.load(Ordering::Relaxed) {
                    // 客户端放弃等待并通知服务器关闭通道；无 PTY 会话下远端进程
                    // 通常随通道关闭终止，个别场景可能留存（详见中断提示文案）
                    let _ = channel.close().await;
                    return Err(lang::SSH_CMD_ABORTED().to_string());
                }
            }
        }
    }
    Ok(out)
}

/// 建立 SSH 连接 + 认证 + 打开 SFTP 子系统，返回句柄与会话及家目录。
async fn connect(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
) -> Result<(Handle<SshHandler>, SftpSession, String), String> {
    let config = Arc::new(russh::client::Config::default());
    let mut session = russh::client::connect(config, (host, port), SshHandler)
        .await
        .map_err(|e| lang::SFTP_ERR_CONNECT(e.to_string()))?;
    let auth = session
        .authenticate_password(user, pass)
        .await
        .map_err(|e| lang::SFTP_ERR_CONNECT(e.to_string()))?;
    if !auth.success() {
        return Err(lang::SFTP_ERR_AUTH().to_string());
    }
    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| lang::SFTP_ERR_CONNECT(e.to_string()))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| lang::SFTP_ERR_CONNECT(e.to_string()))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| lang::SFTP_ERR_CONNECT(e.to_string()))?;
    let cwd = sftp
        .canonicalize(".")
        .await
        .map_err(|e| lang::SFTP_ERR_CONNECT(e.to_string()))?;
    Ok((session, sftp, cwd))
}

/// 目录列表缓存有效期：期间内重复浏览（上级/双击返回等导航）直接命中内存，零网络往返。
const CACHE_TTL: Duration = Duration::from_secs(15);

/// 缓存键归一化：去掉尾部 `/`（根目录 `/` 保持原样），避免同目录不同写法重复请求
fn cache_key(path: &str) -> String {
    let p = path.trim_end_matches('/');
    if p.is_empty() {
        "/".to_string()
    } else {
        p.to_string()
    }
}

/// 列目录（带缓存）：目录在前、名称升序。
/// `force=false` 且缓存未过期 → 直接返回缓存条目；`force=true` 或缓存缺失/过期 →
/// 向服务器重拉并回填缓存。read_dir 一次往返即带回属性，无需逐条 stat。
async fn list(
    state: &mut WorkerState,
    path: &str,
    force: bool,
) -> Result<(String, Vec<SftpEntry>), String> {
    if !force {
        if let Some((at, entries)) = state.dir_cache.get(&cache_key(path)) {
            if at.elapsed() < CACHE_TTL {
                return Ok((path.to_string(), entries.clone()));
            }
        }
    }
    let Some(sftp) = &state.sftp else {
        return Err(lang::SFTP_NO_CONN().to_string());
    };
    let mut out = Vec::new();
    let dir = sftp
        .read_dir(path)
        .await
        .map_err(|e| lang::SFTP_ERR_LIST(e.to_string()))?;
    for f in dir {
        let m = f.metadata();
        out.push(SftpEntry {
            name: f.file_name(),
            is_dir: m.is_dir(),
            size: m.size.unwrap_or(0),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    state
        .dir_cache
        .insert(cache_key(path), (Instant::now(), out.clone()));
    Ok((path.to_string(), out))
}

async fn mkdir(state: &WorkerState, path: &str) -> Result<(), String> {
    let Some(sftp) = &state.sftp else {
        return need_conn();
    };
    sftp.create_dir(path)
        .await
        .map_err(|e| lang::SFTP_ERR_MKDIR(e.to_string()))
}

async fn delete(state: &WorkerState, path: &str, is_dir: bool) -> Result<(), String> {
    let Some(sftp) = &state.sftp else {
        return need_conn();
    };
    let err = |e: russh_sftp::client::error::Error| lang::SFTP_ERR_DELETE(e.to_string());
    if is_dir {
        // 目录必须递归删：remove_dir 只能删空目录
        delete_dir_recursive(sftp, path).await
    } else {
        sftp.remove_file(path).await.map_err(err)
    }
}

/// 深度优先递归删除目录：先删空所有子目录与文件（自底向上），最后删目录本身。
/// async fn 递归需 Box::pin 装箱。
fn delete_dir_recursive<'a>(
    sftp: &'a SftpSession,
    path: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        let dir = sftp
            .read_dir(path)
            .await
            .map_err(|e| lang::SFTP_ERR_DELETE(e.to_string()))?;
        for f in dir {
            let is_dir = f.metadata().is_dir();
            let child = join_path(path, &f.file_name());
            if is_dir {
                delete_dir_recursive(sftp, &child).await?;
            } else {
                sftp.remove_file(&child)
                    .await
                    .map_err(|e| lang::SFTP_ERR_DELETE(e.to_string()))?;
            }
        }
        sftp.remove_dir(path)
            .await
            .map_err(|e| lang::SFTP_ERR_DELETE(e.to_string()))
    })
}

async fn upload(state: &WorkerState, local: &str, remote: &str) -> Result<(), String> {
    let Some(sftp) = &state.sftp else {
        return need_conn();
    };
    let mut file = std::fs::File::open(local).map_err(|e| lang::SFTP_ERR_UPLOAD(e.to_string()))?;
    let mut f = sftp
        .open_with_flags(
            remote,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE | OpenFlags::READ,
        )
        .await
        .map_err(|e| lang::SFTP_ERR_UPLOAD(e.to_string()))?;
    // 分块流式上传（64 KiB 缓冲），避免大文件整体读入内存
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf)
            .map_err(|e| lang::SFTP_ERR_UPLOAD(e.to_string()))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])
            .await
            .map_err(|e| lang::SFTP_ERR_UPLOAD(e.to_string()))?;
    }
    f.flush()
        .await
        .map_err(|e| lang::SFTP_ERR_UPLOAD(e.to_string()))?;
    let _ = f.shutdown().await;
    Ok(())
}

async fn download(state: &WorkerState, remote: &str, local_dir: &str) -> Result<(), String> {
    let Some(sftp) = &state.sftp else {
        return need_conn();
    };
    let fname = std::path::Path::new(remote)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| remote.to_string());
    let dest = std::path::Path::new(local_dir).join(fname);
    let mut file =
        std::fs::File::create(&dest).map_err(|e| lang::SFTP_ERR_DOWNLOAD(e.to_string()))?;
    let mut f = sftp
        .open(remote)
        .await
        .map_err(|e| lang::SFTP_ERR_DOWNLOAD(e.to_string()))?;
    // 分块流式下载（64 KiB 缓冲），避免大文件整体驻留内存
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .await
            .map_err(|e| lang::SFTP_ERR_DOWNLOAD(e.to_string()))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| lang::SFTP_ERR_DOWNLOAD(e.to_string()))?;
    }
    let _ = f.shutdown().await;
    Ok(())
}

/// 路径拼接：cwd（绝对路径） + 名称。
pub fn join_path(cwd: &str, name: &str) -> String {
    if cwd == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), name)
    }
}

/// 取父目录："/a/b" → "/a"，"/a" → "/"，"/" → "/"。
pub fn parent_path(path: &str) -> String {
    let p = path.trim_end_matches('/');
    if p.is_empty() || p == "/" {
        return "/".to_string();
    }
    match p.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => p[..i].to_string(),
        None => "/".to_string(),
    }
}

/// 字节数 → 人类可读（B/KB/MB/GB）。
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{human_size, join_path, parent_path};

    #[test]
    fn join_path_cases() {
        assert_eq!(join_path("/", "a"), "/a");
        assert_eq!(join_path("/a", "b"), "/a/b");
        assert_eq!(join_path("/a/", "b"), "/a/b");
        assert_eq!(join_path("/a/b", "c.txt"), "/a/b/c.txt");
    }

    #[test]
    fn parent_path_cases() {
        assert_eq!(parent_path("/a/b"), "/a");
        assert_eq!(parent_path("/a"), "/");
        assert_eq!(parent_path("/"), "/");
        assert_eq!(parent_path("/a/b/"), "/a"); // 尾斜杠容忍
        assert_eq!(parent_path(""), "/");
    }

    #[test]
    fn human_size_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.00 MB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }
}
