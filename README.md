# Qi Toolbox

---

[English](#english) | [中文](#中文)

---

<a name="中文"></a>

## 中文

轻量级桌面工具集，基于 Rust + windui 构建，提供 2FA 验证码生成、账号密码加密、Turso 数据库浏览、SFTP 文件管理、远程检测五大功能。

### 功能

#### 2FA TOTP 验证码生成

- 生成兼容主流平台的 Base32 安全密钥
- 实时生成 6 位 TOTP 验证码（SHA1, 30s 步长）
- 生成 otpauth:// 二维码，可直接扫码配置 Google Authenticator / Authy
- 密钥本地保存，方便重复使用

#### 账号密码加密

- 支持 Argon2id / Bcrypt / PBKDF2 三种主流哈希算法
- 平台预设：Laravel、Django、Spring Boot、Express、ASP.NET Core、Rails、WordPress、Go
- 一键生成 8/12/16 位随机密码（包含大小写字母、数字、特殊字符）
- 自动生成 SQL UPDATE 语句，方便直接应用到数据库

#### Turso 数据库浏览

- 连接本地 Turso / libSQL / SQLite 数据库文件
- 浏览所有表及表结构
- 数据表格展示，支持列过滤和分页
- 内置 SQL 查询编辑器，支持语法高亮
- 行详情查看模式

#### SFTP 文件管理

- SSH 密码认证连接，文件列表浏览（目录优先排序）
- 上传 / 下载 / 新建文件夹 / 删除，大文件分块流式传输
- 复用 SSH 会话执行远程命令
- 内置部署 / 维护常用命令模板（系统状态、服务管理、部署容器、网络），点选即填入命令框

#### 远程检测

- 支持 http / https / ws / wss / ftp 地址自动解析（缺省按 https，端口可携带）
- 连接：TCP 建连（加密协议自动 TLS 握手），返回 RTT 与解析到的 IP
- Ping：连测 3 次，输出最小 / 平均 / 最大时延
- SSL 证书状态：证书链校验 + 主题 / 签发者 / 有效期 / 剩余天数（仅 https/wss）
- 网页状态：HTTP 状态码 / 响应大小 / 耗时，自动跟随重定向（仅 http/https）
- 端点二维码：一键生成当前地址二维码，扫码即可分发

### 预编译下载

前往 [Releases](https://github.com/wujianqi/qi-toolbox/releases) 页面下载最新版本。

- `qi-toolbox.exe` — 单包双语言：启动跟随系统语言（中文系统→中文，否则英文），可在主面板侧栏底部「中 / EN」手动切换

### 从源码编译

#### 前提条件

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

#### 编译

```bash
# Debug 版本
cargo build

# Release 版本（体积优化）
cargo build --release
```

#### 一键构建发布版

```powershell
# Windows (PowerShell)
.\build_release.ps1

# Linux / macOS
bash build_release.sh
```

产物输出到 `release/` 目录：

```
release/
├── qi-toolbox.exe    # 单包双语言（跟随系统，可在应用内切换）
└── README.md
```

### 项目结构

分层设计：`core/` 业务层（纯逻辑，不依赖 UI 框架）+ `ui/` 界面层（windui），板块按页独立，便于扩展。

```
src/
├── main.rs           # 入口：平台初始化 + 启动 UI
├── lang.rs           # 国际化文案（中文 / 英文，运行时切换）
├── core/             # 业务层（不依赖 windui）
│   ├── totp.rs       # TOTP 密钥生成、验证码计算、二维码数据
│   ├── password.rs   # 密码哈希（Argon2id / Bcrypt / PBKDF2）
│   ├── sftp.rs       # SFTP 工作线程（SSH/SFTP 命令协议）
│   ├── turso.rs      # Turso/libSQL 数据库连接与查询
│   ├── db.rs         # 数据库后台任务编排（消息协议 + 线程）
│   └── remote.rs     # 远程检测（URL 解析 / Ping / SSL / 网页状态）
└── ui/               # 界面层（windui，仅渲染与交互）
    ├── mod.rs        # 入口 + AppState（页面状态聚合）
    ├── widgets.rs    # 共享组件（卡片 / 导航 / 主题切换 / 关于）
    ├── table.rs      # 数据表格渲染（表列表 / 分页 / 详情视图）
    ├── sql.rs        # SQL 查询面板
    ├── totp.rs       # TOTP 页
    ├── password.rs   # 密码页
    ├── turso.rs      # Turso 页
    ├── sftp.rs       # SFTP 页
    └── remote.rs     # 远程检测页
```

### 技术栈

- [Rust](https://www.rust-lang.org/) — 系统语言
- [windui](https://crates.io/crates/windui) — Windows 原生 GUI 框架
- [totp-rs](https://github.com/constverif/totp-rs) — TOTP 算法实现
- [argon2](https://github.com/RustCrypto/password-hashes) — Argon2id 哈希
- [bcrypt](https://github.com/Keats/rust-bcrypt) — Bcrypt 加密
- [pbkdf2](https://github.com/RustCrypto/password-hashes) — PBKDF2 哈希
- [turso](https://github.com/tursodatabase/turso-client-rust) — Turso/libSQL 客户端
- [qrcode](https://github.com/mynanism/qrcode-rust) — 二维码生成
- [russh](https://github.com/warp-tech/russh) — SSH / SFTP 客户端
- [native-tls](https://github.com/sfackler/rust-native-tls) — TLS 证书校验

### 许可证

[MIT License](LICENSE)

---

<a name="english"></a>

## English

Lightweight desktop toolbox built with Rust + windui, providing 2FA authenticator, password hashing, Turso database browsing, SFTP file manager, and remote check.

### Features

#### 2FA TOTP Authenticator

- Generate Base32 secret keys compatible with major platforms
- Real-time 6-digit TOTP code generation (SHA1, 30s step)
- Generate otpauth:// QR codes for Google Authenticator / Authy setup
- Save keys locally for reuse

#### Password Hashing

- Supports Argon2id / Bcrypt / PBKDF2 algorithms
- Platform presets: Laravel, Django, Spring Boot, Express, ASP.NET Core, Rails, WordPress, Go
- Generate random passwords (8/12/16 chars) with uppercase, lowercase, digits, and symbols
- Auto-generate SQL UPDATE statements for direct database use

#### Turso Database Viewer

- Connect to local Turso / libSQL / SQLite database files
- Browse all tables and schema
- Data grid with column filtering and pagination
- Built-in SQL query editor with syntax highlighting
- Row detail view

#### SFTP File Manager

- Password-authenticated SSH connections, directory listing (directories first)
- Upload / download / mkdir / delete, chunked streaming for large files
- Run remote commands over the existing SSH session
- Built-in deploy/maintenance command templates (system, services, deploy & containers, network), click to fill the command box

#### Remote Check

- Auto-parse http / https / ws / wss / ftp URLs (defaults to https, port supported)
- Connect: TCP handshake (auto TLS for encrypted schemes), reports RTT and resolved IP
- Ping: 3 probes with min / avg / max latency
- SSL certificate: chain validation + subject / issuer / validity / days left (https/wss only)
- Web status: HTTP status code / size / elapsed, follows redirects (http/https only)
- Endpoint QR code: one-click QR for the current URL, scan to share

### Download

Visit the [Releases](https://github.com/wujianqi/qi-toolbox/releases) page.

- `qi-toolbox.exe` — Single package with built-in zh/en: follows the system language on startup (Chinese system → Chinese, otherwise English), switchable via the 中/EN toggle at the sidebar bottom

### Build from Source

#### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

#### Build

```bash
# Debug
cargo build

# Release (optimized)
cargo build --release
```

#### Build for Release

```powershell
# Windows (PowerShell)
.\build_release.ps1

# Linux / macOS
bash build_release.sh
```

Output in `release/`:

```
release/
├── qi-toolbox.exe    # Single package, zh/en built-in (follows system, switchable in-app)
└── README.md
```

### Project Structure

Layered design: `core/` business layer (pure logic, no UI framework) + `ui/` presentation layer (windui). Each feature lives in its own page module for easy extension.

```
src/
├── main.rs           # Entry point: platform init + launch UI
├── lang.rs           # i18n strings (zh / en, runtime switch)
├── core/             # Business layer (windui-free)
│   ├── totp.rs       # TOTP key generation, code calculation, QR data
│   ├── password.rs   # Password hashing (Argon2id / Bcrypt / PBKDF2)
│   ├── sftp.rs       # SFTP worker thread (SSH/SFTP command protocol)
│   ├── turso.rs      # Turso/libSQL database connection and queries
│   ├── db.rs         # Database background tasks (message protocol + threads)
│   └── remote.rs     # Remote check (URL parse / Ping / SSL / web status)
└── ui/               # Presentation layer (windui, rendering & interaction only)
    ├── mod.rs        # Entry + AppState (page state aggregation)
    ├── widgets.rs    # Shared components (card / nav / theme toggle / about)
    ├── table.rs      # Data table rendering (list / pagination / detail)
    ├── sql.rs        # SQL query panel
    ├── totp.rs       # TOTP page
    ├── password.rs   # Password page
    ├── turso.rs      # Turso page
    ├── sftp.rs       # SFTP page
    └── remote.rs     # Remote check page
```

### Tech Stack

- [Rust](https://www.rust-lang.org/) — Systems language
- [windui](https://crates.io/crates/windui) — Native Windows GUI framework
- [totp-rs](https://github.com/constverif/totp-rs) — TOTP algorithm
- [argon2](https://github.com/RustCrypto/password-hashes) — Argon2id hashing
- [bcrypt](https://github.com/Keats/rust-bcrypt) — Bcrypt encryption
- [pbkdf2](https://github.com/RustCrypto/password-hashes) — PBKDF2 hashing
- [turso](https://github.com/tursodatabase/turso-client-rust) — Turso/libSQL client
- [qrcode](https://github.com/mynanism/qrcode-rust) — QR code generation
- [russh](https://github.com/warp-tech/russh) — SSH / SFTP client
- [native-tls](https://github.com/sfackler/rust-native-tls) — TLS certificate validation

### License

[MIT License](LICENSE)
