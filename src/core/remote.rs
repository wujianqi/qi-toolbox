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
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::ClientConfig;
use rustls_pki_types::ServerName;
use url::Url;
use x509_parser::parse_x509_certificate;

use crate::core::MsgSink;
use crate::lang;

/// 全局共享 rustls 客户端配置（根证书用 webpki-roots 内置库）
fn tls_config() -> Arc<ClientConfig> {
    use std::sync::OnceLock;
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Arc::new(
                ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .expect("tls protocol versions")
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

/// TLS 握手（rustls，webpki-roots 根证书），失败统一转 TLS 错误文案
fn tls_handshake(
    host: &str,
    stream: TcpStream,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, String> {
    let server_name =
        ServerName::try_from(host.to_string()).map_err(|e| lang::REMOTE_ERR_TLS(e.to_string()))?;
    let conn = rustls::ClientConnection::new(tls_config(), server_name)
        .map_err(|e| lang::REMOTE_ERR_TLS(e.to_string()))?;
    let mut tls = rustls::StreamOwned::new(conn, stream);
    // rustls 握手是惰性的：StreamOwned 创建后并不会自动握手，必须显式驱动 IO
    // 直到握手完成，否则 peer_certificates() 恒为 None（SSL 检测拿不到证书）。
    while tls.conn.is_handshaking() {
        tls.conn
            .complete_io(&mut tls.sock)
            .map_err(|e| lang::REMOTE_ERR_TLS(e.to_string()))?;
    }
    Ok(tls)
}

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
    /// 安全检测结果
    SecDone(Result<SecInfo, String>),
    /// SEO 分析结果
    SeoDone(Result<SeoInfo, String>),
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
    /// 解析出的目标 IP（首次探测时记录；空串 = 无此项，展示时跳过）
    pub ip: String,
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
    /// 响应体（UTF-8 lossy，上限 WEB_MAX_BYTES）
    pub body: String,
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
    let url = Url::parse(&with_scheme).map_err(|e| lang::REMOTE_ERR_PARSE(e.to_string()))?;

    let scheme = url.scheme().to_string();
    let default_port = match scheme.as_str() {
        "http" | "ws" => 80u16,
        "https" | "wss" => 443,
        "ftp" => 21,
        _ => return Err(lang::REMOTE_ERR_SCHEME(scheme)),
    };
    let host = url
        .host_str()
        .ok_or_else(|| lang::REMOTE_ERR_HOST().to_string())?
        .to_string();
    let port = url.port().unwrap_or(default_port);
    let path = if url.path().is_empty() {
        "/"
    } else {
        url.path()
    }
    .to_string();
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
        .map_err(|e| lang::REMOTE_ERR_DNS(e.to_string()))?
        .collect();
    if addrs.is_empty() {
        return Err(lang::REMOTE_ERR_DNS("no address"));
    }
    let mut last_err = String::new();
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => return Ok((s, addr)),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(lang::REMOTE_ERR_CONNECT(last_err))
}

/// 建连（https/wss 额外 TLS 握手），返回 RTT 与 IP
fn run_connect(ep: &RemoteEndpoint) -> Result<ConnectInfo, String> {
    let start = Instant::now();
    let (stream, addr) = tcp_connect(&ep.host, ep.port)?;
    if ep.is_tls() {
        tls_handshake(&ep.host, stream)?;
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
    let mut ip = String::new();
    for _ in 0..PROBES {
        let start = Instant::now();
        let (stream, addr) = tcp_connect(&ep.host, ep.port)?;
        if ip.is_empty() {
            ip = addr.ip().to_string();
        }
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
        ip,
    })
}

/// TLS 握手 + 证书链校验 + 证书详情（仅 https/wss）
fn run_check_ssl(ep: &RemoteEndpoint) -> Result<SslInfo, String> {
    if !ep.is_tls() {
        return Err(lang::REMOTE_NOT_TLS().to_string());
    }
    let (stream, _) = tcp_connect(&ep.host, ep.port)?;
    let tls = tls_handshake(&ep.host, stream)?;
    let der: Vec<u8> = tls
        .conn
        .peer_certificates()
        .and_then(|certs: &[rustls_pki_types::CertificateDer]| {
            certs.first().map(|c| c.as_ref().to_vec())
        })
        .ok_or_else(|| lang::REMOTE_ERR_NO_CERT().to_string())?;
    let (_, parsed) =
        parse_x509_certificate(&der).map_err(|e| lang::REMOTE_ERR_CERT(e.to_string()))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days_left = (parsed.validity().not_after.timestamp() - now) / 86_400;
    let cert_err = |e: String| lang::REMOTE_ERR_CERT(e);
    let not_before = parsed
        .validity()
        .not_before
        .to_rfc2822()
        .map_err(cert_err)?;
    let not_after = parsed.validity().not_after.to_rfc2822().map_err(cert_err)?;

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
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
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

/// 单次 HTTP GET 的响应摘要（状态码 + 关注头 + 响应字节数 + 响应体）
struct HttpResponse {
    status_code: u16,
    reason: String,
    /// Location 头（重定向跟随用）
    location: String,
    content_type: String,
    server: String,
    bytes: u64,
    /// 响应体（UTF-8 lossy，上限 WEB_MAX_BYTES；SEO 分析与响应体展示用）
    body: String,
}

/// 发起一次 HTTP GET（新建连接，写请求后**整段读取响应**并解析）。仅 http/https。
/// 整段读入（上限 [`WEB_MAX_BYTES`]）后一次性解析状态行与关注头，
/// 避免逐字节/逐行小读带来的大量系统调用。
fn http_request(scheme: &str, host: &str, port: u16, path: &str) -> Result<HttpResponse, String> {
    let (stream, _) = tcp_connect(host, port)?;
    // 读取响应设超时：服务器不按 Connection: close 关闭时也能按时收尾
    stream
        .set_read_timeout(Some(WEB_READ_TIMEOUT))
        .map_err(|e| lang::REMOTE_ERR_HTTP(e.to_string()))?;

    let mut conn = if scheme == "https" {
        Conn::Tls(Box::new(tls_handshake(host, stream)?))
    } else {
        Conn::Plain(stream)
    };

    // Host 头：非默认端口时携带端口
    let is_default_port = if scheme == "http" {
        port == 80
    } else {
        port == 443
    };
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
        .map_err(|e| lang::REMOTE_ERR_HTTP(e.to_string()))?;

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
            Err(e) => return Err(lang::REMOTE_ERR_READ(e.to_string())),
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
        body: text.into_owned(),
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
            .map_err(|e| lang::REMOTE_ERR_PARSE(e.to_string()))?;
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
                port = next
                    .port()
                    .unwrap_or(if scheme == "https" { 443 } else { 80 });
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
        body: resp.body,
    })
}

/// SEO 检查结果（解析首页 HTML 的基础 SEO 元信息）
#[derive(Clone)]
pub struct SeoInfo {
    pub title: String,
    /// 缺失 = 空串
    pub description: String,
    /// 缺失 = 空串
    pub keywords: String,
    pub canonical: String,
    /// viewport meta（移动端适配），缺失 = 空串
    pub viewport: String,
    pub h1_count: usize,
    pub img_total: usize,
    /// 缺 alt 属性的图片数
    pub img_no_alt: usize,
    /// 响应体大小（字节，HTML 源码）
    pub html_bytes: u64,
}

/// 从 HTML 提取第一个 `<tag ...>` 的属性值（name 不区分大小写）。
/// 极简扫描：定位 `<tag`，逐段读 `key="value"` / `key=value`。
fn meta_attr(html: &str, tag: &str, attr: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(pos) = lower[search_from..].find(&format!("<{}", tag)) {
        let start = search_from + pos;
        // 标签名后必须是非字母（排除 `<titlexxx` 之类的误配）
        let after = &lower[start + 1 + tag.len()..];
        if after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            search_from = start + 1;
            continue;
        }
        // 标签结束前扫描属性
        let tag_end = after
            .find('>')
            .map(|i| start + 1 + tag.len() + i)
            .unwrap_or(html.len());
        let seg = &html[start + 1 + tag.len()..tag_end.min(html.len())];
        let seg_lower = seg.to_ascii_lowercase();
        if let Some(ap) = seg_lower.find(attr) {
            // 属性名后必须是 `=`（排除近似属性名前缀误配）
            let rest = seg[ap + attr.len()..].trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                let val = val.trim_start();
                let quoted = val.starts_with('"') || val.starts_with('\'');
                let q = val.as_bytes()[0] as char;
                let inner = if quoted { &val[1..] } else { val };
                let end = if quoted {
                    inner.find(q).unwrap_or(inner.len())
                } else {
                    inner
                        .find(|c: char| c.is_whitespace())
                        .unwrap_or(inner.len())
                };
                return Some(inner[..end].to_string());
            }
        }
        search_from = start + 1;
    }
    None
}

