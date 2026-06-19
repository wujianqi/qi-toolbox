# Qi Toolbox

---

[English](#english) | [中文](#中文)

---

<a name="中文"></a>

## 中文

轻量级桌面工具集，基于 Rust + egui 构建，提供 2FA 验证码生成、账号密码加密、Turso 数据库浏览三大功能。

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

### 预编译下载

前往 [Releases](https://github.com/wujianqi/qi-toolbox/releases) 页面下载最新版本。

- `qi-toolbox_cn.exe` — 中文版
- `qi-toolbox_en.exe` — 英文版

### 从源码编译

#### 前提条件

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

#### 编译

```bash
# Debug 版本
cargo build

# Release 版本（体积优化）
cargo build --release

# 英文版
cargo build --release --features english
```

#### 一键构建双语发布版

```powershell
# Windows (PowerShell)
.\build_release.ps1

# Linux / macOS
bash build_release.sh
```

产物输出到 `release/` 目录：

```
release/
├── qi-toolbox_cn.exe    # 中文版
├── qi-toolbox_en.exe    # 英文版
└── README.md
```

### 项目结构

```
src/
├── main.rs           # 入口，窗口初始化
├── ui.rs             # 界面布局与交互逻辑
├── strings.rs        # 国际化字符串（中文 / 英文）
├── sql_editor.rs     # SQL 语法高亮编辑器
├── password.rs       # 密码哈希算法（Argon2id / Bcrypt / PBKDF2）
├── totp.rs           # TOTP 密钥生成、验证码计算、二维码生成
├── turso_viewer.rs   # Turso/libSQL 数据库连接与查询
└── datatable.rs      # 数据表格渲染（表列表、分页、详情视图）
```

### 技术栈

- [Rust](https://www.rust-lang.org/) — 系统语言
- [egui](https://github.com/emilk/egui) — 即时模式 GUI 框架
- [totp-rs](https://github.com/constverif/totp-rs) — TOTP 算法实现
- [argon2](https://github.com/RustCrypto/password-hashes) — Argon2id 哈希
- [bcrypt](https://github.com/Keats/rust-bcrypt) — Bcrypt 加密
- [pbkdf2](https://github.com/RustCrypto/password-hashes) — PBKDF2 哈希
- [turso](https://github.com/tursodatabase/turso-client-rust) — Turso/libSQL 客户端
- [qrcode](https://github.com/mynanism/qrcode-rust) — 二维码生成

### 许可证

[MIT License](LICENSE)

---

<a name="english"></a>

## English

Lightweight desktop toolbox built with Rust + egui, providing 2FA authenticator, password hashing, and Turso database browsing.

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

### Download

Visit the [Releases](https://github.com/wujianqi/qi-toolbox/releases) page.

- `qi-toolbox_cn.exe` — Chinese version
- `qi-toolbox_en.exe` — English version

### Build from Source

#### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

#### Build

```bash
# Debug
cargo build

# Release (optimized)
cargo build --release

# English version
cargo build --release --features english
```

#### Build Both Languages for Release

```powershell
# Windows (PowerShell)
.\build_release.ps1

# Linux / macOS
bash build_release.sh
```

Output in `release/`:

```
release/
├── qi-toolbox_cn.exe    # Chinese
├── qi-toolbox_en.exe    # English
└── README.md
```

### Project Structure

```
src/
├── main.rs           # Entry point, window setup
├── ui.rs             # UI layout and interaction
├── strings.rs        # Internationalization strings (zh / en)
├── sql_editor.rs     # SQL syntax highlighting editor
├── password.rs       # Password hashing (Argon2id / Bcrypt / PBKDF2)
├── totp.rs           # TOTP key generation, code calculation, QR codes
├── turso_viewer.rs   # Turso/libSQL database connection and queries
└── datatable.rs      # Data table rendering (list, pagination, detail view)
```

### Tech Stack

- [Rust](https://www.rust-lang.org/) — Systems language
- [egui](https://github.com/emilk/egui) — Immediate-mode GUI framework
- [totp-rs](https://github.com/constverif/totp-rs) — TOTP algorithm
- [argon2](https://github.com/RustCrypto/password-hashes) — Argon2id hashing
- [bcrypt](https://github.com/Keats/rust-bcrypt) — Bcrypt encryption
- [pbkdf2](https://github.com/RustCrypto/password-hashes) — PBKDF2 hashing
- [turso](https://github.com/tursodatabase/turso-client-rust) — Turso/libSQL client
- [qrcode](https://github.com/mynanism/qrcode-rust) — QR code generation

### License

[MIT License](LICENSE)
