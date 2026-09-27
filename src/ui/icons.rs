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

/// 云（S3 浏览）
pub const CLOUD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17.5 19a4.5 4.5 0 0 0 .42-8.98 6.5 6.5 0 0 0-12.7 1.61A4 4 0 0 0 6 19h11.5z"/></svg>"##;

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

/// 数据库（圆柱体）：分级列表的库/schema 组头
pub const DATABASE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/></svg>"##;

/// MySQL（海豚简笔）：侧栏导航
pub const MYSQL: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000000"><path d="M23.556 18.153c-.189-.333-.626-.903-2.239-2.001a26.402 26.402 0 00-3.01-1.783c-.736-1.991-2.16-4.572-4.239-7.68-2.238-3.347-4.837-4.757-7.742-4.198a17.968 17.968 0 00-.984-.907C4.61.955 2.815.421 1.784.524 1.317.571.966.742.741 1.031.22 1.702.394 2.668 2.797 5.815c.457 1.338.555 2.036.555 2.388 0 .743.384 1.596 1.202 2.663L3.785 13.5c-.686 2.352.561 5.394 1.722 6.454.62.566 1.141.482 1.399.381.517-.203.934-.709 1.204-2.152.138.339.284.705.438 1.097.831 2.11 1.644 3.278 2.557 3.673a.77.77 0 00.612-1.411c-.226-.099-.85-.57-1.738-2.826-.75-1.903-1.295-3.176-1.668-3.893a.771.771 0 00-1.452.324c-.093 2.189-.314 3.136-.462 3.535-.637-.733-1.638-3.03-1.136-4.751.499-1.713.793-2.716.881-3.012A.77.77 0 006 10.214c-1.002-1.23-1.11-1.818-1.11-2.013 0-.709-.222-1.693-.68-3.01a.79.79 0 00-.115-.214C2.8 3.291 2.325 2.447 2.151 2.046c.616.029 1.788.362 2.189.706.412.351.805.723 1.179 1.114.192.203.48.286.751.215 2.41-.623 4.544.509 6.519 3.463 2.112 3.158 3.515 5.725 4.17 7.629a.771.771 0 00.394.443 24.277 24.277 0 013.098 1.81c.178.121.338.233.483.337h-2.377a.77.77 0 00-.585 1.268c.763.904 1.548 1.79 2.353 2.657a.77.77 0 101.15-1.022l-.03-.032a52.34 52.34 0 01-1.204-1.331h2.647a.77.77 0 00.668-1.15z"/><path d="M8.163 6.025c.305.523.54.898.706 1.125.199.391.509.703.708 1.095l.048-.048c.349-.239.623-1.191.066-1.815-.289-.437-1.227-.425-1.528-.357z"/></svg>"##;

