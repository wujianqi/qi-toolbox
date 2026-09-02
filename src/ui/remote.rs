//! 远程检测页 UI：连接（弹窗输入网址）+ Ping + SSL 证书状态 + 网页状态
//!
//! 页面状态封装在 [`RemoteUi`]：含全部信号 + 后台任务通道发送端 + 当前端点，
//! 后台消息统一由 [`RemoteUi::on_msg`] 消费；业务逻辑在 [`crate::core::remote`]。

use std::cell::RefCell;
use std::rc::Rc;

use windui::prelude::*;

use super::{card, icons, input_dialog, sink};
use crate::core;
use crate::core::qr::QrEntry;
use crate::core::remote::{self, RemoteEndpoint};
use crate::lang;

/// 远程检测页状态：全部 UI 信号 + 后台任务通道发送端 + 当前端点
#[derive(Clone)]
pub struct RemoteUi {
    /// 连接弹窗显隐
    pub show_conn: Signal<bool>,
    /// 弹窗内网址输入
    pub url_input: Signal<String>,
    /// 已连接（端点解析成功）→ Ping/SSL/网页状态 可用
    pub connected: Signal<bool>,
    /// 当前端点显示
    pub endpoint_display: Signal<String>,
    pub status: Signal<String>,
    pub error: Signal<String>,
    /// 检测结果多行输出（Ping/SSL/网页状态）
    pub result: Signal<String>,
    /// 端点二维码弹窗显隐
    pub show_qr: Signal<bool>,
    /// 端点二维码像素（0 或 1 条，生成后弹窗显示）
    pub qr: Signal<Vec<QrEntry>>,
    /// 后台任务结果通道（`App::channel` 创建后回填）
    tx: Rc<RefCell<Option<Sender<core::remote::RemoteMsg>>>>,
    /// 当前已解析端点（连接成功后回填，Ping/SSL/网页状态复用）
    endpoint: Rc<RefCell<Option<RemoteEndpoint>>>,
}

impl RemoteUi {
    pub fn new() -> Self {
        Self {
            show_conn: signal(false),
            url_input: signal(String::new()),
            connected: signal(false),
            endpoint_display: signal(String::new()),
            status: signal(String::new()),
            error: signal(String::new()),
            result: signal(String::new()),
            show_qr: signal(false),
            qr: signal(Vec::new()),
            tx: Rc::new(RefCell::new(None)),
            endpoint: Rc::new(RefCell::new(None)),
        }
    }

    /// 回填后台任务通道发送端（`App::channel` 返回后调用一次）
    pub fn set_tx(&self, tx: Sender<core::remote::RemoteMsg>) {
        *self.tx.borrow_mut() = Some(tx);
    }

    /// 取后台任务通道发送端（UI 构建前已回填）
    pub fn tx(&self) -> Sender<core::remote::RemoteMsg> {
        self.tx
            .borrow()
            .as_ref()
            .expect("remote tx 已在 run() 中回填")
            .clone()
    }

    /// 消费后台检测消息（`App::channel` 回 UI 线程时调用）
    pub fn on_msg(&self, msg: core::remote::RemoteMsg) {
        match msg {
            core::remote::RemoteMsg::Connected(Ok(info)) => {
                self.status.set(
                    lang::REMOTE_CONNECTED()
                        .replacen("{}", &self.endpoint_display.get(), 1)
                        .replacen("{}", &info.rtt_ms.to_string(), 1)
                        .replacen("{}", &info.ip, 1),
                );
                self.error.set(String::new());
            }
            core::remote::RemoteMsg::Connected(Err(e)) => self.error.set(e),
            core::remote::RemoteMsg::PingDone(Ok(info)) => {
                self.result.set(
                    lang::REMOTE_PING_OK()
                        .replacen("{}", &info.min_ms.to_string(), 1)
                        .replacen("{}", &info.avg_ms.to_string(), 1)
                        .replacen("{}", &info.max_ms.to_string(), 1)
                        .replacen("{}", &info.count.to_string(), 1),
                );
                self.error.set(String::new());
            }
            core::remote::RemoteMsg::PingDone(Err(e)) => self.error.set(e),
            core::remote::RemoteMsg::SslDone(Ok(info)) => {
                let mut text = String::new();
                text.push_str(lang::REMOTE_SSL_TITLE());
                text.push('\n');
                text.push_str(&format!(
                    "{} {}",
                    lang::REMOTE_SSL_SUBJECT(),
                    info.subject
                ));
                text.push('\n');
                text.push_str(&format!(
                    "{} {}",
                    lang::REMOTE_SSL_ISSUER(),
                    info.issuer
                ));
                text.push('\n');
                text.push_str(
                    &lang::REMOTE_SSL_VALID()
                        .replacen("{}", &info.not_before, 1)
                        .replacen("{}", &info.not_after, 1),
                );
                text.push('\n');
                let days_txt = if info.days_left < 0 {
                    lang::REMOTE_SSL_EXPIRED().replace("{}", &(-info.days_left).to_string())
                } else {
                    lang::REMOTE_SSL_DAYS().replace("{}", &info.days_left.to_string())
                };
                text.push_str(&days_txt);
                self.result.set(text);
                self.error.set(String::new());
            }
            core::remote::RemoteMsg::SslDone(Err(e)) => self.error.set(e),
            core::remote::RemoteMsg::WebDone(Ok(info)) => {
                let mut text = lang::REMOTE_WEB_OK()
                    .replacen("{}", &info.status_code.to_string(), 1)
                    .replacen("{}", &info.reason, 1)
                    .replacen("{}", &info.bytes.to_string(), 1)
                    .replacen("{}", &info.rtt_ms.to_string(), 1);
                if info.redirects > 0 {
                    text.push('\n');
                    text.push_str(&format!(
                        "{} {}",
                        lang::REMOTE_WEB_REDIRECTS(),
                        info.redirects
                    ));
                    text.push('\n');
                    text.push_str(&format!("{} {}", lang::REMOTE_WEB_FINAL(), info.final_url));
                }
                if !info.content_type.is_empty() {
                    text.push('\n');
                    text.push_str(&format!(
                        "{} {}",
                        lang::REMOTE_WEB_CTYPE(),
                        info.content_type
                    ));
                }
                if !info.server.is_empty() {
                    text.push('\n');
                    text.push_str(&format!("{} {}", lang::REMOTE_WEB_SERVER(), info.server));
                }
                self.result.set(text);
                self.error.set(String::new());
            }
            core::remote::RemoteMsg::WebDone(Err(e)) => self.error.set(e),
        }
    }
}

