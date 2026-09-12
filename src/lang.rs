//! 国际化字符串 — 运行时语言切换
//!
//! 语言状态（0=英文 1=中文）由全局原子变量保存：
//!   - 启动时 `main()` 检测系统语言自动设置（中文系统→中文，否则英文）
//!   - 主面板侧栏底部「中 / EN」可手动切换
//!
//! 每个文案是运行时函数：`lang::FOO()` 按当前语言返回 `&'static str`。
//! 语言切换后由整树重建机制（build_ui 重跑）重新解析全部文案。

// 文案函数沿用旧常量的大写命名（迁移机械、便于对照旧代码），故豁免 non_snake_case。
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicU8, Ordering};

/// 英文
pub const LANG_EN: u8 = 0;
/// 中文
pub const LANG_ZH: u8 = 1;

static CURRENT: AtomicU8 = AtomicU8::new(LANG_ZH);

/// 当前语言（LANG_EN / LANG_ZH）
pub fn current() -> u8 {
    CURRENT.load(Ordering::Relaxed)
}

/// 设置语言（LANG_EN / LANG_ZH）
pub fn set_current(lang: u8) {
    CURRENT.store(lang, Ordering::Relaxed);
}

/// 按语言取文案：`["英文", "中文"][当前语言]`
fn s(en: &'static str, zh: &'static str) -> &'static str {
    [en, zh][current() as usize]
}

// ────────────────────── 软件名称 ──────────────────────
pub fn APP_NAME() -> &'static str {
    s("Qi Toolbox", "奇兔")
}

// ────────────────────── 侧栏菜单 ──────────────────────
pub fn TAB_2FA() -> &'static str {
    s("2FA Authenticator", "2FA验证码")
}
pub fn TAB_PASSWORD() -> &'static str {
    s("Random Passwords", "随机密码")
}
pub fn TAB_TURSO() -> &'static str {
    s("Turso DB", "Turso库浏览")
}
pub fn TAB_SFTP() -> &'static str {
    s("SFTP / SSH", "SFTP/SSH")
}
pub fn TAB_REMOTE() -> &'static str {
    s("Remote Check", "远程检测")
}
pub fn TAB_ABOUT() -> &'static str {
    s("About", "关于软件")
}

// ────────────────────── 侧栏底部 toggle ──────────────────────
pub fn TOGGLE_LANG() -> &'static str {
    s("Switch Language", "切换语言")
}
pub fn TOGGLE_THEME() -> &'static str {
    s("Switch Theme", "切换主题")
}
pub fn TOGGLE_SIDEBAR() -> &'static str {
    s("Show / Hide Sidebar", "显示/隐藏菜单")
}