/// PostgreSQL（大象头简笔）：侧栏导航
pub const POSTGRESQL: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000000"><path d="M22.998 4.034a9.2 9.2 0 00-1.69-2.404 3.089 3.089 0 00-2.206-.947h-3.11a6.76 6.76 0 00-3.164-.924 6.687 6.687 0 00-3.689.924H6.067c-1.397 0-2.562.205-3.466.6-.825.357-1.448.884-1.855 1.557C.481 3.278.33 3.873.284 4.659c-.036.6-.008 1.314.08 2.118.156 1.405.47 2.823.691 3.609l.047.164c.195.698.491 1.757.914 2.726.553 1.269 1.179 2.066 1.911 2.44.27.138.566.218.867.234.031 0 .062.003.091.003.293-.003.581-.06.851-.171a2.772 2.772 0 001.423-1.295l.187-.356c.07.065.145.13.221.19.324.26.678.483 1.051.665a6.622 6.622 0 01-.839.584c-.029.016-.057.034-.086.049a1.33 1.33 0 00-.652 1.337c.067.537.459.976.984 1.101a5.34 5.34 0 002.99-.158v1.848c0 .436.086.867.252 1.269.158.387.389.737.683 1.036.288.296.633.532 1.012.696.392.169.813.257 1.238.254h.003c.426.003.846-.083 1.238-.252a3.14 3.14 0 001.012-.698c.293-.299.524-.652.683-1.036a3.34 3.34 0 00.252-1.272v-2.347c.112.008.231.013.345.013a5.787 5.787 0 001.669-.254 7.918 7.918 0 002.022-.955c.361-.236.543-.67.462-1.093a1.05 1.05 0 00-.823-.844 8 8 0 01-1.957-.646l3.46-4.727a4.723 4.723 0 00.432-4.857zM5.244 14.488c-.267.106-.488.101-.716-.013-.299-.151-.761-.581-1.277-1.763-.382-.88-.662-1.879-.849-2.541l-.047-.166a23.246 23.246 0 01-.646-3.38c-.231-2.088.054-2.827.187-3.048.257-.423.672-.766 1.233-1.012.737-.319 1.724-.485 2.936-.485h1.469c-.771.944-1.264 2.141-1.438 3.501-.036.293-.065.6-.083.909l-.003.018v5.174l.005.026c.005.062.055.592.405 1.251l-.459.872c-.153.296-.408.53-.717.657zm2.482-1.999a2.485 2.485 0 01-.358-.877V7.206h1.075a1.133 1.133 0 00-.013.164.85.85 0 00.836.859.845.845 0 00.836-.854c0-.029 0-.055-.005-.083.597.208.997.774.994 1.407v.994a6.913 6.913 0 01-1.438 4.231 4.567 4.567 0 01-1.202-.662 3.121 3.121 0 01-.725-.773zm1.991 4.294c-.4.047-.805.031-1.199-.042a7.82 7.82 0 001.464-1.129c.33.234.594.545.774.906a4.018 4.018 0 01-1.039.265zm6.319-.185v3.152a1.87 1.87 0 01-.537 1.319c-.34.35-.807.545-1.293.543h-.003a1.791 1.791 0 01-1.293-.545 1.868 1.868 0 01-.535-1.316v-2.105a3.883 3.883 0 00-1.493-3.079 8.374 8.374 0 001.56-4.87v-.996c0-.384-.075-.763-.221-1.119a2.847 2.847 0 00-.6-.911 2.842 2.842 0 00-.89-.62 2.747 2.747 0 00-1.089-.231H7.435l.008-.062A5.792 5.792 0 018.11 3.66a4.723 4.723 0 011.23-1.438c.945-.737 2.165-1.116 3.434-1.067 1.269.049 2.453.522 3.338 1.332.47.428.849.948 1.114 1.524.26.571.423 1.184.48 1.809h-.626a2.619 2.619 0 00-1.874.8 2.739 2.739 0 00-.776 1.924 6.837 6.837 0 00.558 2.726 6.686 6.686 0 001.584 2.264c.156.145.317.286.485.418l-.013.016a3.911 3.911 0 00-1.008 2.63zm.236-9.086c.06.41.408.719.823.722a.82.82 0 00.636-.304l-.003 3.725v.042c.021.327 0 .654-.06.976a4.38 4.38 0 01-.187-.169c-1.08-1.007-1.698-2.453-1.698-3.961.004-.397.183-.776.489-1.031zm2.755 8.313a4.396 4.396 0 01-1.568.19c.099-.41.296-.787.579-1.098a4.73 4.73 0 00.171-.202 8.892 8.892 0 001.786.724 6.2 6.2 0 01-.968.386zm3.078-9.426a3.309 3.309 0 01-.62 1.659l-2.393 3.271.003-4.79a3.32 3.32 0 00-.008-.457v-.003c-.075-1.555-.587-2.926-1.472-4h1.49c.231 0 .459.049.672.14.215.091.408.226.571.392.584.6 1.07 1.29 1.436 2.043.264.54.376 1.145.321 1.745z"/></svg>"##;