/// 统计 `<h1>` 出现次数与 img 总数 / 缺 alt 数
fn count_tags(html: &str) -> (usize, usize, usize) {
    let lower = html.to_ascii_lowercase();
    let h1_count = lower.matches("<h1").count();
    let mut img_total = 0usize;
    let mut img_no_alt = 0usize;
    let mut from = 0usize;
    while let Some(pos) = lower[from..].find("<img") {
        let start = from + pos;
        let tag_end = lower[start..]
            .find('>')
            .map(|i| start + i)
            .unwrap_or(lower.len());
        let seg = lower[start..tag_end.min(lower.len())].to_string();
        img_total += 1;
        if !seg.contains("alt=") {
            img_no_alt += 1;
        }
        from = start + 4;
    }
    (h1_count, img_total, img_no_alt)
}

/// SEO 分析（仅 http/https）：解析网页 title/description/keywords/
/// canonical/viewport/H1/图片 alt，给出缺失项提示。
fn run_seo(ep: &RemoteEndpoint) -> Result<SeoInfo, String> {
    if !ep.is_http() {
        return Err(lang::REMOTE_WEB_HTTP_ONLY().to_string());
    }
    // 跟随重定向取落地页（复用网页状态的跳转逻辑上限）
    let mut scheme = ep.scheme.clone();
    let mut host = ep.host.clone();
    let mut port = ep.port;
    let mut path = ep.path.clone();
    let resp = loop {
        let r = http_request(&scheme, &host, port, &path)?;
        let is_redirect = (300..400).contains(&r.status_code) && !r.location.is_empty();
        if !is_redirect {
            break r;
        }
        let base = Url::parse(&format!("{}://{}:{}", scheme, host, port))
            .map_err(|e| lang::REMOTE_ERR_PARSE(e.to_string()))?;
        match base.join(&r.location) {
            Ok(next) if matches!(next.scheme(), "http" | "https") => {
                scheme = next.scheme().to_string();
                host = next.host_str().unwrap_or(&host).to_string();
                port = next
                    .port()
                    .unwrap_or(if scheme == "https" { 443 } else { 80 });
                path = if next.path().is_empty() {
                    "/"
                } else {
                    next.path()
                }
                .to_string();
            }
            _ => break r,
        }
    };
    if resp.status_code != 200 {
        return Err(lang::SEO_BAD_STATUS(resp.status_code as u64).to_string());
    }
    // title 单独提取（meta_attr 只取属性，title 取标签内容）
    let lower = resp.body.to_ascii_lowercase();
    let title = lower
        .find("<title")
        .and_then(|s| {
            let inner_start = resp.body[s..].find('>')? + s + 1;
            let end = lower[inner_start..].find("</title>")? + inner_start;
            Some(resp.body[inner_start..end].trim().to_string())
        })
        .unwrap_or_default();
    let (h1_count, img_total, img_no_alt) = count_tags(&resp.body);
    Ok(SeoInfo {
        title,
        description: meta_attr(&resp.body, "meta", "name=\"description\"").unwrap_or_default(),
        keywords: meta_attr(&resp.body, "meta", "name=\"keywords\"").unwrap_or_default(),
        canonical: meta_attr(&resp.body, "link", "rel=\"canonical\"").unwrap_or_default(),
        viewport: meta_attr(&resp.body, "meta", "name=\"viewport\"").unwrap_or_default(),
        h1_count,
        img_total,
        img_no_alt,
        html_bytes: resp.bytes,
    })
}

