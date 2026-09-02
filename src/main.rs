//! Qi Toolbox — windui 桌面版
//!
//! 模块布局（分层）：
//! - [`core`]：业务层，纯逻辑、不依赖 UI 框架（TOTP/密码/SFTP/Turso/后台任务）
//! - [`lang`]：国际化文案（core 与 ui 共用）
//! - [`ui`]：界面层，仅做渲染与交互（windui），页面状态/消息处理按页封装

#![windows_subsystem = "windows"]

mod core;
mod lang;
mod ui;

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

/// 启动时检测系统语言：主语言为中文（LANGID 主语言 0x04，含简繁）则界面默认中文，否则英文。
/// 用户可在主面板「中 / EN」手动覆盖。
#[cfg(windows)]
fn detect_system_language() {
    use crate::lang::{self, LANG_EN, LANG_ZH};
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    let lang_id = unsafe { GetUserDefaultUILanguage() };
    // LANGID：低 10 位为主语言；LANG_CHINESE = 0x04
    lang::set_current(if (lang_id & 0x3FF) == 0x04 { LANG_ZH } else { LANG_EN });
}

#[cfg(not(windows))]
fn detect_system_language() {}

fn main() {
    hide_console_window();
    detect_system_language(); // 跟随系统语言自动选择（中文系统→中文，非中文→英文）
    ui::run();
}