/// SQLite（羽毛笔简笔）：侧栏导航
pub const SQLITE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000000"><path d="M20.698.313c-.494-.451-1.062-.611-1.706-.483-.579.107-1.191.451-1.834 1.03l-.547.547c-.3.322-.59.665-.869 1.03a2.342 2.342 0 00-.933-.193H4.478c-.687 0-1.271.241-1.754.724A2.39 2.39 0 002 4.722v10.331c0 .665.241 1.239.724 1.722a2.388 2.388 0 001.754.724h7.273c.064.322.118.601.161.837v.225c-.043.923-.021 1.845.064 2.768.107 1.266.279 2.156.515 2.671l.129-.064c-.343-1.073-.483-2.403-.418-3.991.107-2.553.665-5.353 1.674-8.4.88-2.274 1.883-4.296 3.009-6.066s2.258-3.095 3.395-3.975c-.644.558-1.373 1.523-2.188 2.896a51.58 51.58 0 00-2.06 3.862 63.296 63.296 0 00-1.352 3.154 41.496 41.496 0 00-1.963 6.919c.279-.858.847-1.609 1.705-2.253a5.56 5.56 0 011.159-.676c.708-.88 1.48-1.953 2.317-3.218-1.244.3-2.027.515-2.349.644l-.804.322 1.223-.644c.922-.494 1.738-.858 2.446-1.094 1.459-2.339 2.446-4.366 2.961-6.083.686-2.338.46-4.011-.677-5.02zM11.88 11.03c.193.408.375.933.547 1.577l.161.74-.193-.515c-.064-.15-.236-.472-.515-.965l-.161-.354-.418 1.191c.15.279.29.622.418 1.03.107.3.204.622.29.965l.097.451-.161-.451c-.043-.172-.268-.611-.676-1.319l-.097-.161c-.236.858-.316 1.352-.241 1.481.075.129.177.354.306.676l.064.193H4.478a.499.499 0 01-.515-.515V4.723a.499.499 0 01.515-.515h10.073a22.592 22.592 0 00-1.674 3.428c-.471 1.211-.803 2.343-.997 3.394z"/></svg>"##;

// ────────────────────── 文件浏览器彩色图标（自带配色，不参与主题染色） ──────────────────────
/// 文件夹
pub const FOLDER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#FFB300" d="M2 7a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7z"/><path fill="#FFCA28" d="M2 9h20v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9z"/></svg>"##;

/// 文件夹选中
pub const FOLDER_SELECTED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#FFB300" d="M2 7a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7z"/><path fill="#FFCA28" d="M2 9h20v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9z"/><circle cx="18.5" cy="18.5" r="4.8" fill="#66BB6A"/><path d="M16 18.6l1.9 1.9 3.5-3.7" fill="none" stroke="#fff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// 图像文件
pub const IMAGE_FILE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#8E7BA8" d="M14 3v6h6L14 3z"/><circle cx="9" cy="14" r="1.2" fill="#8E7BA8"/><path d="M5.5 19.5l3.5-4 2.5 2.5 3.5-4.5 3.5 6z" fill="#8E7BA8"/></svg>"##;

/// 图像文件选中
pub const IMAGE_FILE_SELECTED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#8E7BA8" d="M14 3v6h6L14 3z"/><circle cx="9" cy="14" r="1.2" fill="#8E7BA8"/><path d="M5.5 19.5l3.5-4 2.5 2.5 3.5-4.5 3.5 6z" fill="#8E7BA8"/><circle cx="18.5" cy="18.5" r="4.8" fill="#66BB6A"/><path d="M16 18.6l1.9 1.9 3.5-3.7" fill="none" stroke="#fff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// 配置文件
pub const CONFIG_FILE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#6B9BA0" d="M14 3v6h6L14 3z"/><rect x="6.5" y="12.5" width="11" height="1.2" rx="0.6" fill="#6B9BA0"/><rect x="6.5" y="15.5" width="11" height="1.2" rx="0.6" fill="#6B9BA0"/><rect x="6.5" y="18.5" width="11" height="1.2" rx="0.6" fill="#6B9BA0"/><circle cx="10" cy="13.1" r="1.6" fill="#E4E8EC"/><circle cx="14" cy="16.1" r="1.6" fill="#E4E8EC"/><circle cx="9" cy="19.1" r="1.6" fill="#E4E8EC"/></svg>"##;