/// 后台线程：SEO 分析，结果经 sink 回传
pub fn spawn_seo(sink: MsgSink<RemoteMsg>, endpoint: RemoteEndpoint) {
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run_seo(&endpoint)));
        let msg = match r {
            Ok(Ok(info)) => RemoteMsg::SeoDone(Ok(info)),
            Ok(Err(e)) => RemoteMsg::SeoDone(Err(e)),
            Err(_) => RemoteMsg::SeoDone(Err(lang::REMOTE_PANIC().to_string())),
        };
        sink(msg);
    });
}

// ── 检测结果 → 展示文本（供 UI 结果区直接渲染，\n 分行）──

/// 响应体预览截断长度（字符数，超出追加省略号）
const BODY_PREVIEW_CHARS: usize = 2000;

/// SSL 证书检查结果 → 多行文本
pub fn format_ssl_info(info: &SslInfo) -> String {
    let mut text = String::new();
    text.push_str(&lang::REMOTE_SSL_TITLE());
    text.push('\n');
    text.push_str(&format!("{} {}", lang::REMOTE_SSL_SUBJECT(), info.subject));
    text.push('\n');
    text.push_str(&format!("{} {}", lang::REMOTE_SSL_ISSUER(), info.issuer));
    text.push('\n');
    text.push_str(&lang::REMOTE_SSL_VALID(&info.not_before, &info.not_after));
    text.push('\n');
    let days_txt = if info.days_left < 0 {
        lang::REMOTE_SSL_EXPIRED(-info.days_left)
    } else {
        lang::REMOTE_SSL_DAYS(info.days_left)
    };
    text.push_str(&days_txt);
    text
}

