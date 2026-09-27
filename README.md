# 奇兔宝 (Qi Toolbox)

> **小身材，大管家 —— 你的随身运维百宝箱。**

---

[English](#english) | [中文](#中文)

---

<a name="中文"></a>

## 中文

轻量级一站式服务器快捷管理工具箱，基于 Rust + windui 构建的原生 Windows 桌面应用，集 2FA 验证码生成、账号密码加密、数据库浏览（Turso / MySQL / PostgreSQL）、SFTP 文件管理、S3 对象存储浏览、远程检测（连接 / Ping / SSL 证书 / 响应体 / SEO 分析 / 安全检测）于一体。

> **特点**：单文件绿色版、零安装、无后台进程；敏感数据（TOTP 密钥、SFTP 密码等）本地加密存储（Windows 走 DPAPI）；内置中英双语，跟随系统语言启动，可在应用内一键切换。

### ✨ 亮点

- **一个 exe 走天下**：单文件绿色版，下载即用，U 盘可携带，不写注册表、无后台进程
- **数据只属于你**：TOTP 密钥、SFTP 密码等敏感信息全部本地加密存储（Windows DPAPI）
- **从密码到上线的完整链路**：生成密码 → 哈希加密 → 生成 SQL → SFTP 部署 → 远程检测，一个工具贯穿全流程
- **原生性能**：Rust 构建，原生 Windows GUI，启动即开即用，无 Electron 式臃肿
- **安全体检一体化**：SSL 证书 / 安全响应头 / 敏感路径 / 风险端口 / TLS 旧版本，一键扫描
- **中英双语随行**：跟随系统语言启动，应用内一键切换

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

#### 数据库浏览（Turso / MySQL / PostgreSQL）

- 连接本地 Turso / libSQL / SQLite 数据库文件，或远程 MySQL / PostgreSQL 数据库
- 多库址管理：保存常用连接，快速切换
- 浏览所有表及表结构
- 数据表格展示，支持列过滤和分页
- 内置 SQL 查询编辑器，支持语法高亮
- 数据导出 CSV，行详情查看模式

#### S3 对象存储浏览

- 连接 S3 兼容对象存储（AWS S3、MinIO、Cloudflare R2 等）
- Bucket 与对象列表浏览，路径式 / 虚拟主机式寻址可选
- 对象上传 / 下载 / 删除

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
- 响应体：HTTP 状态码 / 响应大小 / 耗时 + 响应体预览，自动跟随重定向（仅 http/https）
- SEO 分析：title / description / keywords / canonical / viewport / H1 / 图片 alt 一目了然（仅 http/https）
- 安全检测：敏感路径暴露 / 安全响应头缺失 / 风险端口开放 / TLS 旧版本兼容，一键扫描分级报告（仅 http/https）
- 端点二维码：一键生成当前地址二维码，扫码即可分发

### 界面截图

> 截图存放于 `docs/screenshots/`，随版本发布更新。

| 模块 | 截图 |
|---|---|
| 2FA TOTP 验证码 | ![TOTP](docs/screenshots/totp.png) |
| 数据库浏览 | ![数据库](docs/screenshots/database.png) |
| SFTP 文件管理 | ![SFTP](docs/screenshots/sftp.png) |
| 远程检测 | ![远程检测](docs/screenshots/remote.png) |
| 密码加密 | ![密码](docs/screenshots/password.png) |
| S3 对象存储 | ![S3](docs/screenshots/s3.png) |

### 预编译下载

前往 [Releases](https://github.com/wujianqi/qi-toolbox/releases) 页面下载最新版本。

- `qi-toolbox.exe` — 单包双语言：启动跟随系统语言（中文系统→中文，否则英文），可在主面板侧栏底部「中 / EN」手动切换

### 从源码编译

#### 前提条件

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

> 注：项目目前主要面向 Windows（windui 为 Windows 原生 GUI 框架），其他平台未经充分测试。

#### 编译

```bash
# Debug 版本
cargo build

# Release 版本（体积优化）
cargo build --release
```

产物位于 `target/release/qi-toolbox.exe`。

### 许可证

[MIT License](LICENSE)

---

<a name="english"></a>

## English

Lightweight one-stop server management toolbox — a native Windows desktop app built with Rust + windui, combining a 2FA authenticator, password hashing, database viewers (Turso / MySQL / PostgreSQL), an SFTP file manager, an S3 object storage browser, and remote checks (connect / ping / SSL certificate / response body / SEO analysis / security scan).

> **Highlights**: single-file portable build, zero install, no background services; sensitive data (TOTP secrets, SFTP passwords, etc.) is stored locally with platform encryption (DPAPI on Windows); built-in zh/en bilingual UI follows the system language and can be switched in-app.

### ✨ Highlights

- **One exe for everything**: single-file portable build, download and run, USB-stick friendly, no registry writes, no background processes
- **Your data stays yours**: TOTP secrets, SFTP passwords and other sensitive data are stored locally with encryption (DPAPI on Windows)
- **Full workflow from password to production**: generate password → hash → SQL → SFTP deploy → remote check, all in one tool
- **Native performance**: built with Rust and a native Windows GUI — instant startup, no Electron bloat
- **One-click security scan**: SSL certificate / security headers / sensitive paths / risky ports / legacy TLS, with a graded report
- **Bilingual out of the box**: follows system language, one-click switch in-app

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

#### Database Viewer (Turso / MySQL / PostgreSQL)

- Connect to local Turso / libSQL / SQLite files, or remote MySQL / PostgreSQL databases
- Multi-connection management: save frequently used connections for quick switching
- Browse all tables and schema
- Data grid with column filtering and pagination
- Built-in SQL query editor with syntax highlighting
- CSV export and row detail view

#### S3 Object Storage Browser

- Connect to S3-compatible object storage (AWS S3, MinIO, Cloudflare R2, etc.)
- Bucket / object listing with path-style or virtual-host addressing
- Object upload / download / delete

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
- Web body: HTTP status code / size / elapsed + response body preview, follows redirects (http/https only)
- SEO analysis: title / description / keywords / canonical / viewport / H1 / image alt at a glance (http/https only)
- Security scan: sensitive path exposure / missing security headers / risky open ports / legacy TLS support, one-click graded report (http/https only)
- Endpoint QR code: one-click QR for the current URL, scan to share

### Screenshots

> Screenshots live in `docs/screenshots/` and are updated with each release.

| Module | Screenshot |
|---|---|
| 2FA TOTP Authenticator | ![TOTP](docs/screenshots/totp.png) |
| Database Viewer | ![Database](docs/screenshots/database.png) |
| SFTP File Manager | ![SFTP](docs/screenshots/sftp.png) |
| Remote Check | ![Remote](docs/screenshots/remote.png) |
| Password Hashing | ![Password](docs/screenshots/password.png) |
| S3 Object Storage | ![S3](docs/screenshots/s3.png) |

### Download

Visit the [Releases](https://github.com/wujianqi/qi-toolbox/releases) page.

- `qi-toolbox.exe` — Single package with built-in zh/en: follows the system language on startup (Chinese system → Chinese, otherwise English), switchable via the 中/EN toggle at the sidebar bottom

### Build from Source

#### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

> 注：项目目前主要面向 Windows（windui 为 Windows 原生 GUI 框架），其他平台未经充分测试。

#### Build

```bash
# Debug
cargo build

# Release (optimized)
cargo build --release
```

The binary is at `target/release/qi-toolbox.exe`.

### License

[MIT License](LICENSE)