// ────────────────────── SFTP 页 ──────────────────────
pub fn SFTP_HOST() -> &'static str {
    s("Host:", "主机：")
}
pub fn SFTP_PORT() -> &'static str {
    s("Port:", "端口：")
}
pub fn SFTP_USER() -> &'static str {
    s("Username:", "用户名：")
}
pub fn SFTP_PASS() -> &'static str {
    s("Password:", "密码：")
}
pub fn SFTP_CONNECT() -> &'static str {
    s("Connect", "连接")
}
pub fn SFTP_DISCONNECT() -> &'static str {
    s("Disconnect", "断开")
}
pub fn SFTP_CONNECTED() -> &'static str {
    s("\u{2705} Connected", "\u{2705} 已连接")
}
pub fn SFTP_PATH() -> &'static str {
    s("Path", "路径")
}
pub fn SFTP_UP() -> &'static str {
    s("Up", "上级")
}
pub fn SFTP_REFRESH() -> &'static str {
    s("Refresh", "刷新")
}
pub fn SFTP_MKDIR() -> &'static str {
    s("New Folder", "新建文件夹")
}
pub fn SFTP_UPLOAD() -> &'static str {
    s("Upload", "上传")
}
pub fn SFTP_DOWNLOAD() -> &'static str {
    s("Download", "下载")
}
pub fn SFTP_DELETE() -> &'static str {
    s("Delete", "删除")
}
pub fn SFTP_PICK_UPLOAD() -> &'static str {
    s("Select file to upload", "选择要上传的文件")
}
pub fn SFTP_PICK_DOWNLOAD() -> &'static str {
    s("Select save folder", "选择保存目录")
}
pub fn SFTP_MKDIR_TITLE() -> &'static str {
    s("New Folder", "新建文件夹")
}
pub fn SFTP_MKDIR_HINT() -> &'static str {
    s("Folder name", "文件夹名称")
}
pub fn SFTP_OK() -> &'static str {
    s("OK", "确定")
}
pub fn SFTP_CANCEL() -> &'static str {
    s("Cancel", "取消")
}
pub fn SFTP_NO_CONN() -> &'static str {
    s("Connect to a server first", "请先连接服务器")
}
pub fn SFTP_NO_SELECT() -> &'static str {
    s("Select a file first", "请先选择文件")
}
pub fn SFTP_NO_FILE_SELECT() -> &'static str {
    s(
        "Folders cannot be downloaded, please select files",
        "文件夹无法下载，请选择文件",
    )
}
pub fn SFTP_DELETE_CONFIRM_MULTI() -> &'static str {
    s(
        "Delete the following {} item(s)? This cannot be undone!",
        "确认删除以下 {} 项？删除后不可恢复！",
    )
}
pub fn SFTP_DELETE_DIR() -> &'static str {
    s("Folder", "文件夹")
}
pub fn SFTP_ERR_CONNECT() -> &'static str {
    s("Connect failed: {}", "连接失败: {}")
}
pub fn SFTP_ERR_AUTH() -> &'static str {
    s("Authentication failed", "认证失败")
}
pub fn SFTP_ERR_LIST() -> &'static str {
    s("List failed: {}", "读取目录失败: {}")
}
pub fn SFTP_ERR_MKDIR() -> &'static str {
    s("Create folder failed: {}", "新建文件夹失败: {}")
}
pub fn SFTP_ERR_DELETE() -> &'static str {
    s("Delete failed: {}", "删除失败: {}")
}
pub fn SFTP_ERR_UPLOAD() -> &'static str {
    s("Upload failed: {}", "上传失败: {}")
}
pub fn SFTP_ERR_DOWNLOAD() -> &'static str {
    s("Download failed: {}", "下载失败: {}")
}
pub fn SFTP_DONE_MKDIR() -> &'static str {
    s("Folder created", "已创建文件夹")
}
pub fn SFTP_DONE_DELETE() -> &'static str {
    s("Deleted", "已删除")
}
pub fn SFTP_DONE_UPLOAD() -> &'static str {
    s("Uploaded: {}", "上传完成: {}")
}
pub fn SFTP_DONE_DOWNLOAD() -> &'static str {
    s("Downloaded: {}", "下载完成: {}")
}
pub fn SFTP_CMD() -> &'static str {
    s("SSH Command", "SSH命令")
}
pub fn SFTP_CMD_WIN_TITLE() -> &'static str {
    s("SSH Command", "SSH 命令")
}
pub fn SFTP_CMD_HINT() -> &'static str {
    s("Enter a command, e.g. ls -la", "输入命令，如 ls -la")
}
pub fn SFTP_CMD_EXEC() -> &'static str {
    s("Run", "执行")
}
pub fn SFTP_ERR_EXEC() -> &'static str {
    s("Command failed: {}", "命令执行失败: {}")
}
pub fn SSH_CMD_STOP() -> &'static str {
    s("Stop", "停止")
}
pub fn SSH_CMD_RUNNING() -> &'static str {
    s("Running…", "运行中…")
}
pub fn SSH_CMD_STOP_TIP() -> &'static str {
    s(
        "Force stop the running command (use when the server hangs)",
        "强制中断当前命令（服务器卡死时使用）",
    )
}
pub fn SSH_CMD_ABORTED() -> &'static str {
    s(
        "Command interrupted by user. Note: if the SSH connection itself hung, the remote process may still run — reconnect and check if needed.",
        "命令已被手动中断。注意：若 SSH 连接本身已卡死，远端进程可能仍在运行，必要时请断开重连确认。",
    )
}
pub fn SSH_CMD_ADD() -> &'static str {
    s("Add Custom Command", "添加自定义命令")
}
pub fn SSH_CMD_ADD_BTN() -> &'static str {
    s("＋ Add Command", "＋ 添加命令")
}
pub fn SSH_CMD_ADD_HINT() -> &'static str {
    s(
        "Saved on this device and restored next launch.",
        "命令将保存到本机，下次打开仍可用。",
    )
}
pub fn SSH_CMD_REMOVE_TIP() -> &'static str {
    s("Remove this command", "删除该命令")
}
pub fn SSH_TPL_TITLE() -> &'static str {
    s("Command templates (click to fill)", "常用命令模板（点击填入）")
}
pub fn SSH_TPL_SYS() -> &'static str {
    s("System", "系统状态")
}
pub fn SSH_TPL_SVC() -> &'static str {
    s("Services", "服务管理")
}
pub fn SSH_TPL_SEC() -> &'static str {
    s("Security Hardening", "安全增强")
}
pub fn SSH_TPL_APP() -> &'static str {
    s("Deploy / Containers", "部署 / 容器")
}
pub fn SSH_TPL_DEPLOY() -> &'static str {
    s("App Environment Deploy", "应用环境部署")
}
pub fn SSH_TPL_NET() -> &'static str {
    s("Network", "网络")
}
pub fn SSH_TPL_LOG() -> &'static str {
    s("Logs", "日志查看")
}
pub fn SSH_TPL_DB() -> &'static str {
    s("Databases", "数据库")
}
pub fn SSH_TPL_RUNTIME() -> &'static str {
    s("Runtimes / Languages", "运行时/语言")
}
pub fn SSH_TPL_MY() -> &'static str {
    s("My Commands", "我的命令")
}

