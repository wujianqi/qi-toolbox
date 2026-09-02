//! 远程检测（业务层，不含任何 UI 代码）
//!
//! 支持 http/https/ws/wss/ftp 等常见网络协议地址的自动解析，提供：
//! - 连接：TCP 建连（https/wss 额外做 TLS 握手），返回 RTT 与解析到的 IP
//! - Ping：TCP 建连往返时延（不依赖 ICMP，无需管理员权限），连测 3 次取 min/avg/max
//! - SSL 证书状态：TLS 握手 + 证书链校验（系统根证书库）+ 证书详情解析（仅 https/wss）
//! - 网页状态：HTTP GET 请求并解析状态码/响应大小/耗时，自动跟随重定向（仅 http/https）
//!
//! 网络操作一律在独立线程执行，结果经 [`MsgSink`] 投递回 UI 线程。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use native_tls::TlsConnector;
use url::Url;
use x509_parser::parse_x509_certificate;

use crate::core::MsgSink;
use crate::lang;

/// TCP 建连超时（DNS 解析不在此限内）
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 网页状态检查：读取响应体的超时与大小上限
const WEB_READ_TIMEOUT: Duration = Duration::from_secs(5);
const WEB_MAX_BYTES: usize = 1024 * 1024;
/// 网页状态检查：最大跟随重定向跳数
const MAX_REDIRECTS: usize = 5;

/// 解析后的远程端点（UI 线程持有，供 Ping/SSL/网页状态复用）
#[derive(Clone, Debug)]
pub struct RemoteEndpoint {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// 请求路径（http/https 网页状态检查用，默认 "/"）
    pub path: String,
    /// 规范化显示：scheme://host[:port][path]
    pub display: String,
}

impl RemoteEndpoint {
    /// 是否加密协议（https/wss 需要 TLS 握手）
    pub fn is_tls(&self) -> bool {
        matches!(self.scheme.as_str(), "https" | "wss")
    }

    /// 是否网页协议（http/https 才支持网页状态检查）
    pub fn is_http(&self) -> bool {
        matches!(self.scheme.as_str(), "http" | "https")
    }
}

/// 后台 → UI 的消息
pub enum RemoteMsg {
    /// 连接结果：Ok((RTT ms, 解析到的 IP))
    Connected(Result<ConnectInfo, String>),
    /// Ping 结果：Ok(RTT ms)
    PingDone(Result<PingInfo, String>),
    /// SSL 证书检查结果
    SslDone(Result<SslInfo, String>),
    /// 网页状态检查结果
    WebDone(Result<WebStatusInfo, String>),
}

/// 连接结果（RTT 与解析到的 IP）
#[derive(Clone)]
pub struct ConnectInfo {
    pub rtt_ms: u128,
    pub ip: String,
}

/// Ping 结果（TCP 建连往返时延统计，连测多次取 min/avg/max）
#[derive(Clone)]
pub struct PingInfo {
    /// 探测次数
    pub count: usize,
    pub min_ms: u128,
    pub avg_ms: u128,
    pub max_ms: u128,
}

/// SSL 证书详情
#[derive(Clone)]
pub struct SslInfo {
    pub subject: String,
    pub issuer: String,
    pub not_before: String,
    pub not_after: String,
    /// 剩余有效天数（负数 = 已过期）
    pub days_left: i64,
}

/// 网页状态检查结果（HTTP 状态码 + 响应大小 + 耗时 + 重定向/响应头信息）
#[derive(Clone)]
pub struct WebStatusInfo {
    pub status_code: u16,
    pub reason: String,
    pub rtt_ms: u128,
    pub bytes: u64,
    /// 跟随重定向跳数（未跳转 = 0）
    pub redirects: usize,
    /// 最终请求地址（重定向后可能变化）
    pub final_url: String,
    /// Content-Type 响应头（无则空串）
    pub content_type: String,
    /// Server 响应头（无则空串）
    pub server: String,
}

