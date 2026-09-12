//! 共享 UI 组件：卡片 / 侧栏导航项 / 主题切换 / 关于页
//! （各功能页面文件复用；与业务无关的纯展示组件）

use windui::prelude::*;

use super::icons;
use super::select_text::SelectText;
use crate::lang;

/// 侧栏导航项：图标块 + 名称，选中/未选中两棵子树叠放互斥显示，
/// 左缘指示条作为**覆盖层**贴在行左沿。
///
/// `on_switch`：点击切换后回调（模块 id），供 UI 层同步窗体标题等。
/// 不用改 padding / border 来表达选中：那样切换时图标与文字会横向跳一下。
/// 两态的内边距完全一致，动的只有底色、图标底色与字重/字色。
/// （仿 windui examples/settings.rs 的 `nav_item`，字形换成 SVG 图标）
pub fn nav_item(
    name: &'static str,
    icon: &'static [u8],
    i: usize,
    sel: Signal<usize>,
    on_switch: impl Fn(usize) + 'static,
) -> Element {
    let t = windui::theme::current();
    let on_accent = Role::OnAccent.resolve(&t);
    let muted = Role::TextMuted.resolve(&t);

    // 图标块：选中=主题色底 + 白图标，未选中=浅底 + 灰图标
    let chip = |selected: bool| {
        Element::stack()
            .size(26, 26)
            .corner(7.0)
            .bg_role(if selected { Role::Accent } else { Role::Surface })
            .child(
                Element::image_content(
                    ImageContent::from_svg_bytes(icon, Some(14))
                        .tint(if selected { on_accent } else { muted }),
                )
                .align(Align::Center),
            )
    };

    let on = Element::row()
        .width_match()
        .height(38)
        .corner(9.0)
        .cross(Align::Center)
        .spacing(10)
        .padding_xy(10, 0)
        // 淡底而非实底：实底 accent 会把整条侧栏拉成一块高饱和色斑，压过右侧内容。
        .bg_role_alpha(Role::Accent, 0.12)
        .child(chip(true))
        .child(
            Element::label(name)
                .font_size(13.0)
                .font_weight(600)
                .fg_role(Role::Accent)
                .weight(1.0)
                .max_lines(1),
        )
        .visible_when(move || sel.get() == i);

    let off = Element::row()
        .clickable()
        .on_click({
            let on_switch = on_switch;
            move |_| {
                sel.set(i);
                on_switch(i);
            }
        })
        .width_match()
        .height(38)
        .corner(9.0)
        .cross(Align::Center)
        .spacing(10)
        .padding_xy(10, 0)
        .child(chip(false))
        .child(
            Element::label(name)
                .font_size(13.0)
                .font_weight(500)
                .fg_role(Role::TextMuted)
                .weight(1.0)
                .max_lines(1),
        )
        .visible_when(move || sel.get() != i);

    // 左缘指示条：不占行内宽度，故选中/未选中时图标与文字位置完全一致。
    let indicator = Element::row()
        .width_match()
        .height(38)
        .cross(Align::Center)
        .child(
            Element::leaf()
                .width(3)
                .height(16)
                .corner(1.5)
                .bg_role(Role::Accent),
        )
        .visible_when(move || sel.get() == i);

    Element::stack()
        .width_match()
        .height(38)
        .child(on)
        .child(off)
        .child(indicator)
}

/// 官方风格卡片：Surface 底 + 圆角 + 内边距 + 标题 + 分隔线 + 正文
/// （仿 windui examples/fullshowcase.rs 的 `card`）
pub fn card(title: &str, body: Element) -> Element {
    Element::col()
        .width_match()
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding(16)
        .spacing(8)
        .child(
            Element::label(title)
                .font_size(15.0)
                .font_weight(600)
                .fg_role(Role::Text)
                .width_match(),
        )
        .child(Element::divider())
        .child(body)
}

/// 只读可选文本（「拓展 label」）：文本随信号变化、可选中（点击/拖拽/双击选词/三击选行/
/// Shift+方向扩选）、可复制（Ctrl+C / 右键「复制」），但**只读**——不写信号、不可键入。
///
/// 用于各输出表单的文本项（2FA 验证码、Turso 详情页字段值、SSH 命令输出……）：替代
/// 「借多行 `text_input` 承载只读输出」的旧做法（windui 没有只读+可选中的纯文本控件，
/// [`SelectText`] 即为此补齐）。
///
/// **纯文字展示**：无底色、无边框、无圆角（控件自绘文字与选区，焦点时落一个光标）。
/// `SelectText` 是真实控件（`hit_opaque`），整块 bounds 仍能吞命中收点击，故去掉底色
/// 不丢交互。需要底色时由调用方自行加 `.bg_role(..)` 等修饰。
/// 用法：
/// ```ignore
/// select_text(output).width_match().height(120)
/// ```
pub fn select_text(text: Signal<String>) -> Element {
    Element::leaf().widget(SelectText::new(text))
}

