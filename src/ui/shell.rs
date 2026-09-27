//! 平台与窗口壳层助手（从 `mod.rs` 拆出）：
//! 应用图标光栅化、窗体标题、系统深浅色偏好读取。

use windui::signal::Signal;

use super::icons;
use crate::lang;

/// 应用图标：`logo.svg` 矢量源按需光栅化。平台在**不同场合要不同尺寸**
/// 的图标（任务栏取大档，标题栏取小档），`IconSource::sized` 让每档都按
/// 实际物理像素 1:1 现画，避免固定位图交给系统缩放导致高 DPI 发糊。
pub(crate) fn app_icon() -> windui::icon::IconSource {
    windui::icon::IconSource::sized(|size| {
        // Image::from_svg_bytes 返回 Result；失败走 1×1 透明像素兜底
        windui::render::image::Image::from_svg_bytes(icons::LOGO, Some(size))
            .ok()
            .and_then(|img| windui::icon::WindowIcon::from_image(&img))
            .unwrap_or_else(|| {
                // 解析失败兜底：1×1 透明像素（正常不会走到）
                windui::icon::WindowIcon::from_rgba(1, 1, vec![0, 0, 0, 0])
                    .expect("1×1 RGBA 必然合法")
            })
    })
}

/// 模块 id → 模块名（窗体标题后缀，与侧栏名称一致：如「启途 - 关于」）
fn module_name(id: usize) -> String {
    match id {
        0 => lang::TAB_2FA(),
        1 => lang::TAB_PASSWORD(),
        2 => lang::TAB_TURSO(),
        3 => lang::TAB_SFTP(),
        4 => lang::TAB_REMOTE(),
        6 => lang::S3_TAB(),
        7 => lang::MYSQL_TAB(),
        8 => lang::PG_TAB(),
        _ => lang::TAB_ABOUT(),
    }
}

/// 把「APP_NAME - 模块名」写入标题信号（windui 0.19 起标题绑 `Signal<String>`
/// 跟随，不再走 SetWindowTextW 平台 hack）
pub(crate) fn set_window_title(title: Signal<String>, tab: usize) {
    title.set(format!("{} - {}", lang::APP_NAME(), module_name(tab)));
}

/// 读取 Windows「应用」深浅色偏好（注册表 AppsUseLightTheme：1=浅色 0=深色）。
/// 读取失败或非 Windows 平台一律按浅色处理。
#[cfg(windows)]
pub(crate) fn system_light_theme() -> bool {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ,
    };
    let path: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let name: Vec<u16> = "AppsUseLightTheme"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut key = 0isize;
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut key) == 0
            && RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut value as *mut u32 as *mut u8,
                &mut size,
            ) == 0
            && size == 4
    };
    if key != 0 {
        unsafe {
            RegCloseKey(key);
        }
    }
    ok && value != 0
}

#[cfg(not(windows))]
pub(crate) fn system_light_theme() -> bool {
    true
}