/// 解析网址：支持 http/https/ws/wss/ftp，缺省协议前缀时按 https 处理。
/// 端口缺省时按协议默认值（http/ws=80，https/wss=443，ftp=21）。
pub fn parse_endpoint(input: &str) -> Result<RemoteEndpoint, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(lang::REMOTE_ERR_EMPTY().to_string());
    }
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{}", raw)
    };
    let url = Url::parse(&with_scheme)
        .map_err(|e| lang::REMOTE_ERR_PARSE().replace("{}", &e.to_string()))?;

    let scheme = url.scheme().to_string();
    let default_port = match scheme.as_str() {
        "http" | "ws" => 80u16,
        "https" | "wss" => 443,
        "ftp" => 21,
        _ => return Err(lang::REMOTE_ERR_SCHEME().replace("{}", &scheme)),
    };
    let host = url
        .host_str()
        .ok_or_else(|| lang::REMOTE_ERR_HOST().to_string())?
        .to_string();
    let port = url.port().unwrap_or(default_port);
    let path = if url.path().is_empty() { "/" } else { url.path() }.to_string();
    let display = if url.port().is_some() {
        format!("{}://{}:{}{}", scheme, host, port, path)
    } else {
        format!("{}://{}{}", scheme, host, path)
    };

    Ok(RemoteEndpoint {
        scheme,
        host,
        port,
        path,
        display,
    })
}

/// TCP 建连（带超时，依次尝试解析出的每个地址）
fn tcp_connect(host: &str, port: u16) -> Result<(TcpStream, SocketAddr), String> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| lang::REMOTE_ERR_DNS().replace("{}", &e.to_string()))?
        .collect();
    if addrs.is_empty() {
        return Err(lang::REMOTE_ERR_DNS().replace("{}", "no address"));
    }
    let mut last_err = String::new();
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => return Ok((s, addr)),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(lang::REMOTE_ERR_CONNECT().replace("{}", &last_err))
}

/// 建连（https/wss 额外 TLS 握手），返回 RTT 与 IP
fn run_connect(ep: &RemoteEndpoint) -> Result<ConnectInfo, String> {
    let start = Instant::now();
    let (stream, addr) = tcp_connect(&ep.host, ep.port)?;
    if ep.is_tls() {
        let connector = TlsConnector::new()
            .map_err(|e| lang::REMOTE_ERR_TLS().replace("{}", &e.to_string()))?;
        connector
            .connect(&ep.host, stream)
            .map_err(|e| lang::REMOTE_ERR_TLS().replace("{}", &e.to_string()))?;
    }
    let rtt_ms = start.elapsed().as_millis();
    Ok(ConnectInfo {
        rtt_ms,
        ip: addr.ip().to_string(),
    })
}

/// TCP 建连往返时延（Ping）：连测 3 次，返回 min/avg/max
fn run_ping(ep: &RemoteEndpoint) -> Result<PingInfo, String> {
    const PROBES: usize = 3;
    let mut rtts = Vec::with_capacity(PROBES);
    for _ in 0..PROBES {
        let start = Instant::now();
        let (stream, _) = tcp_connect(&ep.host, ep.port)?;
        drop(stream);
        rtts.push(start.elapsed().as_millis());
    }
    let min_ms = *rtts.iter().min().unwrap_or(&0);
    let max_ms = *rtts.iter().max().unwrap_or(&0);
    let avg_ms = rtts.iter().sum::<u128>() / rtts.len().max(1) as u128;
    Ok(PingInfo {
        count: rtts.len(),
        min_ms,
        avg_ms,
        max_ms,
    })
}

/// TLS 握手 + 证书链校验 + 证书详情（仅 https/wss）
fn run_check_ssl(ep: &RemoteEndpoint) -> Result<SslInfo, String> {
    if !ep.is_tls() {
        return Err(lang::REMOTE_NOT_TLS().to_string());
    }
    let (stream, _) = tcp_connect(&ep.host, ep.port)?;
    let connector = TlsConnector::new()
        .map_err(|e| lang::REMOTE_ERR_TLS().replace("{}", &e.to_string()))?;
    let tls = connector
        .connect(&ep.host, stream)
        .map_err(|e| lang::REMOTE_ERR_TLS().replace("{}", &e.to_string()))?;
    let cert = tls
        .peer_certificate()
        .map_err(|e| lang::REMOTE_ERR_TLS().replace("{}", &e.to_string()))?
        .ok_or_else(|| lang::REMOTE_ERR_NO_CERT().to_string())?;
    let der = cert
        .to_der()
        .map_err(|e| lang::REMOTE_ERR_CERT().replace("{}", &e.to_string()))?;
    let (_, parsed) = parse_x509_certificate(&der)
        .map_err(|e| lang::REMOTE_ERR_CERT().replace("{}", &e.to_string()))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days_left = (parsed.validity().not_after.timestamp() - now) / 86_400;
    let cert_err = |e: String| lang::REMOTE_ERR_CERT().replace("{}", &e);
    let not_before = parsed
        .validity()
        .not_before
        .to_rfc2822()
        .map_err(cert_err)?;
    let not_after = parsed
        .validity()
        .not_after
        .to_rfc2822()
        .map_err(cert_err)?;

    Ok(SslInfo {
        subject: parsed.subject().to_string(),
        issuer: parsed.issuer().to_string(),
        not_before,
        not_after,
        days_left,
    })
}

