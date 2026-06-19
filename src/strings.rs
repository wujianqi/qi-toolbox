//! 国际化字符串 — 通过 Cargo feature "english" 切换语言
//! 默认中文，`cargo build --features english` 编译英文版

// ────────────────────── 通用 ──────────────────────

#[cfg(not(feature = "english"))]
pub mod lang {
    // 标签页
    pub const TAB_2FA: &str = "2FA验证码生成";
    pub const TAB_PASSWORD: &str = "账号密码生成";
    pub const TAB_TURSO: &str = "Turso数据库浏览";

    // 关于
    pub const ABOUT_BTN: &str = "\u{203C} 关于";
    pub const ABOUT_TITLE: &str = "关于";
    pub const ABOUT_DESC: &str = "开源桌面工具集，提供以下功能：";
    pub const ABOUT_2FA: &str = "2FA 验证码生成  -  生成 TOTP 密钥与二维码，兼容 Google Authenticator / Authy";
    pub const ABOUT_PWD: &str = "账号密码生成  -  支持 Argon2id / Bcrypt / PBKDF2 三种算法，附带 SQL 更新语句";
    pub const ABOUT_TURSO: &str = "Turso 数据库浏览  -  连接本地 Turso/libSQL 数据库，查看表结构与数据";
    pub const ABOUT_BUILT: &str = "基于 Rust + egui 构建";

    // 密码页
    pub const PWD_TITLE: &str = "账号密码生成";
    pub const PWD_PLATFORM: &str = "目标平台：";
    pub const PWD_ALGO: &str = "加密算法：";
    pub const PWD_GEN_8: &str = "\u{1F3B2} 8位";
    pub const PWD_GEN_12: &str = "\u{1F3B2} 12位";
    pub const PWD_GEN_16: &str = "\u{1F3B2} 16位";
    pub const PWD_INPUT_LABEL: &str = "待加密密码：";
    pub const PWD_INPUT_HINT: &str = "在此输入或点击上方按钮生成密码...";
    pub const PWD_ENCRYPT: &str = "\u{1F512} 加密";
    pub const PWD_OUTPUT_LABEL: &str = "加密的密码：";
    pub const PWD_SQL_LABEL: &str = "SQL更新语句：";

    // 算法描述
    pub const ALGO_ARGON2: &str = "OWASP 推荐，抗 GPU/ASIC";
    pub const ALGO_BCRYPT: &str = "经典算法，兼容性最广";
    pub const ALGO_PBKDF2: &str = "NIST 推荐，标准兼容";

    // 平台预设标签
    pub const PLAT_NONE: &str = "自定义";
    pub const PLAT_LARAVEL: &str = "Laravel (PHP)";
    pub const PLAT_DJANGO: &str = "Django (Python)";
    pub const PLAT_SPRING: &str = "Spring Boot (Java)";
    pub const PLAT_EXPRESS: &str = "Express (Node.js)";
    pub const PLAT_DOTNET: &str = "ASP.NET Core (.NET)";
    pub const PLAT_RAILS: &str = "Rails (Ruby)";
    pub const PLAT_WORDPRESS: &str = "WordPress (PHP)";
    pub const PLAT_GO: &str = "Go (Golang)";

    // 平台预设说明
    pub const NOTE_NONE: &str = "手动选择算法";
    pub const NOTE_LARAVEL: &str = "password_hash() 默认 Bcrypt";
    pub const NOTE_DJANGO: &str = "make_password() 默认 PBKDF2";
    pub const NOTE_SPRING: &str = "BCryptPasswordEncoder";
    pub const NOTE_EXPRESS: &str = "bcryptjs / argon2";
    pub const NOTE_DOTNET: &str = "PasswordHasher<T> 默认 PBKDF2";
    pub const NOTE_RAILS: &str = "has_secure_password 默认 Bcrypt";
    pub const NOTE_WORDPRESS: &str = "wp_hash_password() 支持 Argon2id";
    pub const NOTE_GO: &str = "golang.org/x/crypto/bcrypt";