// ────────────────────── 远程检测页 ──────────────────────
pub fn REMOTE_CONNECT() -> &'static str {
    s("Connect", "连接")
}
pub fn REMOTE_DIALOG_TITLE() -> &'static str {
    s("Connect Remote URL", "连接远程地址")
}
pub fn REMOTE_URL_LABEL() -> &'static str {
    s("URL", "网址")
}
pub fn REMOTE_URL_HINT() -> &'static str {
    s("https://example.com:8443", "https://example.com:8443")
}
pub fn REMOTE_URL_SUPPORT() -> &'static str {
    s(
        "Supports http / https / ws / wss / ftp; port optional, e.g. https://example.com:8443. No scheme defaults to https.",
        "支持 http / https / ws / wss / ftp 协议；端口可一并填写，如 https://example.com:8443。不填协议前缀时默认按 https 处理。",
    )
}
pub fn REMOTE_PING() -> &'static str {
    s("Ping", "Ping")
}
pub fn REMOTE_SSL() -> &'static str {
    s("SSL Certificate", "SSL证书状态")
}
pub fn REMOTE_WEB() -> &'static str {
    s("Web Status", "网页状态")
}
pub fn REMOTE_ENDPOINT() -> &'static str {
    s("Endpoint", "当前端点")
}
pub fn REMOTE_CARD_RESULT() -> &'static str {
    s("Result", "检测结果")
}
pub fn REMOTE_RESULT_HINT() -> &'static str {
    s("Ping / SSL / web status results", "Ping / SSL / 网页状态检测结果")
}
pub fn REMOTE_CONNECTED() -> &'static str {
    s("Connected {} RTT {} ms IP {}", "已连接 {} RTT {} ms IP {}")
}
pub fn REMOTE_PING_OK() -> &'static str {
    s(
        "Ping OK, min {} / avg {} / max {} ms ({} probes)",
        "Ping 正常，最小 {} / 平均 {} / 最大 {} ms（{} 次）",
    )
}
pub fn REMOTE_BUSY() -> &'static str {
    s("Checking...", "检测中…")
}
pub fn REMOTE_SSL_TITLE() -> &'static str {
    s("SSL Certificate", "SSL 证书信息")
}
pub fn REMOTE_SSL_SUBJECT() -> &'static str {
    s("Subject:", "主题:")
}
pub fn REMOTE_SSL_ISSUER() -> &'static str {
    s("Issuer:", "签发者:")
}
pub fn REMOTE_SSL_VALID() -> &'static str {
    s("Valid from {} to {}", "有效期 {} 至 {}")
}
pub fn REMOTE_SSL_DAYS() -> &'static str {
    s("{} days left", "剩余 {} 天")
}
pub fn REMOTE_SSL_EXPIRED() -> &'static str {
    s("Expired {} days ago", "已过期 {} 天")
}
pub fn REMOTE_WEB_OK() -> &'static str {
    s("HTTP {} {} {} bytes {} ms", "HTTP {} {} {} 字节 {} ms")
}
pub fn REMOTE_WEB_REDIRECTS() -> &'static str {
    s("Redirects:", "重定向:")
}
pub fn REMOTE_WEB_FINAL() -> &'static str {
    s("Final URL:", "最终地址:")
}
pub fn REMOTE_WEB_CTYPE() -> &'static str {
    s("Content-Type:", "类型:")
}
pub fn REMOTE_WEB_SERVER() -> &'static str {
    s("Server:", "服务器:")
}
pub fn REMOTE_QR() -> &'static str {
    s("QR Code", "二维码")
}
pub fn REMOTE_QR_TITLE() -> &'static str {
    s("URL QR Code", "网址二维码")
}
pub fn REMOTE_QR_GEN() -> &'static str {
    s("Generate", "生成")
}
pub fn REMOTE_QR_EMPTY() -> &'static str {
    s("Please enter a URL first", "请先输入网址")
}
pub fn REMOTE_QR_SUPPORT() -> &'static str {
    s(
        "Connected URL is pre-filled; edit it and click Generate.",
        "已连接时默认带入连接网址，可修改后生成。",
    )
}
pub fn REMOTE_NOT_TLS() -> &'static str {
    s(
        "Not an encrypted protocol (TLS check requires https/wss)",
        "非加密协议，TLS 检查仅支持 https/wss",
    )
}
pub fn REMOTE_WEB_HTTP_ONLY() -> &'static str {
    s(
        "Web status only supports http/https protocols",
        "网页状态仅支持 http/https 协议",
    )
}
pub fn REMOTE_ERR_EMPTY() -> &'static str {
    s("URL is empty", "网址为空")
}
pub fn REMOTE_ERR_PARSE() -> &'static str {
    s("Invalid URL: {}", "网址解析失败: {}")
}
pub fn REMOTE_ERR_SCHEME() -> &'static str {
    s("Unsupported scheme: {}", "不支持的协议: {}")
}
pub fn REMOTE_ERR_HOST() -> &'static str {
    s("Missing host", "缺少主机名")
}
pub fn REMOTE_ERR_DNS() -> &'static str {
    s("DNS resolution failed: {}", "域名解析失败: {}")
}
pub fn REMOTE_ERR_CONNECT() -> &'static str {
    s("Connect failed: {}", "连接失败: {}")
}
pub fn REMOTE_ERR_TLS() -> &'static str {
    s("TLS handshake failed: {}", "TLS 握手失败: {}")
}
pub fn REMOTE_ERR_NO_CERT() -> &'static str {
    s("No certificate presented", "未返回证书")
}
pub fn REMOTE_ERR_CERT() -> &'static str {
    s("Certificate parse failed: {}", "证书解析失败: {}")
}
pub fn REMOTE_ERR_HTTP() -> &'static str {
    s("HTTP request failed: {}", "HTTP 请求失败: {}")
}
pub fn REMOTE_ERR_BAD_RESPONSE() -> &'static str {
    s("Malformed HTTP response", "HTTP 响应格式错误")
}
pub fn REMOTE_ERR_READ() -> &'static str {
    s("Read failed: {}", "读取失败: {}")
}
pub fn REMOTE_PANIC() -> &'static str {
    s("Unknown panic", "未知panic")
}