/// 网页状态 → 多行文本（只含状态摘要，**不含响应体**——响应体较长，
/// 详情经 [`format_web_detail`] 在弹窗里单独展示）
pub fn format_web_info(info: &WebStatusInfo) -> String {
    let mut text = lang::REMOTE_WEB_OK(
        info.status_code as u64,
        &info.reason,
        info.bytes,
        info.rtt_ms as u64,
    );
    if info.redirects > 0 {
        text.push_str(&format!(
            "\n{} {}\n{} {}",
            lang::REMOTE_WEB_REDIRECTS(),
            info.redirects,
            lang::REMOTE_WEB_FINAL(),
            info.final_url
        ));
    }
    if !info.content_type.is_empty() {
        text.push_str(&format!(
            "\n{} {}",
            lang::REMOTE_WEB_CTYPE(),
            info.content_type
        ));
    }
    if !info.server.is_empty() {
        text.push_str(&format!("\n{} {}", lang::REMOTE_WEB_SERVER(), info.server));
    }
    text
}

/// 网页状态响应体详情 → 多行文本（弹窗展示用：状态头 + 截断后的响应体）
pub fn format_web_detail(info: &WebStatusInfo) -> String {
    let mut text = format_web_info(info);
    // 响应体预览（截断，避免弹窗内容过长）
    if !info.body.trim().is_empty() {
        text.push_str("\n──────────\n");
        let mut body: String = info.body.chars().take(BODY_PREVIEW_CHARS).collect();
        if info.body.chars().count() > BODY_PREVIEW_CHARS {
            body.push('…');
        }
        text.push_str(&body);
    }
    text
}

