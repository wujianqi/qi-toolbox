//! SVG 图标字节常量 — Lucide 风格 24×24 线性图标（MIT 许可风格，手写精简路径）
//!
//! 单色黑描边，可用 `Image::tinted` 按主题色染色；配合 `on_state` 注册各状态
//! 染色图，实现图标随按钮 平常/悬停/按下/禁用 状态变色。
//!
//! 注意：`ImageContent::tint` 会把颜色应用到**所有**状态层，无法区分状态，
//! 因此状态着色一律用 `on_state(state, 染色图)`（见 [`stateful_icon`]）。

use windui::render::image::{Image, VisualState};
use windui::ui::ImageContent;

/// 软件 Logo（`src/logo.svg` 编译期内嵌，作应用窗口图标 / 关于页品牌图）。
/// Logo 自带品牌色（非单色线性图），不参与主题染色。
pub const LOGO: &[u8] = include_bytes!("../logo.svg");

/// 一次性解析 + 按主题色染好的图标四态（普通/悬停/按下/禁用）。
/// `Image` 为 Rc 共享，克隆廉价 —— 供高频行内图标（如表格行按钮）在闭包外
/// 构造一次、每次 `content()` 复用，避免滚动重建时反复解析 SVG 与染色。
#[derive(Clone)]
pub struct StatefulIcon {
    normal: Image,
    hover: Image,
    pressed: Image,
    disabled: Image,
}

impl StatefulIcon {
    /// 解析 SVG 并按当前主题色染色四态；解析失败返回 None（绘制时画占位框）。
    ///
    /// 输入法工具栏风格：平常=灰（`text_muted`），悬停/按下=主题蓝（`accent`），
    /// 禁用=更浅的灰（`text_disabled`）。
    pub fn from_svg(bytes: &[u8], target_width: Option<u32>) -> Option<Self> {
        let raw = Image::from_svg_bytes(bytes, target_width).ok()?;
        let th = windui::theme::current();
        let p = &th.palette;
        Some(Self {
            normal: raw.tinted(p.text_muted),
            hover: raw.tinted(p.accent_hover),
            pressed: raw.tinted(p.accent_active),
            disabled: raw.tinted(p.text_disabled),
        })
    }

    /// 组装为带状态层的 `ImageContent`（各状态层已染色，不设 tint）。
    pub fn content(&self) -> ImageContent {
        ImageContent::new(Some(self.normal.clone()))
            .on_state(VisualState::Hover, self.hover.clone())
            .on_state(VisualState::Pressed, self.pressed.clone())
            .on_state(VisualState::Disabled, self.disabled.clone())
    }
}

/// 带状态着色的按钮图标：平常=主题文字色，悬停=主题强调色，按下=强调激活色，
/// 禁用=禁用文字色。用 `Element::icon_content(...)` 挂到按钮上替换静态 `icon_svg`。
pub fn stateful_icon(bytes: &[u8], target_width: Option<u32>) -> ImageContent {
    StatefulIcon::from_svg(bytes, target_width)
        .map(|s| s.content())
        .unwrap_or_else(|| ImageContent::new(None))
}


/// 关于（圆圈 + i）
pub const INFO: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>"##;

/// 骰子（随机生成密码）
pub const DICE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1"/><circle cx="15.5" cy="8.5" r="1"/><circle cx="8.5" cy="15.5" r="1"/><circle cx="15.5" cy="15.5" r="1"/></svg>"##;

/// 锁（加密）
pub const LOCK: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>"##;

/// 钥匙（生成密钥）
pub const KEY: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/></svg>"##;

/// 闪电（生成验证码）
pub const ZAP: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10"/></svg>"##;

/// 软盘（保存）
pub const SAVE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><polyline points="17 21 17 13 7 13 7 21"/><polyline points="7 3 7 8 15 8"/></svg>"##;

/// 二维码
pub const QR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><path d="M14 14h3v3h-3z"/><path d="M21 14v3h-3"/></svg>"##;

/// 文件夹（打开数据库文件）
pub const FOLDER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>"##;

/// 插头（连接数据库）
pub const PLUG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8z"/></svg>"##;

/// 刷新（循环箭头）
pub const REFRESH: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><polyline points="21 3 21 8 16 8"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/><polyline points="3 21 3 16 8 16"/></svg>"##;

/// SQL 查询（终端/代码）
pub const TERMINAL: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>"##;

/// 执行（播放三角）
pub const PLAY: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="6 3 20 12 6 21 6 3"/></svg>"##;

/// 清空（垃圾桶）
pub const TRASH: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>"##;

/// 表格/列设置（网格）
pub const TABLE_ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="3" y1="15" x2="21" y2="15"/><line x1="9" y1="9" x2="9" y2="21"/><line x1="15" y1="9" x2="15" y2="21"/></svg>"##;

/// 返回（左向箭头）
pub const ARROW_LEFT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 19 5 12 12 5"/></svg>"##;

/// 放大镜（查看行详情）
pub const SEARCH: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>"##;

/// 月亮（深色主题）
pub const MOON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z"/></svg>"##;

/// 太阳（浅色主题）
pub const SUN: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/></svg>"##;

/// 服务器（SFTP 菜单）
pub const SERVER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></svg>"##;

/// 文件（SFTP 文件列表行）
pub const FILE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z"/><polyline points="14 2 14 8 20 8"/></svg>"##;

/// 上传（向上箭头出托盘）
pub const UPLOAD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>"##;

/// 下载（向下箭头入托盘）
pub const DOWNLOAD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>"##;

/// 地球（远程检测菜单）
pub const GLOBE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/></svg>"##;

/// 盾牌（SSL 证书状态）
pub const SHIELD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/></svg>"##;
