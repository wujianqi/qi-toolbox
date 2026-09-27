//! Qi Toolbox — windui 桌面版
//!
//! 模块布局（分层）：
//! - [`core`]：业务层，纯逻辑、不依赖 UI 框架（TOTP/密码/SFTP/Turso/后台任务）
//! - [`lang`]：国际化文案（core 与 ui 共用）
//! - [`ui`]：界面层，仅做渲染与交互（windui），页面状态/消息处理按页封装

#![windows_subsystem = "windows"]

mod core;
#[path = "i18n/lang.rs"]
mod lang;
mod ui;
mod widgets;

#[cfg(windows)]
fn hide_console_window() {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

    unsafe {
        let hwnd = GetConsoleWindow();
        if hwnd != 0 {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn hide_console_window() {}

fn main() {
    hide_console_window();
    // 装载 i18n 译文（Initial::System 跟随系统语言：中文系统→zh-CN，非中文→en）
    lang::install();
    // 文件日志：先初始化（含全局 panic hook），再记录启动信息到缓存目录 logs/
    crate::core::log::init();
    // 本地存储库（store.db）：建表 + 旧文件数据一次性导入（失败仅记日志，不阻断启动）
    if let Err(e) = crate::core::store::init() {
        crate::core::log::warn("store", &e);
    }
    migrate_legacy_key_file();
    let lang_label = if lang::is_zh() { "zh" } else { "en" };
    crate::core::log::info(
        "app",
        &format!(
            "Qi Toolbox v{} starting on {}/{} (lang {})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            lang_label
        ),
    );
    ui::run();
}

/// 遗留 `qi_key.txt`（工作目录下的 2FA 密钥）一次性迁入 store kv 表后删除。
/// 敏感键按 settings 的加密规则写库；无加密后端/失败时保留旧文件不动。
fn migrate_legacy_key_file() {
    const KEY: &str = "totp.key";
    let legacy = std::path::Path::new("qi_key.txt");
    let Ok(text) = std::fs::read_to_string(legacy) else {
        return;
    };
    let key = text.trim();
    if key.is_empty() {
        let _ = std::fs::remove_file(legacy);
        return;
    }
    // 已有记忆的密钥则不覆盖（以库中为准）
    let exists = crate::core::store::kv_all()
        .ok()
        .and_then(|kvs| kvs.iter().find(|(k, _)| k == KEY).map(|(_, _)| ()))
        .is_some();
    if exists {
        let _ = std::fs::remove_file(legacy);
        return;
    }
    // 复用 settings 的加密路径：借 commit 的映射语义（拼装一次性 map 不方便，直接走 protect）
    if crate::core::settings::is_secret_key(KEY) {
        let enc = (|| -> Option<String> {
            let cipher = crate::core::settings::protect(key.as_bytes()).ok()?;
            Some(String::from_utf8_lossy(&cipher).into_owned())
        })();
        if let Some(enc) = enc {
            if crate::core::store::kv_set(&[(KEY, enc.as_str())]).is_ok() {
                let _ = std::fs::remove_file(legacy);
            }
        }
    }
}