/// 明文 / TLS 双态连接（网页状态检查统一读写接口）。
/// TLS 变体装箱：schannel 的 `TlsStream` 体积远大于 `TcpStream`，不装箱会把
/// 整个枚举撑到大变体尺寸（clippy::large_enum_variant）。
enum Conn {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            Conn::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Tls(s) => s.flush(),
        }
    }
}

/// 单次 HTTP GET 的响应摘要（状态码 + 关注头 + 响应字节数）
struct HttpResponse {
    status_code: u16,
    reason: String,
    /// Location 头（重定向跟随用）
    location: String,
    content_type: String,
    server: String,
    bytes: u64,
}

/// 发起一次 HTTP GET（新建连接，写请求后**整段读取响应**并解析）。仅 http/https。
/// 整段读入（上限 [`WEB_MAX_BYTES`]）后一次性解析状态行与关注头，
/// 避免逐字节/逐行小读带来的大量系统调用。
fn http_request(scheme: &str, host: &str, port: u16, path: &str) -> Result<HttpResponse, String> {
    let (stream, _) = tcp_connect(host, port)?;
    // 读取响应设超时：服务器不按 Connection: close 关闭时也能按时收尾
    stream
        .set_read_timeout(Some(WEB_READ_TIMEOUT))
        .map_err(|e| lang::REMOTE_ERR_HTTP().replace("{}", &e.to_string()))?;

    let mut conn = if scheme == "https" {
        let connector = TlsConnector::new()
            .map_err(|e| lang::REMOTE_ERR_TLS().replace("{}", &e.to_string()))?;
        let tls = connector
            .connect(host, stream)
            .map_err(|e| lang::REMOTE_ERR_TLS().replace("{}", &e.to_string()))?;
        Conn::Tls(Box::new(tls))
    } else {
        Conn::Plain(stream)
    };

    // Host 头：非默认端口时携带端口
    let is_default_port = if scheme == "http" { port == 80 } else { port == 443 };
    let host_header = if is_default_port {
        host.to_string()
    } else {
        format!("{}:{}", host, port)
    };
    // GET 请求：只带必要头；Connection: close 让服务器发完即断
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Qi-Toolbox\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        path, host_header
    );
    conn.write_all(request.as_bytes())
        .map_err(|e| lang::REMOTE_ERR_HTTP().replace("{}", &e.to_string()))?;

    // 整段读取响应（含头与 body，上限 WEB_MAX_BYTES；超时/断开即止），
    // 之后一次性解析——避免逐字节/逐行小读的大量系统调用
    let mut resp = Vec::with_capacity(2048);
    let mut buf = [0u8; 8192];
    loop {
        if resp.len() >= WEB_MAX_BYTES {
            break;
        }
        let n = match conn.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // 读超时 / 连接重置：已到手的内容照常解析
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::ConnectionReset =>
            {
                break;
            }
            Err(e) => return Err(lang::REMOTE_ERR_READ().replace("{}", &e.to_string())),
        };
        resp.extend_from_slice(&buf[..n]);
    }

    // 解析状态行：HTTP/1.x <code> <reason>（str::lines 兼容 \r\n）
    let text = String::from_utf8_lossy(&resp);
    let mut lines = text.lines();
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.split_whitespace();
    if !parts.next().unwrap_or("").starts_with("HTTP/") {
        return Err(lang::REMOTE_ERR_BAD_RESPONSE().to_string());
    }
    let status_code: u16 = parts
        .next()
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| lang::REMOTE_ERR_BAD_RESPONSE().to_string())?;
    let reason = parts.collect::<Vec<_>>().join(" ");

    // 响应头：到空行为止，收集关注的字段
    let mut location = String::new();
    let mut content_type = String::new();
    let mut server = String::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if k.eq_ignore_ascii_case("location") && location.is_empty() {
                location = v.to_string();
            } else if k.eq_ignore_ascii_case("content-type") && content_type.is_empty() {
                content_type = v.to_string();
            } else if k.eq_ignore_ascii_case("server") && server.is_empty() {
                server = v.to_string();
            }
        }
    }

    Ok(HttpResponse {
        status_code,
        reason,
        location,
        content_type,
        server,
        bytes: resp.len() as u64,
    })
}