/// SEO 分析结果 → 多行文本
pub fn format_seo_info(info: &SeoInfo) -> String {
    let kv = |label: String, val: &str| {
        if val.is_empty() {
            format!("{} {}", label, lang::SEO_MISSING())
        } else {
            format!("{} {}", label, val)
        }
    };
    let mut text = String::new();
    text.push_str(&lang::SEO_TITLE());
    text.push('\n');
    text.push_str(&kv(lang::SEO_TITLE_LABEL(), &info.title));
    text.push('\n');
    text.push_str(&kv(lang::SEO_DESCRIPTION(), &info.description));
    text.push('\n');
    text.push_str(&kv(lang::SEO_KEYWORDS(), &info.keywords));
    text.push('\n');
    text.push_str(&kv(lang::SEO_CANONICAL(), &info.canonical));
    text.push('\n');
    text.push_str(&kv(lang::SEO_VIEWPORT(), &info.viewport));
    text.push('\n');
    text.push_str(&format!("{} {}", lang::SEO_H1(), info.h1_count));
    text.push('\n');
    text.push_str(&lang::SEO_IMAGES_N(
        info.img_total as u64,
        info.img_no_alt as u64,
    ));
    text.push('\n');
    text.push_str(&format!(
        "{} {} bytes",
        lang::SEO_HTML_SIZE(),
        info.html_bytes
    ));
    text
}

/// 安全检测结果 → 多行文本
pub fn format_sec_info(info: &SecInfo) -> String {
    let mut text = String::new();
    text.push_str(&lang::SEC_TITLE());
    text.push('\n');
    if info.findings.is_empty() {
        text.push_str(&lang::SEC_OK(info.passed as u64));
    } else {
        let high = info
            .findings
            .iter()
            .filter(|f| f.level == SecLevel::High)
            .count();
        let mid = info
            .findings
            .iter()
            .filter(|f| f.level == SecLevel::Mid)
            .count();
        text.push_str(&lang::SEC_SUMMARY(high as u64, mid as u64));
        for f in &info.findings {
            text.push_str(&format!("\n{} {} — {}", f.level.mark(), f.title, f.detail));
        }
        text.push('\n');
        text.push_str(&lang::SEC_OK(info.passed as u64));
    }
    text
}

/// 后台线程：连接远端（TCP + 可选 TLS），结果经 sink 回传
pub fn spawn_connect(sink: MsgSink<RemoteMsg>, endpoint: RemoteEndpoint) {
    std::thread::spawn(move || {
        let r =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run_connect(&endpoint)));
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
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            run_check_ssl(&endpoint)
        }));
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
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            run_web_status(&endpoint)
        }));
        let msg = match r {
            Ok(Ok(info)) => RemoteMsg::WebDone(Ok(info)),
            Ok(Err(e)) => RemoteMsg::WebDone(Err(e)),
            Err(_) => RemoteMsg::WebDone(Err(lang::REMOTE_PANIC().to_string())),
        };
        sink(msg);
    });
}

// ────────────────────── 安全检测 ──────────────────────

/// 敏感路径探测清单（可公开访问即视为信息泄露）
const SENSITIVE_PATHS: &[&str] = &[
    ".env",
    ".git/config",
    "phpinfo.php",
    "backup.zip",
    "wp-config.php.bak",
    "server-status",
];

/// 常见风险端口（主机维度探测，与目标端口无关）
const RISK_PORTS: &[u16] = &[21, 22, 23, 445, 3306, 3389, 5432, 6379, 9200, 27017];

/// 单条发现（漏洞项 + 说明，如 "HSTS 缺失"）
#[derive(Clone)]
pub struct SecFinding {
    pub level: SecLevel,
    pub title: String,
    pub detail: String,
}

/// 漏洞等级：高 / 中
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SecLevel {
    High,
    Mid,
}

/// 安全检测结果：逐条发现 + 已通过项计数
#[derive(Clone)]
pub struct SecInfo {
    pub findings: Vec<SecFinding>,
    /// 通过（未发现问题）的检查项数
    pub passed: usize,
}

impl SecLevel {
    pub fn mark(self) -> &'static str {
        match self {
            SecLevel::High => "[!!!]",
            SecLevel::Mid => "[!! ]",
        }
    }
}