// ────────────────────── 关于 ──────────────────────
pub fn ABOUT_DESC() -> &'static str {
    s(
        "Open-source all-in-one website management toolkit with the following features:",
        "开源一站式网站管理工具箱，提供以下功能：",
    )
}
pub fn ABOUT_2FA() -> &'static str {
    s(
            "2FA Authenticator  -  Generate TOTP keys & QR codes, compatible with Google Authenticator / Authy",
            "2FA 验证码生成  -  生成 TOTP 密钥与二维码，兼容 Google Authenticator / Authy",
        )
}
pub fn ABOUT_PWD() -> &'static str {
    s(
        "Password Generator  -  Supports Argon2id / Bcrypt / PBKDF2, with SQL UPDATE output",
        "账号密码生成  -  支持 Argon2id / Bcrypt / PBKDF2 三种算法，附带 SQL 更新语句",
    )
}
pub fn ABOUT_TURSO() -> &'static str {
    s(
        "Turso Database Viewer  -  Browse local Turso/libSQL databases, view tables and data",
        "Turso 数据库浏览  -  连接本地 Turso/libSQL 数据库，查看表结构与数据",
    )
}
pub fn ABOUT_REMOTE() -> &'static str {
    s(
        "Remote Check  -  ping / SSL certificate / web status",
        "远程检测  -  Ping / SSL 证书 / 网页状态",
    )
}
pub fn ABOUT_SFTP() -> &'static str {
    s(
        "SFTP File Manager  -  upload / download / remote command",
        "SFTP 文件管理  -  上传 / 下载 / 远程命令",
    )
}
pub fn ABOUT_BUILT() -> &'static str {
    s("Built with Rust + windui", "基于 Rust + windui 构建")
}
pub fn ABOUT_LICENSE_TITLE() -> &'static str {
    s("Open Source & License", "开源与许可")
}
pub fn ABOUT_LICENSE() -> &'static str {
    s(
        "Licensed under the MIT License (© 2026 wujianqi); full terms in the repository's LICENSE file.",
        "本项目基于 MIT License 开源（© 2026 wujianqi）；完整条款见仓库 LICENSE 文件。",
    )
}
pub fn ABOUT_ICONS() -> &'static str {
    s(
        "Some UI icons are styled after Lucide (MIT License, lucide.dev).",
        "部分 UI 图标参考 Lucide 风格绘制（MIT 许可，lucide.dev）。",
    )
}
pub fn ABOUT_THIRD_PARTY() -> &'static str {
    s(
        "Third-party library licenses: see Cargo.toml / Cargo.lock and each project's repository.",
        "第三方依赖许可：见 Cargo.toml / Cargo.lock 与各依赖项目的仓库说明。",
    )
}

