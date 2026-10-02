//! 扩展基础组件：卡片 / 输入弹窗 / 站点管理弹窗 / 文件选择 / PNG 保存等
//!
//! 与业务无关的基础性 UI 组件；各功能页面文件复用。
//! 侧栏导航项 / 主题按钮 / 关于页在 [`crate::ui::nav`]。

use windui::prelude::*;

use super::select_text::SelectText;
use crate::lang;

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

/// 管理弹窗左栏：列表列（统一形态）。小标题 + 圆角描边滚动列表。
///
/// 各管理弹窗（站点/库址）的左栏此前是裸 `SurfaceAlt` 底色块，无标题、
/// 与右栏表单视觉重量不平衡；统一收口到这里，改形态只动一处。
///
/// `is_empty` 为真时列表中央叠一行置灰占位提示（引导点「新建」），
/// 避免空列表只剩一块空白底色。
pub fn mgr_list_col(list: Element, is_empty: Signal<bool>) -> Element {
    Element::col()
        .width(200)
        .height_match()
        .spacing(6)
        .child(
            Element::label(lang::MGR_LIST())
                .font_size(11.0)
                .font_weight(600)
                .fg_role(Role::TextMuted),
        )
        .child(
            Element::stack()
                .width_match()
                .height_match()
                .child(
                    Element::scroll()
                        .width_match()
                        .height_match()
                        .bg_role(Role::SurfaceAlt)
                        .border_role(Role::Border, 1)
                        .corner(8.0)
                        .child(Element::col().width_match().padding_xy(4, 4).child(list)),
                )
                .child(
                    // 空态占位：列表区正中一行置灰提示（隐藏时不渲染、不拦截命中）
                    Element::col()
                        .fill()
                        .align_xy(Align::Center, Align::Center)
                        .visible_when(move || is_empty.get())
                        .child(
                            Element::label(lang::MGR_EMPTY())
                                .font_size(11.0)
                                .fg_role(Role::TextMuted),
                        ),
                ),
        )
}

/// 管理弹窗右栏：表单列（统一形态）。表单包进标准卡片，与左栏列表对仗。
///
/// `card()` 自带「连接信息」标题 + 分隔线，字段分组一目了然；
/// 高度撑满与左栏对齐，末尾由调用方决定是否加 `flex_spacer`。
pub fn mgr_form_col(form: Element) -> Element {
    Element::col()
        .width_match()
        .weight(1.0)
        .height_match()
        .child(card(&lang::MGR_FORM(), form).height_match())
}

/// 管理类弹窗（库址/站点统一形态）：标准标题栏（标题 + ✕），按钮区固定三枚、
/// 位置统一——`新建` 左，`删除` `保存` 右。
///
/// 与 [`input_dialog`] 的差异仅在按钮区：去掉「确定」，固定三按钮布局；
/// 按钮区上方加分隔线，与正文拉开层次。
// 弹窗形态固定（标题栏 + 三按钮），8 个参数均为语义明确的独立部件，强行聚合
// 反而增加调用点噪音。
#[allow(clippy::too_many_arguments)]
pub fn mgr_dialog(
    show: Signal<bool>,
    title: impl Into<String>,
    width: i32,
    on_close: impl FnMut(&mut windui::core::EventCtx) + 'static,
    body: Element,
    btn_new: Element,
    btn_del: Element,
    btn_save: Element,
) -> Element {
    let th = windui::theme::current();
    // 页脚「关闭」按钮与标题栏 ✕ 共用关闭回调：FnMut 闭包不可克隆，
    // 包一层 Rc<RefCell<dyn FnMut>> 供两处按钮共享
    type CloseFn = dyn FnMut(&mut windui::core::EventCtx);
    let on_close: std::rc::Rc<std::cell::RefCell<CloseFn>> =
        std::rc::Rc::new(std::cell::RefCell::new(on_close));
    let close_btn = {
        let on_close = on_close.clone();
        Element::button(crate::lang::DT_CLOSE())
            .small()
            .neutral()
            .on_click(move |ctx| (on_close.borrow_mut())(ctx))
    };
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
                .on_click(move |ctx| (on_close.borrow_mut())(ctx)),
        );
    let footer = Element::col()
        .width_match()
        .spacing(10)
        .child(Element::divider())
        .child(
            Element::row()
                .width_match()
                .spacing(8)
                .child(btn_new)
                .child(Element::flex_spacer())
                .child(btn_del)
                .child(btn_save)
                .child(close_btn),
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

/// 把 RGBA 像素编码为 PNG 写入指定路径（8-bit RGBA，无滤镜压缩默认档）。
/// 供 TOTP / 远程检测两页的二维码保存共用。
pub fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

/// 本地文件选择对话框（rfd 不可用时返回 None 静默取消）。
/// 供 S3 上传等场景共用。
pub fn pick_file() -> Option<String> {
    rfd::FileDialog::new()
        .pick_file()
        .map(|p| p.to_string_lossy().into_owned())
}

/// 本地目录选择对话框。供 S3 下载目标等场景共用。
pub fn pick_dir() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().into_owned())
}

/// PNG 保存对话框（过滤 PNG、默认文件名）+ 写入。
/// 供 TOTP / 远程检测两页的二维码保存共用；取消选择返回 `Ok(None)`。
pub fn save_qr_png(
    default_name: &str,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> Result<Option<String>, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG", &["png"])
        .set_file_name(default_name)
        .save_file()
    else {
        return Ok(None);
    };
    write_png(&path, w, h, rgba)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