impl Default for RemoteUi {
    fn default() -> Self {
        Self::new()
    }
}

/// 远程检测页：连接（弹窗输入网址）+ Ping + SSL 证书状态 + 网页状态
pub fn build_remote_tab(ui: &RemoteUi) -> Element {
    let RemoteUi {
        show_conn,
        url_input,
        connected,
        endpoint_display,
        status,
        error,
        result,
        show_qr,
        qr,
        ..
    } = ui.clone();
    let tx = ui.tx();
    let endpoint = ui.endpoint.clone();

    // ── 连接：弹出网址输入窗（解析成功即视为已连接，可执行后续检测）──
    let connect_btn = Element::button(lang::REMOTE_CONNECT())
        .small()
        .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
        .on_click({
            let endpoint = endpoint.clone();
            move |_| {
                // 再次连接时预填当前端点（便于改端口/路径重连）
                let ep = endpoint.borrow().clone();
                url_input.set(ep.map(|e| e.display).unwrap_or_default());
                show_conn.set(true);
            }
        });

    // ── Ping：连接前不可用（TCP 建连时延）──
    let ping_btn = Element::button(lang::REMOTE_PING())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::ZAP, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let tx = tx.clone();
            let endpoint = endpoint.clone();
            move |_| {
                let ep = endpoint.borrow().clone();
                if let Some(ep) = ep {
                    status.set(lang::REMOTE_BUSY().to_string());
                    remote::spawn_ping(sink(tx.clone()), ep);
                }
            }
        });

    // ── SSL 证书状态：连接前不可用（仅 https/wss）──
    let ssl_btn = Element::button(lang::REMOTE_SSL())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::SHIELD, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let tx = tx.clone();
            let endpoint = endpoint.clone();
            move |_| {
                let ep = endpoint.borrow().clone();
                if let Some(ep) = ep {
                    status.set(lang::REMOTE_BUSY().to_string());
                    remote::spawn_check_ssl(sink(tx.clone()), ep);
                }
            }
        });

    // ── 网页状态：连接前不可用（仅 http/https，返回 HTTP 状态码）──
    let web_btn = Element::button(lang::REMOTE_WEB())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::GLOBE, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let tx = tx.clone();
            let endpoint = endpoint.clone();
            move |_| {
                let ep = endpoint.borrow().clone();
                if let Some(ep) = ep {
                    status.set(lang::REMOTE_BUSY().to_string());
                    remote::spawn_web_status(sink(tx.clone()), ep);
                }
            }
        });

    // ── 端点二维码：生成当前地址二维码，便于扫码分发（连接后可用）──
    let qr_btn = Element::button(lang::REMOTE_QR())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::QR, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let endpoint = endpoint.clone();
            move |_| {
                let ep = endpoint.borrow().clone();
                if let Some(ep) = ep {
                    match crate::core::qr::qr_rgba(&ep.display) {
                        Ok((rgba, w, h)) => {
                            qr.set(vec![QrEntry { id: 0, rgba, w, h }]);
                            error.set(String::new());
                            show_qr.set(true);
                        }
                        Err(e) => error.set(e),
                    }
                }
            }
        });

    // ── 工具栏：连接 / Ping / SSL / 网页状态 / 二维码 + 状态条 ──
    let toolbar = Element::row()
        .width_match()
        .spacing(8)
        .cross(Align::Center)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding_xy(12, 10)
        .child(connect_btn)
        .child(ping_btn)
        .child(ssl_btn)
        .child(web_btn)
        .child(qr_btn)
        .child(Element::leaf().width(1).height(20).bg_role(Role::Divider))
        .child(Element::flex_spacer())
        .child(
            Element::label_signal(status)
                .font_size(11.0)
                .fg_role(Role::TextMuted),
        )
        .child(
            Element::label_signal(error)
                .font_size(11.0)
                .fg_role(Role::Danger),
        );

    // ── 当前端点行 ──
    let endpoint_row = Element::row()
        .width_match()
        .spacing(8)
        .cross(Align::Center)
        .child(
            Element::label(lang::REMOTE_ENDPOINT())
                .font_size(12.0)
                .fg_role(Role::TextMuted),
        )
        .child(
            Element::label_signal(endpoint_display)
                .font_size(12.0)
                .fg_role(Role::TextMuted)
                .weight(1.0)
                .max_lines(1),
        );

    // ── 结果卡片：多行只读输出区 ──
    let result_card = card(
        lang::REMOTE_CARD_RESULT(),
        Element::col()
            .spacing(6)
            .child(
                Element::text_input(result, "")
                    .multiline()
                    .font_family("Consolas")
                    .font_size(13.0)
                    .width_match()
                    .height(220),
            )
            .child(
                Element::label(lang::REMOTE_RESULT_HINT())
                    .font_size(11.0)
                    .fg_role(Role::TextMuted),
            ),
    );

    // ── 连接弹窗：网址输入 + 确定/取消 ──
    let conn_dialog = input_dialog(
        show_conn,
        lang::REMOTE_DIALOG_TITLE(),
        420,
        move |_| show_conn.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(Element::label(lang::REMOTE_URL_LABEL()).font_size(13.0))
            .child(Element::text_input(url_input, lang::REMOTE_URL_HINT()).width_match())
            .child(
                Element::label(lang::REMOTE_URL_SUPPORT())
                    .font_size(11.0)
                    .fg_role(Role::TextMuted),
            )
            // 解析失败的错误就地显示在弹窗内（弹窗保持打开，便于修改重试）
            .child(
                Element::label_signal(error)
                    .font_size(11.0)
                    .fg_role(Role::Danger),
            ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click(move |_| show_conn.set(false)),
            )
            .child(
                Element::button(lang::SFTP_OK())
                    .small()
                    .on_click({
                        let tx = tx.clone();
                        let endpoint = endpoint.clone();
                        move |_| {
                            match remote::parse_endpoint(&url_input.get()) {
                                Ok(ep) => {
                                    let display = ep.display.clone();
                                    *endpoint.borrow_mut() = Some(ep.clone());
                                    connected.set(true);
                                    endpoint_display.set(display);
                                    status.set(String::new());
                                    error.set(String::new());
                                    result.set(String::new());
                                    // 解析成功即发起连接探测（TCP + 可选 TLS）
                                    remote::spawn_connect(sink(tx.clone()), ep);
                                    show_conn.set(false);
                                }
                                // 解析失败：不关闭弹窗，错误在弹窗内显示
                                Err(e) => error.set(e),
                            }
                        }
                    }),
            ),
    );

    // ── 端点二维码弹窗：二维码 + 可复制地址文本（供扫码分发）──
    let qr_view = Element::list_signal(
        qr,
        |e: &QrEntry| e.id,
        |e: QrEntry| {
            Element::image_rgba(e.w, e.h, &e.rgba)
                .fit(Fit::Contain)
                .corner(6.0)
                .bg_role(Role::Surface)
        },
    );
    let qr_dialog = input_dialog(
        show_qr,
        lang::REMOTE_QR_TITLE(),
        320,
        move |_| show_qr.set(false),
        Element::col()
            .width_match()
            .cross(Align::Center)
            .spacing(12)
            .child(qr_view)
            // 地址用可换行/可复制的输入框承载（label 不支持 wrap，长 URL 会被裁掉）
            .child(
                Element::text_input(endpoint_display, "")
                    .multiline()
                    .wrap(true)
                    .font_size(12.0)
                    .width_match()
                    .height(52),
            ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click(move |_| show_qr.set(false)),
            ),
    );

    // ── 页面：工具栏 + 当前端点 + 结果卡片（+ 连接/二维码弹窗浮层）──
    Element::stack()
        .fill()
        .child(
            Element::col()
                .padding(12)
                .spacing(8)
                .child(
                    Element::label(lang::REMOTE_TITLE())
                        .font_size(18.0)
                        .font_weight(700)
                        .padding_xy(8, 4),
                )
                .child(toolbar)
                .child(endpoint_row)
                .child(result_card.weight(1.0)),
        )
        .child(conn_dialog)
        .child(qr_dialog)
}