/// 单次 GET 的完整响应头集合（安全检测需要任意头，非固定关注字段）。
/// 复用 [`tcp_connect`] + TLS，整段读取后按行拆头。
fn http_get_headers(
    scheme: &str,
    host: &str,
    port: u16,
    path: &str,
) -> Result<(u16, Vec<(String, String)>), String> {
    let (stream, _) = tcp_connect(host, port)?;
    stream
        .set_read_timeout(Some(WEB_READ_TIMEOUT))
        .map_err(|e| lang::REMOTE_ERR_HTTP(e.to_string()))?;
    let mut conn = if scheme == "https" {
        Conn::Tls(Box::new(tls_handshake(host, stream)?))
    } else {
        Conn::Plain(stream)
    };
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Qi-Toolbox\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        path,
        if (scheme == "http" && port == 80) || (scheme == "https" && port == 443) {
            host.to_string()
        } else {
            format!("{}:{}", host, port)
        }
    );
    conn.write_all(request.as_bytes())
        .map_err(|e| lang::REMOTE_ERR_HTTP(e.to_string()))?;

    let mut resp = Vec::with_capacity(2048);
    let mut buf = [0u8; 8192];
    loop {
        if resp.len() >= WEB_MAX_BYTES {
            break;
        }
        let n = match conn.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::ConnectionReset =>
            {
                break;
            }
            Err(e) => return Err(lang::REMOTE_ERR_READ(e.to_string())),
        };
        resp.extend_from_slice(&buf[..n]);
    }

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
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    Ok((status_code, headers))
}

fn header_get<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// 端口是否可建连（失败即视为关闭/被过滤）
fn port_open(host: &str, port: u16) -> bool {
    tcp_connect(host, port).is_ok()
}

/// TLS 旧版本握手探测：rustls 不支持对特定旧版本做降级握手，因此这里
/// 通过手动 ClientHello 探测 TLS 1.0/1.1 —— 发送握手包后读取服务器
/// 响应字节判断是否被拒绝。
fn probe_legacy_tls(host: &str, port: u16, version: [u8; 2]) -> Result<bool, String> {
    use std::io::Write;
    let (mut stream, _) = tcp_connect(host, port)?;
    stream
        .set_read_timeout(Some(WEB_READ_TIMEOUT))
        .map_err(|e| lang::REMOTE_ERR_TLS(e.to_string()))?;
    // 极简 TLS ClientHello：Record(version) + Handshake(ClientHello, cipher: TLS_RSA_WITH_AES_128_CBC_SHA)
    let mut hs = Vec::new();
    hs.push(0x01); // ClientHello
    let mut body = Vec::new();
    body.extend_from_slice(&version); // client_version
    body.extend_from_slice(&[
        0x53, 0x8a, 0xc7, 0x1b, 0x2d, 0xa4, 0x0e, 0x5f, 0x93, 0x2c, 0x44, 0x18, 0x76, 0xa5, 0x2b,
        0xd1, 0x08, 0xf3, 0x61, 0x7e, 0x9c, 0x04, 0xed, 0x2f, 0x6b, 0x15, 0xc0, 0x3a, 0x87, 0xd9,
        0x4e, 0x72,
    ]); // random
    body.push(0x00); // session id len
    body.extend_from_slice(&[0x00, 0x02, 0x00, 0x2f]); // cipher_suites: [TLS_RSA_AES_128_CBC_SHA]
    body.push(0x01);
    body.push(0x00); // compression: null
    let body_len = body.len() as u32;
    hs.extend_from_slice(&body_len.to_be_bytes()[1..]);
    hs.extend_from_slice(&body);
    let rec_len = hs.len() as u16;
    let mut pkt = vec![0x16, version[0], version[1]];
    pkt.extend_from_slice(&rec_len.to_be_bytes());
    pkt.extend_from_slice(&hs);
    stream
        .write_all(&pkt)
        .map_err(|e| lang::REMOTE_ERR_TLS(e.to_string()))?;
    stream
        .flush()
        .map_err(|e| lang::REMOTE_ERR_TLS(e.to_string()))?;
    let mut resp = [0u8; 6];
    let n = stream.read(&mut resp).unwrap_or(0);
    // 返回 TLS 记录（content type 0x16/0x15）且版本匹配 → 接受该版本；
    // 立即断开（0 字节）/ 无效记录 → 拒绝
    Ok(n >= 3 && resp[0] == 0x16 && resp[1] == version[0] && resp[2] == version[1])
}