// ────────────────────── 密码页 ──────────────────────
pub fn PWD_CARD_SETUP() -> &'static str {
    s("Generate Settings", "生成配置")
}
pub fn PWD_CARD_OUTPUT() -> &'static str {
    s("Output", "输出结果")
}
pub fn PWD_PLATFORM() -> &'static str {
    s("Target Platform:", "目标平台：")
}
pub fn PWD_ALGO() -> &'static str {
    s("Algorithm:", "加密算法：")
}
pub fn PWD_GEN_8() -> &'static str {
    s("8 chars", "8位")
}
pub fn PWD_GEN_12() -> &'static str {
    s("12 chars", "12位")
}
pub fn PWD_GEN_16() -> &'static str {
    s("16 chars", "16位")
}
pub fn PWD_INPUT_LABEL() -> &'static str {
    s("Password to encrypt:", "待加密密码：")
}
pub fn PWD_INPUT_HINT() -> &'static str {
    s(
        "Enter or click a button above to generate...",
        "在此输入或点击上方按钮生成密码...",
    )
}
pub fn PWD_ENCRYPT() -> &'static str {
    s("Encrypt", "加密")
}
pub fn PWD_OUTPUT_LABEL() -> &'static str {
    s("Encrypted password:", "加密的密码：")
}
pub fn PWD_SQL_LABEL() -> &'static str {
    s("SQL UPDATE statement:", "SQL更新语句：")
}

// ────────────────────── 平台预设标签 ──────────────────────
pub fn PLAT_NONE() -> &'static str {
    s("Custom", "自定义")
}
pub fn PLAT_LARAVEL() -> &'static str {
    s("Laravel (PHP)", "Laravel (PHP)")
}
pub fn PLAT_DJANGO() -> &'static str {
    s("Django (Python)", "Django (Python)")
}
pub fn PLAT_SPRING() -> &'static str {
    s("Spring Boot (Java)", "Spring Boot (Java)")
}
pub fn PLAT_EXPRESS() -> &'static str {
    s("Express (Node.js)", "Express (Node.js)")
}
pub fn PLAT_DOTNET() -> &'static str {
    s("ASP.NET Core (.NET)", "ASP.NET Core (.NET)")
}
pub fn PLAT_RAILS() -> &'static str {
    s("Rails (Ruby)", "Rails (Ruby)")
}
pub fn PLAT_WORDPRESS() -> &'static str {
    s("WordPress (PHP)", "WordPress (PHP)")
}
pub fn PLAT_GO() -> &'static str {
    s("Go (Golang)", "Go (Golang)")
}

