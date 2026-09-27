//! 应用主题：借鉴 WindInput「清风」设计体系的调色板。
//! 现代蓝主色 + 亮色纯白 / 暗色深蓝灰面板 + 较大圆角 + 柔和边界。

use windui::geometry::Color;
use windui::theme::{ButtonTheme, Metrics, Palette, Theme};

/// 清风主色（现代蓝）。
const PRIMARY: u32 = 0x3B82F6;
const PRIMARY_HOVER: u32 = 0x60A5FA;
const PRIMARY_ACTIVE: u32 = 0x2563EB;

/// 主色 10% 透明度叠在浅色底上的等效色（清风 accent_soft 亮色）。
const ACCENT_SOFT_LIGHT: u32 = 0xE7EFFE;
/// 主色 16% 透明度叠在暗色面板上的等效色（清风 accent_soft 暗色）。
const ACCENT_SOFT_DARK: u32 = 0x2C4370;

/// 浅色主题（清风·蓝）：纯白底、浅灰表面、深蓝灰正文。
pub fn light() -> Theme {
    Theme {
        palette: Palette {
            accent: Color::hex(PRIMARY),
            accent_hover: Color::hex(PRIMARY_HOVER),
            accent_active: Color::hex(PRIMARY_ACTIVE),
            on_accent: windui::geometry::Color::WHITE,
            bg: Color::hex(0xFFFFFF),
            surface: Color::hex(0xF2F4F8),
            surface_alt: Color::hex(0xF7F8FA),
            surface_inverse: Color::hex(0x1A1D24),
            on_surface_inverse: Color::hex(0xF2F3F7),
            text: Color::hex(0x1A1D24),
            text_muted: Color::hex(0x5A6072),
            text_subtle: Color::hex(0x8A90A0),
            text_disabled: Color::hex(0xB4B9C4),
            border: Color::hex(0xE2E5EB),
            track: Color::hex(0xD9DDE4),
            placeholder: Color::hex(0x9AA0AE),
            divider: Color::hex(0xEBEDF2),
            danger: Color::hex(0xE5484D),
            success: Color::hex(0x2EA043),
            warning: Color::hex(0xD97706),
        },
        metrics: metrics(),
        button: button(
            ACCENT_SOFT_LIGHT,
            0xD9E6FD, // hover：主色 ~18%
            0xCCE0FC, // press：主色 ~25%
            0x1D4ED8, // 亮色取蓝 700，淡底上对比充足
            0xE8EBF1,
        ),
        ..Theme::default()
    }
}

/// 暗色主题（清风·暗）：深蓝灰底、亮蓝主色、浅色正文。
pub fn dark() -> Theme {
    Theme {
        palette: Palette {
            accent: Color::hex(PRIMARY),
            accent_hover: Color::hex(PRIMARY_HOVER),
            accent_active: Color::hex(PRIMARY_ACTIVE),
            on_accent: windui::geometry::Color::WHITE,
            bg: Color::hex(0x121826),
            surface: Color::hex(0x1E2A3E),
            surface_alt: Color::hex(0x242F45),
            surface_inverse: Color::hex(0xF2F3F7),
            on_surface_inverse: Color::hex(0x121826),
            text: Color::hex(0xF2F3F7),
            text_muted: Color::hex(0x9AA3B4),
            text_subtle: Color::hex(0x6E7788),
            text_disabled: Color::hex(0x525C6E),
            border: Color::hex(0x2E3650),
            track: Color::hex(0x2B3550),
            placeholder: Color::hex(0x5C6376),
            divider: Color::hex(0x232C40),
            danger: Color::hex(0xE5484D),
            success: Color::hex(0x3FB950),
            warning: Color::hex(0xD29922),
        },
        metrics: metrics(),
        button: button(
            ACCENT_SOFT_DARK,
            0x34507E, // hover：淡蓝底加亮一档
            0x3D5D93, // press：再加亮一档
            0x93C5FD, // 暗色用亮蓝字（blue-300）保证深底可读
            0x27314A,
        ),
        ..Theme::default()
    }
}

/// 按钮覆盖层：清风式柔和主按钮——半透明 accent 淡底 + 深蓝文字（替代默认的
/// 实心蓝底白字，与轻量的图标/描边次级按钮更协调）。圆角统一 8px。
/// `light`/`dark` 分别传入各自的淡底/hover/active/文字色。
fn button(soft_bg: u32, soft_hover: u32, soft_active: u32, fg: u32, disabled: u32) -> ButtonTheme {
    ButtonTheme {
        bg: Some(Color::hex(soft_bg)),
        hover: Some(Color::hex(soft_hover)),
        active: Some(Color::hex(soft_active)),
        fg: Some(Color::hex(fg)),
        disabled: Some(Color::hex(disabled)),
        corner: Some(8.0),
    }
}

/// 圆角体系对齐清风：小 6 / 中 8 / 大 12。
fn metrics() -> Metrics {
    Metrics {
        corner_sm: 6.0,
        corner_md: 8.0,
        corner_lg: 12.0,
        ..Metrics::default()
    }
}