/// 安全检测：敏感路径暴露 / 安全响应头 / 风险端口 / TLS 旧版本。
/// Web 项仅 http/https；端口与 TLS 项按协议自动取舍。
fn run_security_check(ep: &RemoteEndpoint) -> Result<SecInfo, String> {
    if !ep.is_http() {
        return Err(lang::REMOTE_WEB_HTTP_ONLY().to_string());
    }
    let mut findings: Vec<SecFinding> = Vec::new();
    let mut passed = 0usize;

    // ① 敏感路径暴露（高）：探测清单内路径是否返回 2xx 且非重定向
    for path in SENSITIVE_PATHS {
        let url = format!("/{}/", path.trim_matches('/'));
        match http_get_headers(&ep.scheme, &ep.host, ep.port, &url) {
            Ok((code, _)) => {
                if (200..300).contains(&code) {
                    findings.push(SecFinding {
                        level: SecLevel::High,
                        title: lang::SEC_PATH_EXPOSED(*path).to_string(),
                        detail: lang::SEC_PATH_DETAIL(code as u64).to_string(),
                    });
                } else {
                    passed += 1;
                }
            }
            // 请求失败（连接拒绝/超时等）：跳过该项，不算通过也不算漏洞
            Err(_) => {}
        }
    }

    // ② 安全响应头缺失（中）
    let (_, headers) = http_get_headers(&ep.scheme, &ep.host, ep.port, &ep.path)?;
    let sec_headers: &[(&str, fn() -> String)] = &[
        ("strict-transport-security", lang::SEC_HSTS),
        ("content-security-policy", lang::SEC_CSP),
        ("x-frame-options", lang::SEC_XFO),
        ("x-content-type-options", lang::SEC_XCTO),
        ("referrer-policy", lang::SEC_REFERRER),
    ];
    for (name, label) in sec_headers {
        if header_get(&headers, name).is_none() {
            findings.push(SecFinding {
                level: SecLevel::Mid,
                title: lang::SEC_HEADER_MISSING(label()).to_string(),
                detail: lang::SEC_HEADER_DETAIL(*name).to_string(),
            });
        } else {
            passed += 1;
        }
    }

    // ③ 风险端口开放（中，高为数据库/缓存直连端口）
    for port in RISK_PORTS {
        if port_open(&ep.host, *port) {
            let level = if matches!(port, 3306 | 5432 | 6379 | 9200 | 27017) {
                SecLevel::High
            } else {
                SecLevel::Mid
            };
            findings.push(SecFinding {
                level,
                title: lang::SEC_PORT_OPEN(*port).to_string(),
                detail: lang::SEC_PORT_DETAIL(*port).to_string(),
            });
        } else {
            passed += 1;
        }
    }

    // ④ TLS 旧版本兼容（中，仅 https/wss 才有 TLS 层）
    if ep.is_tls() {
        for (ver, label) in [
            ([0x03u8, 0x01u8], lang::SEC_TLS10 as fn() -> String),
            ([0x03u8, 0x02u8], lang::SEC_TLS11),
        ] {
            match probe_legacy_tls(&ep.host, ep.port, ver) {
                Ok(true) => findings.push(SecFinding {
                    level: SecLevel::Mid,
                    title: lang::SEC_TLS_OLD(label()).to_string(),
                    detail: lang::SEC_TLS_DETAIL().to_string(),
                }),
                Ok(false) => passed += 1,
                Err(_) => {}
            }
        }
    }

    Ok(SecInfo { findings, passed })
}

