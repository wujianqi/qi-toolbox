//! 侧栏导航项 / 主题切换按钮 / 关于页（原 widgets 中导航相关部分）
//!
//! 纯展示组件（与业务无关的基础控件）在 [`crate::widgets`]。

use windui::prelude::*;

use super::icons;
use crate::lang;
use crate::ui::UpdateStatus;
use crate::widgets::select_text;

/// 关于页说明文本的信号：全局只建一次（thread_local 槽位永生）。
/// 不能在 `build_about_page` 里每次重建都 `signal(..)` 新建——主题切换整树重建
/// 会回收旧子树的构建期信号，控件持有的句柄即成死句柄，再读即 panic。
/// 内容在 [`build_about_page`] 构建时刷新（首建即填入），语言切换后随重建更新文案。
fn about_text_sig() -> Signal<String> {
    use std::cell::RefCell;
    thread_local! {
        static SIG: RefCell<Option<Signal<String>>> = const { RefCell::new(None) };
    }
    SIG.with(|v| {
        let mut g = v.borrow_mut();
        // 自愈：首建若发生在整树重建的 SignalScope 内会被收走（is_alive=false），
        // 此时换新槽位重建——新句柄归属当轮作用域，每轮重建后随读随补，永不残留死句柄。
        if !g.as_ref().is_some_and(|s| s.is_alive()) {
            *g = Some(signal(about_text()));
        }
        // 前一步刚回填，必然可取；用 clone 解包避免 panic 路径
        g.as_ref()
            .and_then(|s| s.is_alive().then_some(*s))
            .unwrap_or_else(|| signal(about_text()))
    })
}

/// 侧栏导航项：图标块 + 名称，选中/未选中两棵子树叠放互斥显示，
/// 左缘指示条作为**覆盖层**贴在行左沿。
///
/// `on_switch`：点击切换后回调（模块 id），供 UI 层同步窗体标题等。
/// 不用改 padding / border 来表达选中：那样切换时图标与文字会横向跳一下。
/// 两态的内边距完全一致，动的只有底色、图标底色与字重/字色。
/// （仿 windui examples/settings.rs 的 `nav_item`，字形换成 SVG 图标）
pub fn nav_item(
    name: &str,
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
            .bg_role(if selected {
                Role::Accent
            } else {
                Role::Surface
            })
            .child(
                Element::image_content(
                    ImageContent::from_svg_bytes(icon, None).tint(if selected {
                        on_accent
                    } else {
                        muted
                    }),
                )
                // 矢量源固有 24dp：钉回原 Some(14) 的逻辑尺寸
                .width(14)
                .height(14)
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

/// 侧栏底部主题 toggle（二选一）：无底色 clickable 行，仅图标随状态变化（月亮↔太阳），
/// 点击切换到另一种。启动时默认跟随系统（run() 按 system_light_theme 初始化 mode）；
/// 点击即写入主题句柄热切换，并 bump theme_epoch 触发重建，让图标定色重新解析。
pub fn theme_toggle(mode: Signal<usize>, th: ThemeHandle, epoch: Signal<Vec<()>>) -> Element {
    let t = windui::theme::current();
    let muted = Role::TextMuted.resolve(&t);
    let dark = mode.get() == 1;
    let icon =
        ImageContent::from_svg_bytes(if dark { icons::MOON } else { icons::SUN }, None).tint(muted);
    Element::stack()
        .clickable()
        .on_click(move |_| {
            let new_dark = mode.get() != 1;
            mode.set(if new_dark { 1 } else { 0 });
            th.set(if new_dark {
                crate::ui::theme::dark()
            } else {
                crate::ui::theme::light()
            });
            epoch.update(|v| v.push(()));
        })
        .weight(1.0)
        .height(28)
        .corner(6.0)
        .tooltip(lang::TOGGLE_THEME())
        .child(
            Element::image_content(icon)
                // 矢量源固有 24dp：钉回原 Some(16) 的逻辑尺寸
                .width(16)
                .height(16)
                .align(Align::Center),
        )
}

/// 关于页大段说明文本（\n 分行，供只读可选文本控件展示与复制）
fn about_text() -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        lang::ABOUT_DESC(),
        lang::ABOUT_AUTHOR(),
        lang::ABOUT_BUILT(),
        lang::ABOUT_LICENSE_TITLE(),
        lang::ABOUT_LICENSE()
    )
}