    // TOTP 页
    pub const TOTP_TITLE: &str = "2FA TOTP验证码生成器";
    pub const TOTP_GEN_KEY: &str = "\u{1F511} 生成密钥";
    pub const TOTP_GEN_HINT: &str = "(生成兼容主流平台的安全密钥)";
    pub const TOTP_KEY_LABEL: &str = "Base32密钥，";
    pub const TOTP_KEY_WARN: &str = "已实际应用的密钥请妥善保存，泄漏将严重影响安全。";
    pub const TOTP_ACCOUNT: &str = "账号名：";
    pub const TOTP_ACCOUNT_HINT: &str = "用户账号名";
    pub const TOTP_ISSUER: &str = "发行方：";
    pub const TOTP_ISSUER_HINT: &str = "服务/应用名称";
    pub const TOTP_KEY_INPUT_HINT: &str = "在此输入或点击上方按钮生成Base32密钥...";
    pub const TOTP_GENERATE: &str = "\u{26A1} 生成";
    pub const TOTP_SAVE: &str = "\u{1F4BE} 保存";
    pub const TOTP_SAVED: &str = "密钥已保存\n当前密钥: {}";
    pub const TOTP_SAVE_FAIL: &str = "保存失败: {}";
    pub const TOTP_QR: &str = "\u{1F4F1} 二维码";
    pub const TOTP_CODE_LABEL: &str = "TOTP验证码：";
    pub const TOTP_SCAN_HINT: &str = "扫描此二维码配置Google Authenticator或Authy";
    pub const TOTP_CLICK_QR: &str = "点击上方按钮生成二维码";

    // Turso 页
    pub const TURSO_OPEN: &str = "\u{1F4C2} 打开";
    pub const TURSO_FILE_TITLE: &str = "选择Turso数据库文件";
    pub const TURSO_FILE_FILTER1: &str = "SQLite/Turso数据库";
    pub const TURSO_FILE_FILTER2: &str = "所有文件";
    pub const TURSO_CONNECT: &str = "\u{1F50C} 连接";
    pub const TURSO_CONNECTED: &str = "\u{2705} {} | {} 个表";
    pub const TURSO_REFRESHED: &str = "\u{2705} 已刷新";
    pub const TURSO_REFRESH: &str = "\u{1F504} 刷新";
    pub const TURSO_CLOSE_SQL: &str = "\u{2716} 关闭SQL";
    pub const TURSO_OPEN_SQL: &str = "\u{1F4DD} SQL查询";
    pub const TURSO_SQL_GROUP: &str = "SQL 查询";
    pub const TURSO_SQL_EXEC: &str = "\u{25B6} 执行";
    pub const TURSO_SQL_STATUS: &str = "\u{2705} {} 列, {} 行";
    pub const TURSO_SQL_CLEAR: &str = "\u{1F5D1} 清空";
    pub const TURSO_UNKNOWN_PANIC: &str = "未知panic";

    // DataTable
    pub const DT_TABLE_LIST: &str = "表列表 ({})";
    pub const DT_NO_TABLE: &str = "请先连接数据库并选择一个表";
    pub const DT_COL_ROW: &str = "| {} 列 | {} 行";
    pub const DT_COLUMNS: &str = "\u{2699} 列 ({}/{})";
    pub const DT_PREV: &str = "\u{25C0} 上一页";
    pub const DT_PAGE: &str = "第 {} / {} 页";
    pub const DT_NEXT: &str = "下一页 \u{25B6}";
    pub const DT_BACK: &str = "\u{2190} 返回列表";
    pub const DT_ROW_DETAIL: &str = "行 {} 详情";

    // 错误信息
    pub const ERR_DIR: &str = "获取当前目录失败: {}";
    pub const ERR_RUNTIME: &str = "创建运行时失败: {}";
    pub const ERR_CONNECT_DB: &str = "连接数据库失败: {}";
    pub const ERR_GET_CONN: &str = "获取连接失败: {}";
    pub const ERR_THREAD_PANIC: &str = "线程panic: {}";
    pub const ERR_THREAD_PANIC_UNKNOWN: &str = "线程panic(未知)";
    pub const ERR_QUERY_TABLES: &str = "查询表列表失败: {}";
    pub const ERR_TABLE_INFO: &str = "获取表结构失败: {}";
    pub const ERR_QUERY_DATA: &str = "查询表数据失败: {}";
    pub const ERR_EMPTY_SQL: &str = "SQL 语句不能为空";
    pub const ERR_QUERY_FAIL: &str = "查询失败: {}";
    pub const ERR_EXEC_FAIL: &str = "执行失败: {}";
    pub const ERR_SALT: &str = "生成随机盐失败: {}";
    pub const ERR_ARGON2_PARAM: &str = "Argon2 参数错误: {}";
    pub const ERR_ARGON2_HASH: &str = "Argon2 加密失败: {}";
    pub const ERR_BCRYPT_HASH: &str = "Bcrypt 加密失败: {}";
}