/// 后台线程：安全检测，结果经 sink 回传
pub fn spawn_security_check(sink: MsgSink<RemoteMsg>, endpoint: RemoteEndpoint) {
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            run_security_check(&endpoint)
        }));
        let msg = match r {
            Ok(Ok(info)) => RemoteMsg::SecDone(Ok(info)),
            Ok(Err(e)) => RemoteMsg::SecDone(Err(e)),
            Err(_) => RemoteMsg::SecDone(Err(lang::REMOTE_PANIC().to_string())),
        };
        sink(msg);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_endpoint ──

    #[test]
    fn parse_endpoint_defaults() {
        // 缺省协议按 https，端口按协议默认值
        let ep = parse_endpoint("example.com").unwrap();
        assert_eq!(ep.scheme, "https");
        assert_eq!(ep.port, 443);
        assert_eq!(ep.host, "example.com");
        assert_eq!(ep.path, "/");
        // display 不带默认端口
        assert_eq!(ep.display, "https://example.com/");

        let http = parse_endpoint("http://example.com").unwrap();
        assert_eq!(http.scheme, "http");
        assert_eq!(http.port, 80);
    }

    #[test]
    fn parse_endpoint_with_port_and_path() {
        let ep = parse_endpoint("https://example.com:8443/admin/login").unwrap();
        assert_eq!(ep.port, 8443);
        assert_eq!(ep.path, "/admin/login");
        assert_eq!(ep.display, "https://example.com:8443/admin/login");
    }

    #[test]
    fn parse_endpoint_schemes() {
        assert_eq!(parse_endpoint("ws://example.com").unwrap().port, 80);
        assert_eq!(parse_endpoint("wss://example.com").unwrap().port, 443);
        assert_eq!(parse_endpoint("ftp://example.com").unwrap().port, 21);
    }

    #[test]
    fn parse_endpoint_rejects() {
        // 空输入
        assert!(parse_endpoint("").is_err());
        assert!(parse_endpoint("   ").is_err());
        // 不支持的协议
        assert!(parse_endpoint("gopher://example.com").is_err());
        // 无主机名
        assert!(parse_endpoint("https://").is_err());
    }

    #[test]
    fn parse_endpoint_trims_whitespace() {
        let ep = parse_endpoint("  https://example.com  ").unwrap();
        assert_eq!(ep.host, "example.com");
    }

    #[test]
    fn parse_endpoint_ipv6_host() {
        // IPv6 字面量：url crate 方括号内解析，host_str 保留方括号形式
        let ep = parse_endpoint("http://[::1]:8080/x").unwrap();
        assert_eq!(ep.port, 8080);
        assert_eq!(ep.path, "/x");
        assert!(ep.host.contains("::1"));
    }

    // ── meta_attr / count_tags（HTML 解析）──

    #[test]
    fn meta_attr_double_and_single_quotes() {
        assert_eq!(
            meta_attr(
                r#"<meta name="description" content="hello world">"#,
                "meta",
                "content"
            ),
            Some("hello world".to_string())
        );
        assert_eq!(
            meta_attr(
                r#"<meta name='keywords' content='a,b,c'>"#,
                "meta",
                "content"
            ),
            Some("a,b,c".to_string())
        );
    }

    #[test]
    fn meta_attr_case_insensitive_and_unquoted() {
        // 标签/属性名大小写不敏感
        assert_eq!(
            meta_attr(
                r#"<META NAME="Description" CONTENT="Big">"#,
                "meta",
                "content"
            ),
            Some("Big".to_string())
        );
        // 无引号值
        assert_eq!(
            meta_attr(
                r#"<meta name=viewport content=width-500>"#,
                "meta",
                "content"
            ),
            Some("width-500".to_string())
        );
    }

    #[test]
    fn meta_attr_missing_returns_none() {
        assert_eq!(meta_attr(r#"<meta name="x">"#, "meta", "content"), None);
        assert_eq!(meta_attr("plain text", "meta", "content"), None);
    }

    #[test]
    fn count_tags_basic() {
        let html = "<H1>Title</h1><h1>Another</h1>\
                    <img src='a.png'><IMG SRC='b.png' alt='x'><img/>";
        let (h1, total, no_alt) = count_tags(html);
        assert_eq!(h1, 2);
        assert_eq!(total, 3);
        assert_eq!(no_alt, 2);
    }

    #[test]
    fn count_tags_empty() {
        assert_eq!(count_tags(""), (0, 0, 0));
    }
}