/// 网页状态检查（仅 http/https）：GET 请求并解析状态码/响应大小/耗时。
/// 30x 带 Location 时自动跟随（最多 [`MAX_REDIRECTS`] 跳，跨协议不跟随），
/// 返回落地页的状态码与重定向跳数。
fn run_web_status(ep: &RemoteEndpoint) -> Result<WebStatusInfo, String> {
    if !ep.is_http() {
        return Err(lang::REMOTE_WEB_HTTP_ONLY().to_string());
    }
    let start = Instant::now();
    let mut scheme = ep.scheme.clone();
    let mut host = ep.host.clone();
    let mut port = ep.port;
    let mut path = ep.path.clone();
    let mut redirects = 0usize;

    let resp = loop {
        let resp = http_request(&scheme, &host, port, &path)?;
        let is_redirect = (300..400).contains(&resp.status_code) && !resp.location.is_empty();
        if !is_redirect || redirects >= MAX_REDIRECTS {
            break resp;
        }
        // 解析 Location：相对路径或绝对 URL（url join 自动拼接）
        let base = Url::parse(&format!("{}://{}:{}", scheme, host, port))
            .map_err(|e| lang::REMOTE_ERR_PARSE().replace("{}", &e.to_string()))?;
        match base.join(&resp.location) {
            Ok(next) => {
                let s = next.scheme().to_string();
                // 跨协议跳转（如跳到 ftp://）不跟随，如实返回当前响应
                if !matches!(s.as_str(), "http" | "https") {
                    break resp;
                }
                let Some(h) = next.host_str() else {
                    break resp;
                };
                scheme = s;
                host = h.to_string();
                port = next.port().unwrap_or(if scheme == "https" { 443 } else { 80 });
                path = if next.path().is_empty() {
                    "/"
                } else {
                    next.path()
                }
                .to_string();
                redirects += 1;
            }
            Err(_) => break resp,
        }
    };

    let final_url = format!("{}://{}:{}{}", scheme, host, port, path);
    Ok(WebStatusInfo {
        status_code: resp.status_code,
        reason: resp.reason,
        rtt_ms: start.elapsed().as_millis(),
        bytes: resp.bytes,
        redirects,
        final_url,
        content_type: resp.content_type,
        server: resp.server,
    })
}

/// 后台线程：连接远端（TCP + 可选 TLS），结果经 sink 回传
pub fn spawn_connect(sink: MsgSink<RemoteMsg>, endpoint: RemoteEndpoint) {
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run_connect(&endpoint)));
        let msg = match r {
            Ok(Ok(info)) => RemoteMsg::Connected(Ok(info)),
            Ok(Err(e)) => RemoteMsg::Connected(Err(e)),
            Err(_) => RemoteMsg::Connected(Err(lang::REMOTE_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：Ping（TCP 建连时延），结果经 sink 回传
pub fn spawn_ping(sink: MsgSink<RemoteMsg>, endpoint: RemoteEndpoint) {
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run_ping(&endpoint)));
        let msg = match r {
            Ok(Ok(info)) => RemoteMsg::PingDone(Ok(info)),
            Ok(Err(e)) => RemoteMsg::PingDone(Err(e)),
            Err(_) => RemoteMsg::PingDone(Err(lang::REMOTE_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：SSL 证书检查，结果经 sink 回传
pub fn spawn_check_ssl(sink: MsgSink<RemoteMsg>, endpoint: RemoteEndpoint) {
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run_check_ssl(&endpoint)));
        let msg = match r {
            Ok(Ok(info)) => RemoteMsg::SslDone(Ok(info)),
            Ok(Err(e)) => RemoteMsg::SslDone(Err(e)),
            Err(_) => RemoteMsg::SslDone(Err(lang::REMOTE_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：网页状态检查（HTTP 状态码），结果经 sink 回传
pub fn spawn_web_status(sink: MsgSink<RemoteMsg>, endpoint: RemoteEndpoint) {
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run_web_status(&endpoint)));
        let msg = match r {
            Ok(Ok(info)) => RemoteMsg::WebDone(Ok(info)),
            Ok(Err(e)) => RemoteMsg::WebDone(Err(e)),
            Err(_) => RemoteMsg::WebDone(Err(lang::REMOTE_PANIC().to_string())),
        };
        sink(msg);
    });
}