// ────────────────────── English ──────────────────────

#[cfg(feature = "english")]
pub mod lang {
    // Tabs
    pub const TAB_2FA: &str = "2FA Authenticator";
    pub const TAB_PASSWORD: &str = "Password Generator";
    pub const TAB_TURSO: &str = "Turso Database";

    // About
    pub const ABOUT_BTN: &str = "\u{203C} About";
    pub const ABOUT_TITLE: &str = "About";
    pub const ABOUT_DESC: &str = "Open-source toolbox with the following features:";
    pub const ABOUT_2FA: &str = "2FA Authenticator  -  Generate TOTP keys & QR codes, compatible with Google Authenticator / Authy";
    pub const ABOUT_PWD: &str = "Password Generator  -  Supports Argon2id / Bcrypt / PBKDF2, with SQL UPDATE output";
    pub const ABOUT_TURSO: &str = "Turso Database Viewer  -  Browse local Turso/libSQL databases, view tables and data";
    pub const ABOUT_BUILT: &str = "Built with Rust + egui";

    // Password tab
    pub const PWD_TITLE: &str = "Password Generator";
    pub const PWD_PLATFORM: &str = "Target Platform:";
    pub const PWD_ALGO: &str = "Algorithm:";
    pub const PWD_GEN_8: &str = "\u{1F3B2} 8 chars";
    pub const PWD_GEN_12: &str = "\u{1F3B2} 12 chars";
    pub const PWD_GEN_16: &str = "\u{1F3B2} 16 chars";
    pub const PWD_INPUT_LABEL: &str = "Password to encrypt:";
    pub const PWD_INPUT_HINT: &str = "Enter or click a button above to generate...";
    pub const PWD_ENCRYPT: &str = "\u{1F512} Encrypt";
    pub const PWD_OUTPUT_LABEL: &str = "Encrypted password:";
    pub const PWD_SQL_LABEL: &str = "SQL UPDATE statement:";

    // Algorithm descriptions
    pub const ALGO_ARGON2: &str = "OWASP recommended, anti GPU/ASIC";
    pub const ALGO_BCRYPT: &str = "Classic algorithm, widest compatibility";
    pub const ALGO_PBKDF2: &str = "NIST recommended, standard compliant";

    // Platform preset labels
    pub const PLAT_NONE: &str = "Custom";
    pub const PLAT_LARAVEL: &str = "Laravel (PHP)";
    pub const PLAT_DJANGO: &str = "Django (Python)";
    pub const PLAT_SPRING: &str = "Spring Boot (Java)";
    pub const PLAT_EXPRESS: &str = "Express (Node.js)";
    pub const PLAT_DOTNET: &str = "ASP.NET Core (.NET)";
    pub const PLAT_RAILS: &str = "Rails (Ruby)";
    pub const PLAT_WORDPRESS: &str = "WordPress (PHP)";
    pub const PLAT_GO: &str = "Go (Golang)";

    // Platform preset notes
    pub const NOTE_NONE: &str = "Select algorithm manually";
    pub const NOTE_LARAVEL: &str = "password_hash() defaults to Bcrypt";
    pub const NOTE_DJANGO: &str = "make_password() defaults to PBKDF2";
    pub const NOTE_SPRING: &str = "BCryptPasswordEncoder";
    pub const NOTE_EXPRESS: &str = "bcryptjs / argon2";
    pub const NOTE_DOTNET: &str = "PasswordHasher<T> defaults to PBKDF2";
    pub const NOTE_RAILS: &str = "has_secure_password defaults to Bcrypt";
    pub const NOTE_WORDPRESS: &str = "wp_hash_password() supports Argon2id";
    pub const NOTE_GO: &str = "golang.org/x/crypto/bcrypt";