// ────────────────────── TOTP 页 ──────────────────────
pub fn TOTP_CARD_SETUP() -> &'static str {
    s("Key Setup", "密钥配置")
}
pub fn TOTP_CARD_OUTPUT() -> &'static str {
    s("Code Output", "验证码输出")
}
pub fn TOTP_GEN_KEY() -> &'static str {
    s("Generate Key", "生成密钥")
}
pub fn TOTP_GEN_HINT() -> &'static str {
    s(
        "(Generate a secure key compatible with major platforms)",
        "(生成兼容主流平台的安全密钥)",
    )
}
pub fn TOTP_KEY_LABEL() -> &'static str {
    s("Base32 Key,", "Base32密钥，")
}
pub fn TOTP_KEY_WARN() -> &'static str {
    s(
        "Keep your active key safe. Leaking it will severely compromise security.",
        "已实际应用的密钥请妥善保存，泄漏将严重影响安全。",
    )
}
pub fn TOTP_ACCOUNT() -> &'static str {
    s("Account Name:", "账号名：")
}
pub fn TOTP_ACCOUNT_HINT() -> &'static str {
    s("e.g. user@example.com", "用户账号名")
}
pub fn TOTP_ISSUER() -> &'static str {
    s("Issuer:", "发行方：")
}
pub fn TOTP_ISSUER_HINT() -> &'static str {
    s("Service / App name", "服务/应用名称")
}
pub fn TOTP_KEY_INPUT_HINT() -> &'static str {
    s(
        "Enter or click a button above to generate a Base32 key...",
        "在此输入或点击上方按钮生成Base32密钥...",
    )
}
pub fn TOTP_GENERATE() -> &'static str {
    s("Generate", "生成")
}
pub fn TOTP_SAVE() -> &'static str {
    s("Save", "保存")
}
pub fn TOTP_SAVED() -> &'static str {
    s("Key saved\nCurrent key: {}", "密钥已保存\n当前密钥: {}")
}
pub fn TOTP_SAVE_FAIL() -> &'static str {
    s("Save failed: {}", "保存失败: {}")
}
pub fn TOTP_QR() -> &'static str {
    s("QR Code", "二维码")
}
pub fn TOTP_SCAN_HINT() -> &'static str {
    s(
        "Scan this QR code to set up Google Authenticator or Authy",
        "扫描此二维码配置Google Authenticator或Authy",
    )
}

// ────────────────────── Turso 页 ──────────────────────
pub fn TURSO_OPEN() -> &'static str {
    s("Open", "打开")
}
pub fn TURSO_FILE_TITLE() -> &'static str {
    s("Select Turso database file", "选择Turso数据库文件")
}
pub fn TURSO_FILE_FILTER1() -> &'static str {
    s("SQLite/Turso databases", "SQLite/Turso数据库")
}
pub fn TURSO_FILE_FILTER2() -> &'static str {
    s("All files", "所有文件")
}
pub fn TURSO_MODE_LOCAL() -> &'static str {
    s("Local File", "本地文件")
}
pub fn TURSO_MODE_REMOTE() -> &'static str {
    s("Network", "网络连接")
}
pub fn TURSO_URL() -> &'static str {
    s("Connection URL", "连接地址")
}
pub fn TURSO_URL_BTN() -> &'static str {
    s("URL", "地址")
}
pub fn TURSO_URL_HINT() -> &'static str {
    s(
        "e.g. libsql://xxx.turso.io or https://...",
        "如 libsql://xxx.turso.io 或 https://...",
    )
}
pub fn TURSO_TOKEN_HINT() -> &'static str {
    s("Auth token (optional)", "访问令牌(可留空)")
}
pub fn TURSO_CONNECT() -> &'static str {
    s("Connect", "连接")
}
pub fn TURSO_DISCONNECT() -> &'static str {
    s("Disconnect", "断开")
}
pub fn TURSO_CONNECTED() -> &'static str {
    s("\u{2705} {} | {} tables", "\u{2705} {} | {} 个表")
}
pub fn TURSO_REFRESHED() -> &'static str {
    s("\u{2705} Refreshed", "\u{2705} 已刷新")
}
pub fn TURSO_REFRESH() -> &'static str {
    s("Refresh", "刷新")
}
pub fn TURSO_OPEN_SQL() -> &'static str {
    s("SQL Query", "SQL查询")
}
pub fn TURSO_SQL_GROUP() -> &'static str {
    s("SQL Query", "SQL 查询")
}
pub fn TURSO_SQL_EXEC() -> &'static str {
    s("Execute", "执行")
}
pub fn TURSO_SQL_STATUS() -> &'static str {
    s("\u{2705} {} cols, {} rows", "\u{2705} {} 列, {} 行")
}
pub fn TURSO_SQL_CLEAR() -> &'static str {
    s("Clear", "清空")
}
pub fn TURSO_UNKNOWN_PANIC() -> &'static str {
    s("Unknown panic", "未知panic")
}

