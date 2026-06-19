//! Qi Toolbox

#![windows_subsystem = "windows"]

mod password;
mod totp;
mod turso_viewer;
mod datatable;
mod strings;
mod sql_editor;
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

fn main() -> eframe::Result<()> {
    hide_console_window();

    // 日志已禁用
    // let _ = std::fs::write("qi-toolbox.log", "=== qi-toolbox started ===\n");

    let options = eframe::NativeOptions::default();

    eframe::run_native(
        "Qi Toolbox",
        options,
        Box::new(|cc| {
            egui_cjk_font::load_cjk_font(&cc.egui_ctx);
            Ok(Box::new(ui::QiToolboxApp::new()))
        }),
    )
}