    // TOTP tab
    pub const TOTP_TITLE: &str = "2FA TOTP Authenticator";
    pub const TOTP_GEN_KEY: &str = "\u{1F511} Generate Key";
    pub const TOTP_GEN_HINT: &str = "(Generate a secure key compatible with major platforms)";
    pub const TOTP_KEY_LABEL: &str = "Base32 Key,";
    pub const TOTP_KEY_WARN: &str = "Keep your active key safe. Leaking it will severely compromise security.";
    pub const TOTP_ACCOUNT: &str = "Account Name:";
    pub const TOTP_ACCOUNT_HINT: &str = "e.g. user@example.com";
    pub const TOTP_ISSUER: &str = "Issuer:";
    pub const TOTP_ISSUER_HINT: &str = "Service / App name";
    pub const TOTP_KEY_INPUT_HINT: &str = "Enter or click a button above to generate a Base32 key...";
    pub const TOTP_GENERATE: &str = "\u{26A1} Generate";
    pub const TOTP_SAVE: &str = "\u{1F4BE} Save";
    pub const TOTP_SAVED: &str = "Key saved\nCurrent key: {}";
    pub const TOTP_SAVE_FAIL: &str = "Save failed: {}";
    pub const TOTP_QR: &str = "\u{1F4F1} QR Code";
    pub const TOTP_CODE_LABEL: &str = "TOTP Code:";
    pub const TOTP_SCAN_HINT: &str = "Scan this QR code to set up Google Authenticator or Authy";
    pub const TOTP_CLICK_QR: &str = "Click the button above to generate QR code";

    // Turso tab
    pub const TURSO_OPEN: &str = "\u{1F4C2} Open";
    pub const TURSO_FILE_TITLE: &str = "Select Turso database file";
    pub const TURSO_FILE_FILTER1: &str = "SQLite/Turso databases";
    pub const TURSO_FILE_FILTER2: &str = "All files";
    pub const TURSO_CONNECT: &str = "\u{1F50C} Connect";
    pub const TURSO_CONNECTED: &str = "\u{2705} {} | {} tables";
    pub const TURSO_REFRESHED: &str = "\u{2705} Refreshed";
    pub const TURSO_REFRESH: &str = "\u{1F504} Refresh";
    pub const TURSO_CLOSE_SQL: &str = "\u{2716} Close SQL";
    pub const TURSO_OPEN_SQL: &str = "\u{1F4DD} SQL Query";
    pub const TURSO_SQL_GROUP: &str = "SQL Query";
    pub const TURSO_SQL_EXEC: &str = "\u{25B6} Execute";
    pub const TURSO_SQL_STATUS: &str = "\u{2705} {} cols, {} rows";
    pub const TURSO_SQL_CLEAR: &str = "\u{1F5D1} Clear";
    pub const TURSO_UNKNOWN_PANIC: &str = "Unknown panic";

    // DataTable
    pub const DT_TABLE_LIST: &str = "Tables ({})";
    pub const DT_NO_TABLE: &str = "Connect to a database and select a table first";
    pub const DT_COL_ROW: &str = "| {} cols | {} rows";
    pub const DT_COLUMNS: &str = "\u{2699} Columns ({}/{})";
    pub const DT_PREV: &str = "\u{25C0} Prev";
    pub const DT_PAGE: &str = "Page {} / {}";
    pub const DT_NEXT: &str = "Next \u{25B6}";
    pub const DT_BACK: &str = "\u{2190} Back";
    pub const DT_ROW_DETAIL: &str = "Row {} Detail";

    // Error messages
    pub const ERR_DIR: &str = "Failed to get current dir: {}";
    pub const ERR_RUNTIME: &str = "Failed to create runtime: {}";
    pub const ERR_CONNECT_DB: &str = "Failed to connect to database: {}";
    pub const ERR_GET_CONN: &str = "Failed to get connection: {}";
    pub const ERR_THREAD_PANIC: &str = "Thread panic: {}";
    pub const ERR_THREAD_PANIC_UNKNOWN: &str = "Thread panic (unknown)";
    pub const ERR_QUERY_TABLES: &str = "Failed to query tables: {}";
    pub const ERR_TABLE_INFO: &str = "Failed to get table info: {}";
    pub const ERR_QUERY_DATA: &str = "Failed to query data: {}";
    pub const ERR_EMPTY_SQL: &str = "SQL statement cannot be empty";
    pub const ERR_QUERY_FAIL: &str = "Query failed: {}";
    pub const ERR_EXEC_FAIL: &str = "Execution failed: {}";
    pub const ERR_SALT: &str = "Failed to generate salt: {}";
    pub const ERR_ARGON2_PARAM: &str = "Argon2 parameter error: {}";
    pub const ERR_ARGON2_HASH: &str = "Argon2 hashing failed: {}";
    pub const ERR_BCRYPT_HASH: &str = "Bcrypt hashing failed: {}";
}