/// 弹窗（带输入框专用）：同 `Element::dialog_panel`，但标题栏 ✕ 关闭按钮
/// **不进键盘焦点环**（`.focusable(false)`）。
///
/// 原因：windui 的模态焦点同步（`sync_modal_focus`）在弹窗弹出时把键盘焦点
/// 交给弹窗内**第一个可聚焦控件**；`dialog_panel` 的 ✕ 排在正文之前，焦点
/// 于是落在 ✕ 上——此时 Ctrl+V 粘贴无效（粘贴键只发给有焦点的输入框），
/// 只能右键菜单粘贴（右键菜单是直接把 Ctrl+V 合成给鼠标下的控件）。
/// 本函数让正文输入框成为弹窗内第一个可聚焦控件，弹窗一开即可键入/粘贴，
/// Ctrl+V / Ctrl+A 立即生效。✕ 仍可点击关闭（ESC 关闭不受影响）。
pub fn input_dialog(
    show: Signal<bool>,
    title: impl Into<String>,
    width: i32,
    on_close: impl FnMut(&mut windui::core::EventCtx) + 'static,
    body: Element,
    footer: Element,
) -> Element {
    let th = windui::theme::current();
    let header = Element::row()
        .width_match()
        .cross(Align::Center)
        .child(
            Element::label(title.into())
                .font_size(18.0)
                .font_weight(700)
                .fg_role(Role::Text)
                .weight(1.0)
                .height(26),
        )
        .child(
            Element::icon_button("\u{2715}")
                .size(28, 28)
                .fg_role(Role::TextMuted)
                .focusable(false) // 关键：不进焦点环，弹窗打开时焦点落到正文输入框
                .on_click(on_close),
        );
    let panel = Element::col()
        .width(width)
        .bg_role(Role::Surface)
        .corner(th.metrics.corner_lg)
        .padding(20)
        .spacing(16)
        .child(header)
        .child(body)
        .child(footer);
    Element::dialog(show, panel)
}

/// 侧栏底部主题 toggle（二选一）：无底色 clickable 行，仅图标随状态变化（月亮↔太阳），
/// 点击切换到另一种。启动时默认跟随系统（run() 按 system_light_theme 初始化 mode）；
/// 点击即写入主题句柄热切换，并 bump theme_epoch 触发重建，让图标定色重新解析。
pub fn theme_toggle(mode: Signal<usize>, th: ThemeHandle, epoch: Signal<Vec<()>>) -> Element {
    let t = windui::theme::current();
    let muted = Role::TextMuted.resolve(&t);
    let dark = mode.get() == 1;
    let icon = ImageContent::from_svg_bytes(if dark { icons::MOON } else { icons::SUN }, Some(16))
        .tint(muted);
    Element::stack()
        .clickable()
        .on_click(move |_| {
            let new_dark = mode.get() != 1;
            mode.set(if new_dark { 1 } else { 0 });
            th.set(if new_dark { Theme::dark() } else { Theme::default() });
            epoch.update(|v| v.push(()));
        })
        .weight(1.0)
        .height(28)
        .corner(6.0)
        .tooltip(lang::TOGGLE_THEME())
        .child(Element::image_content(icon).align(Align::Center))
}

/// 关于页面：品牌（Logo + 名称/版本）+ 功能说明 + 仓库链接。
/// 模块名不再在内容区重复：已上移到窗体标题（如「启途 - 关于」），
/// 页面首块即品牌卡（Logo + 名称起头，不再重复标题文案）。
pub fn build_about_page() -> Element {
    let name = lang::APP_NAME();
    Element::col()
        .padding(20)
        .spacing(14)
        .child(
            Element::col()
                .width_match()
                .bg_role(Role::Surface)
                .corner(10.0)
                .padding(20)
                .spacing(10)
                .child(
                    Element::row()
                        .width_match()
                        .spacing(14)
                        .cross(Align::Center)
                        .child(
                            // Logo 自带品牌色，不参与主题染色（ImageContent::tint 会整体染色）
                            Element::image_content(ImageContent::from_svg_bytes(icons::LOGO, Some(48)))
                                .align(Align::Center),
                        )
                        .child(
                            Element::col()
                                .spacing(2)
                                .child(
                                    Element::label(name)
                                        .font_size(22.0)
                                        .font_weight(700)
                                        .fg_role(Role::Text),
                                )
                                .child(
                                    Element::label(format!("v{}", env!("CARGO_PKG_VERSION")))
                                        .font_size(13.0)
                                        .fg_role(Role::TextMuted),
                                ),
                        ),
                )
                .child(Element::divider())
                .child(Element::label(lang::ABOUT_DESC()).font_size(14.0))
                .child(Element::label(lang::ABOUT_2FA()).font_size(13.0))
                .child(Element::label(lang::ABOUT_PWD()).font_size(13.0))
                .child(Element::label(lang::ABOUT_TURSO()).font_size(13.0))
                .child(Element::label(lang::ABOUT_SFTP()).font_size(13.0))
                .child(Element::label(lang::ABOUT_REMOTE()).font_size(13.0))
                .child(
                    Element::link("GitHub: https://github.com/wujianqi/qi-toolbox")
                        .url("https://github.com/wujianqi/qi-toolbox"),
                )
                .child(
                    Element::label(lang::ABOUT_BUILT())
                        .font_size(12.0)
                        .fg_role(Role::TextMuted),
                )
                .child(Element::divider())
                .child(
                    Element::label(lang::ABOUT_LICENSE_TITLE())
                        .font_size(13.0)
                        .font_weight(600),
                )
                .child(
                    Element::label(lang::ABOUT_LICENSE())
                        .font_size(12.0)
                        .fg_role(Role::TextMuted),
                )
                .child(
                    Element::label(lang::ABOUT_ICONS())
                        .font_size(12.0)
                        .fg_role(Role::TextMuted),
                )
                .child(
                    Element::label(lang::ABOUT_THIRD_PARTY())
                        .font_size(12.0)
                        .fg_role(Role::TextMuted),
                ),
        )
}