/// 配置文件选中
pub const CONFIG_FILE_SELECTED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#6B9BA0" d="M14 3v6h6L14 3z"/><rect x="6.5" y="12.5" width="11" height="1.2" rx="0.6" fill="#6B9BA0"/><rect x="6.5" y="15.5" width="11" height="1.2" rx="0.6" fill="#6B9BA0"/><rect x="6.5" y="18.5" width="11" height="1.2" rx="0.6" fill="#6B9BA0"/><circle cx="10" cy="13.1" r="1.6" fill="#E4E8EC"/><circle cx="14" cy="16.1" r="1.6" fill="#E4E8EC"/><circle cx="9" cy="19.1" r="1.6" fill="#E4E8EC"/><circle cx="18.5" cy="18.5" r="4.8" fill="#66BB6A"/><path d="M16 18.6l1.9 1.9 3.5-3.7" fill="none" stroke="#fff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// 日志文件
pub const LOG_FILE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#7A9B72" d="M14 3v6h6L14 3z"/><rect x="6.5" y="13" width="11" height="1.2" rx="0.6" fill="#7A9B72"/><rect x="6.5" y="16" width="11" height="1.2" rx="0.6" fill="#7A9B72"/><rect x="6.5" y="19" width="8" height="1.2" rx="0.6" fill="#7A9B72"/></svg>"##;

/// 日志文件选中
pub const LOG_FILE_SELECTED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#7A9B72" d="M14 3v6h6L14 3z"/><rect x="6.5" y="13" width="11" height="1.2" rx="0.6" fill="#7A9B72"/><rect x="6.5" y="16" width="11" height="1.2" rx="0.6" fill="#7A9B72"/><rect x="6.5" y="19" width="8" height="1.2" rx="0.6" fill="#7A9B72"/><circle cx="18.5" cy="18.5" r="4.8" fill="#66BB6A"/><path d="M16 18.6l1.9 1.9 3.5-3.7" fill="none" stroke="#fff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// 通用文件
pub const GENERIC_FILE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#7C93A8" d="M14 3v6h6L14 3z"/></svg>"##;

/// 通用文件选中
pub const GENERIC_FILE_SELECTED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#7C93A8" d="M14 3v6h6L14 3z"/><circle cx="18.5" cy="18.5" r="4.8" fill="#66BB6A"/><path d="M16 18.6l1.9 1.9 3.5-3.7" fill="none" stroke="#fff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// 压缩包文件
pub const ARCHIVE_FILE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#AD8B6B" d="M14 3v6h6L14 3z"/><path d="M12 10.5v9" fill="none" stroke="#AD8B6B" stroke-width="1.8" stroke-linecap="round" stroke-dasharray="0.1 2"/></svg>"##;

/// 压缩包文件选中
pub const ARCHIVE_FILE_SELECTED: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E4E8EC" d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9L14 3z"/><path fill="#AD8B6B" d="M14 3v6h6L14 3z"/><path d="M12 10.5v9" fill="none" stroke="#AD8B6B" stroke-width="1.8" stroke-linecap="round" stroke-dasharray="0.1 2"/><circle cx="18.5" cy="18.5" r="4.8" fill="#66BB6A"/><path d="M16 18.6l1.9 1.9 3.5-3.7" fill="none" stroke="#fff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// 上传（向上箭头出托盘）
pub const UPLOAD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>"##;

/// 下载（向下箭头入托盘）
pub const DOWNLOAD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>"##;

/// 地球（远程检测菜单）
pub const GLOBE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/></svg>"##;

/// 盾牌（SSL 证书状态）
pub const SHIELD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/></svg>"##;

/// 左尖括号（收起菜单手柄）
pub const CHEVRON_LEFT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>"##;

/// 右尖括号（展开菜单手柄）
pub const CHEVRON_RIGHT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>"##;

/// 菜单折叠手柄形状：**右半胶囊**（左边缘平直贴分隔线，右侧半圆），实心。
/// 与 18 宽胶囊的右半完全一致（圆角半径 9），随主题染成 Divider 色后视觉上
/// 就是"分隔线鼓出的一块"，整体感强。
pub const HANDLE_TAB: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 9 46" fill="none"><path d="M0 0 A9 9 0 0 1 9 9 L9 37 A9 9 0 0 1 0 46 Z" fill="#000000"/></svg>"##;
