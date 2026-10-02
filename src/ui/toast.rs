//! 全局轻提示（toast）助手：消息一律走 windui 自带 toast 浮层（居中、淡入淡出、
//! 定时自动消失），不再用页面内的常驻状态/错误行占位。
//!
//! 后台消息处理器（app.channel 回调之外线程发回、由 channel 消费的 on_msg）拿不到
//! `EventCtx`，因此 `run()` 里注册一条全局通道，把文案回 UI 线程经 `ctx.toast*`
//! 弹出；控件回调里能直接拿 ctx 的地方优先用 `ctx.toast_ok/toast_err`。

use windui::prelude::*;

pub(crate) enum ToastMsg {
    Info(String),
    Ok(String),
    Err(String),
}

thread_local! {
    static TX: std::cell::RefCell<Option<Sender<ToastMsg>>> =
        const { std::cell::RefCell::new(None) };
}

/// run() 启动期注册：通道回调持有 EventCtx，是唯一能弹 toast 的入口。
pub(crate) fn register(app: &mut App) {
    let tx = app.channel::<ToastMsg>(move |ctx, msg| match msg {
        ToastMsg::Info(t) => ctx.toast(t),
        ToastMsg::Ok(t) => ctx.toast_ok(t),
        ToastMsg::Err(t) => ctx.toast_err(t),
    });
    TX.with(|v| *v.borrow_mut() = Some(tx));
}

/// 中性信息（ℹ，3s）。
pub(crate) fn info(text: impl Into<String>) {
    send(ToastMsg::Info(text.into()));
}
/// 成功（✓，3s）。
pub(crate) fn ok(text: impl Into<String>) {
    send(ToastMsg::Ok(text.into()));
}
/// 失败/错误（✕，5s）。
pub(crate) fn err(text: impl Into<String>) {
    send(ToastMsg::Err(text.into()));
}

fn send(msg: ToastMsg) {
    TX.with(|v| {
        if let Some(tx) = v.borrow().as_ref() {
            let _ = tx.send(msg);
        }
    });
}