// ────────────────────── DataTable ──────────────────────
pub fn DT_TABLE_LIST() -> &'static str {
    s("Tables ({})", "表列表 ({})")
}
pub fn DT_NO_TABLE() -> &'static str {
    s(
        "Connect to a database and select a table first",
        "请先连接数据库并选择一个表",
    )
}
pub fn DT_COL_ROW() -> &'static str {
    s("| {} cols | {} rows", "| {} 列 | {} 行")
}
pub fn DT_COL_MORE() -> &'static str {
    s("… +{} cols", "… +{} 列")
}
pub fn DT_COLS() -> &'static str {
    s("Columns", "列设置")
}
pub fn DT_PREV() -> &'static str {
    s("Prev", "上一页")
}
pub fn DT_PAGE() -> &'static str {
    s("Page {} / {}", "第 {} / {} 页")
}
pub fn DT_NEXT() -> &'static str {
    s("Next", "下一页")
}
pub fn DT_CLOSE() -> &'static str {
    s("Close", "关闭")
}
pub fn DT_BACK() -> &'static str {
    s("Back", "返回列表")
}
pub fn DT_ROW_DETAIL() -> &'static str {
    s("Row {} Detail", "行 {} 详情")
}

// ────────────────────── 错误信息 ──────────────────────
pub fn ERR_RUNTIME() -> &'static str {
    s("Failed to create runtime: {}", "创建运行时失败: {}")
}
pub fn ERR_CONNECT_DB() -> &'static str {
    s("Failed to connect to database: {}", "连接数据库失败: {}")
}
pub fn ERR_GET_CONN() -> &'static str {
    s("Failed to get connection: {}", "获取连接失败: {}")
}
pub fn ERR_THREAD_PANIC() -> &'static str {
    s("Thread panic: {}", "线程panic: {}")
}
pub fn ERR_THREAD_PANIC_UNKNOWN() -> &'static str {
    s("Thread panic (unknown)", "线程panic(未知)")
}
pub fn ERR_QUERY_TABLES() -> &'static str {
    s("Failed to query tables: {}", "查询表列表失败: {}")
}
pub fn ERR_TABLE_INFO() -> &'static str {
    s("Failed to get table info: {}", "获取表结构失败: {}")
}
pub fn ERR_QUERY_DATA() -> &'static str {
    s("Failed to query data: {}", "查询表数据失败: {}")
}
pub fn ERR_EMPTY_SQL() -> &'static str {
    s("SQL statement cannot be empty", "SQL 语句不能为空")
}
pub fn ERR_QUERY_FAIL() -> &'static str {
    s("Query failed: {}", "查询失败: {}")
}
pub fn ERR_EXEC_FAIL() -> &'static str {
    s("Execution failed: {}", "执行失败: {}")
}
pub fn ERR_SALT() -> &'static str {
    s("Failed to generate salt: {}", "生成随机盐失败: {}")
}
pub fn ERR_ARGON2_PARAM() -> &'static str {
    s("Argon2 parameter error: {}", "Argon2 参数错误: {}")
}
pub fn ERR_ARGON2_HASH() -> &'static str {
    s("Argon2 hashing failed: {}", "Argon2 加密失败: {}")
}
pub fn ERR_BCRYPT_HASH() -> &'static str {
    s("Bcrypt hashing failed: {}", "Bcrypt 加密失败: {}")
}

// ────────────────────── 通用（可选文本右键菜单）──────────────────────
pub fn MENU_COPY() -> &'static str {
    s("Copy", "复制")
}
pub fn MENU_SELECT_ALL() -> &'static str {
    s("Select All", "全选")
}