/// 关于页面：品牌（Logo + 名称/版本）+ 功能说明 + 仓库链接。
/// 模块名不再在内容区重复：已上移到窗体标题（如「启途 - 关于」），
/// 页面首块即品牌卡（Logo + 名称起头，不再重复标题文案）。
pub fn build_about_page() -> Element {
    let name = lang::APP_NAME();
    // 文案随当前语言刷新：语言切换经 theme_epoch 整树重建走到这里
    about_text_sig().set(about_text());
    // 更新检查状态（启动自动检查/手动检查共用；Idle 静默不占位）
    let status = crate::ui::update_status();
    // 外层列撑满内容区，卡片 weight 瓜分剩余高度——内容区不满半屏的旧观感
    // （外层 Wrap 高度只按内容收缩）即由此修复
    Element::col().fill().padding(20).spacing(14).child(
        Element::col()
            .width_match()
            .weight(1.0)
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
                        Element::image_content(ImageContent::from_svg_bytes(icons::LOGO, None))
                            // 矢量源固有 100dp（viewBox 100×100）：钉回原 Some(48) 的逻辑尺寸
                            .width(48)
                            .height(48)
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
            .child(
                // 更新状态行：有新版=提示+下载链接；已最新/失败=结果文案；
                // 检查中=禁用按钮。启动自动检查无新版保持 Idle 静默不占位。
                Element::row()
                    .width_match()
                    .spacing(10)
                    .cross(Align::Center)
                    .child(
                        Element::label_signal(status.map(|s: &UpdateStatus| match s {
                            UpdateStatus::New(v) => lang::ABOUT_NEW_VERSION(v),
                            UpdateStatus::UpToDate => lang::ABOUT_UP_TO_DATE(),
                            UpdateStatus::Failed => lang::ABOUT_CHECK_FAILED(),
                            _ => String::new(),
                        }))
                        .font_size(12.0)
                        .fg_role(Role::Accent)
                        .visible_when(move || {
                            matches!(
                                status.get(),
                                UpdateStatus::New(_)
                                    | UpdateStatus::UpToDate
                                    | UpdateStatus::Failed
                            )
                        }),
                    )
                    .child(
                        Element::link(lang::ABOUT_GOTO_RELEASES())
                            .url("https://github.com/wujianqi/qi-toolbox/releases")
                            .visible_when(move || matches!(status.get(), UpdateStatus::New(_))),
                    )
                    .child(
                        // 手动检查更新：请求在后台线程执行，结果经通道回写状态信号；
                        // 检查中禁用防连点
                        Element::button(lang::ABOUT_CHECK_UPDATE())
                            .neutral()
                            .icon_content(icons::stateful_icon(icons::REFRESH))
                            .small()
                            .enabled_when(move || status.get() != UpdateStatus::Checking)
                            .on_click(move |_| {
                                status.set(UpdateStatus::Checking);
                                crate::ui::spawn_update_check();
                            }),
                    )
                    .child(
                        // 打开日志目录：排查用户反馈问题的第一入口
                        Element::button(lang::ABOUT_OPEN_LOGS())
                            .neutral()
                            .icon_content(icons::stateful_icon(icons::INFO))
                            .small()
                            .on_click(|_| {
                                let dir = crate::core::log::dir();
                                let _ = std::fs::create_dir_all(&dir);
                                // Windows 直接唤起资源管理器（避免为打开目录新增依赖）
                                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                            }),
                    ),
            )
            .child(
                // 大段说明用只读可选文本承载：\n 换行、可拖选复制；
                // weight 弹性瓜分卡片剩余高度（固定 height 只有一截）
                select_text(about_text_sig())
                    .font_size(13.0)
                    .width_match()
                    .weight(1.0),
            )
            .child(
                Element::link("GitHub: https://github.com/wujianqi/qi-toolbox")
                    .url("https://github.com/wujianqi/qi-toolbox"),
            )
            .child(Element::divider())
            .child(
                // 赞助：微信收款码 + 提示文案（编译期嵌入，不依赖外部文件）
                Element::row()
                    .width_match()
                    .spacing(14)
                    .child(
                        Element::image_content(ImageContent::from_bytes(include_bytes!(
                            "../myqr.png"
                        )))
                        .fit(Fit::Contain)
                        .width(110)
                        .height(110)
                        .corner(6.0),
                    )
                    .child(
                        Element::label(lang::ABOUT_SPONSOR())
                            .font_size(13.0)
                            .fg_role(Role::TextMuted)
                            .weight(1.0),
                    ),
            ),
    )
}
