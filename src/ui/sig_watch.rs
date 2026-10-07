//! 信号版本监听：对「下拉内部写入的选中索引」这类无回调的信号做差分，执行副作用。
//!
//! windui 的 Dropdown 控件不接 `Element::on_click`（`Widget::take_click` 未实现，
//! Builder 注入的点击回调被静默丢弃），菜单项点击只写 `selected` 信号本身。
//! 要响应「用户选中了某项」，用 [`sig_watch`] 监听该信号的版本号（`Signal::version`
//! 每次写入自增，重复选中同项也能检测到），版本变化即调回调（带 `EventCtx`）。
//!
//! 返回的零尺寸叶子元素必须 `.reactive()` 后挂进控件树才会参与版本跟踪
//! （先例：sftp 页书签跳转）。

use windui::core::{EventCtx, Widget};
use windui::geometry::Size;
use windui::prelude::*;
use windui::style::Style;
use windui::text::TextEngine;

struct SigWatch<F: FnMut(&mut EventCtx)> {
    ver: Box<dyn Fn() -> u64>,
    last: u64,
    f: F,
}

impl<F: FnMut(&mut EventCtx)> Widget for SigWatch<F> {
    fn measure(&self, _a: Size, _s: &Style, _t: &mut dyn TextEngine) -> Size {
        Size::ZERO
    }

    fn on_update(&mut self, ctx: &mut EventCtx) {
        let v = (self.ver)();
        if v == self.last {
            return;
        }
        self.last = v;
        (self.f)(ctx);
    }
}

/// 监听 `version`（一般传 `|| sig.version()`）变化，变化即调 `on_change`。
///
/// `fire_first=false`：以构造时的版本为基准，只响应之后的变化（用于
/// 「选中项回填输入框」——启动回填已由 settings 恢复负责，首帧不重放）；
/// `fire_first=true`：首次 on_update 必跑一次（用于「打开软件即执行一次」
/// 的场景，如 sftp 书签跳转）。
pub(crate) fn sig_watch(
    version: impl Fn() -> u64 + 'static,
    fire_first: bool,
    on_change: impl FnMut(&mut EventCtx) + 'static,
) -> Element {
    let init = version();
    Element::leaf().widget(SigWatch {
        ver: Box::new(version),
        last: if fire_first { u64::MAX } else { init },
        f: on_change,
    })
}
