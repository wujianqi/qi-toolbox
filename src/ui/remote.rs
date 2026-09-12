//! 远程检测页 UI：连接（弹窗输入网址）+ Ping + SSL 证书状态 + 网页状态
//!
//! 页面状态封装在 [`RemoteUi`]：含全部信号 + 后台任务通道发送端 + 当前端点，
//! 后台消息统一由 [`RemoteUi::on_msg`] 消费；业务逻辑在 [`crate::core::remote`]。

use std::cell::RefCell;
use std::rc::Rc;

use windui::prelude::*;

use super::{card, icons, input_dialog, select_text, sink};
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
    /// 二维码像素（0 或 1 条，生成后弹窗显示）
    pub qr: Signal<Vec<QrEntry>>,
    /// 二维码弹窗内网址输入（已连接默认带入连接网址，可手动修改）
    pub qr_url: Signal<String>,
    /// 二维码弹窗内错误（输入为空/生成失败就地提示）
    pub qr_error: Signal<String>,
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
            qr_url: signal(String::new()),
            qr_error: signal(String::new()),
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

/// 远程检测页：连接（弹窗输入网址）+ Ping + SSL 证书状态 + 网页状态 + 网址二维码
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
        qr_url,
        qr_error,
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
                // 再次连接时预填当前端点（便于改端口/路径重连）；
                // 无已连接端点时保留输入框现值（含启动时回填的上次网址）
                if let Some(ep) = endpoint.borrow().clone() {
                    url_input.set(ep.display);
                }
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

    // ── 网址二维码：生成任意网址的二维码，便于扫码分发。
    // 不依赖连接（默认亮）：已连接时默认读连接网址，未连接也可手动输入网址生成。──
    let qr_btn = Element::button(lang::REMOTE_QR())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::QR, Some(16)))
        .on_click({
            let endpoint = endpoint.clone();
            move |_| {
                qr_error.set(String::new());
                // 已连接：弹窗内默认带入连接网址并直接生成；未连接：留空待手动输入
                let url = endpoint.borrow().as_ref().map(|e| e.display.clone()).unwrap_or_default();
                qr_url.set(url.clone());
                if url.trim().is_empty() {
                    qr.set(Vec::new());
                } else {
                    match crate::core::qr::qr_rgba(&url) {
                        Ok((rgba, w, h)) => qr.set(vec![QrEntry { id: 0, rgba, w, h }]),
                        Err(e) => qr_error.set(e),
                    }
                }
                show_qr.set(true);
            }
        });

    // ── 工具栏：连接 / Ping / SSL / 网页状态 ‖ 二维码（不依赖连接）+ 状态条 ──
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
        .child(Element::leaf().width(1).height(20).bg_role(Role::Divider))
        .child(qr_btn)
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
                // 只读可选文本：结果可拖选/Ctrl+C 复制（不再借输入框承载）
                select_text(result)
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
                            // 记忆上次连接网址（尽力而为，下次启动自动回填）
                            let url_s = url_input.get();
                            crate::core::settings::commit(&[(
                                "remote.url",
                                Some(url_s.trim()),
                            )]);
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

    // ── 网址二维码弹窗：二维码 + 可编辑网址输入。
    // 已连接默认带入连接网址；未连接留空，可手动输入网址后点「生成」。──
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
        380,
        move |_| show_qr.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(Element::col().width_match().cross(Align::Center).child(qr_view))
            .child(
                Element::label(lang::REMOTE_URL_LABEL())
                    .font_size(12.0)
                    .fg_role(Role::TextMuted),
            )
            // 网址用可换行输入框承载（label 不支持 wrap，长 URL 会被裁掉），支持手动编辑
            .child(
                Element::text_input(qr_url, lang::REMOTE_URL_HINT())
                    .multiline()
                    .wrap(true)
                    .font_size(12.0)
                    .width_match()
                    .height(52),
            )
            // 生成失败/为空就地提示（弹窗保持打开，便于修改重试）
            .child(
                Element::label_signal(qr_error)
                    .font_size(11.0)
                    .fg_role(Role::Danger),
            )
            .child(
                Element::label(lang::REMOTE_QR_SUPPORT())
                    .font_size(11.0)
                    .fg_role(Role::TextMuted),
            ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click(move |_| show_qr.set(false)),
            )
            .child(
                Element::button(lang::REMOTE_QR_GEN())
                    .small()
                    .on_click(move |_| {
                        let text = qr_url.get();
                        if text.trim().is_empty() {
                            qr_error.set(lang::REMOTE_QR_EMPTY().to_string());
                            return;
                        }
                        match crate::core::qr::qr_rgba(text.trim()) {
                            Ok((rgba, w, h)) => {
                                qr.set(vec![QrEntry { id: 0, rgba, w, h }]);
                                qr_error.set(String::new());
                            }
                            Err(e) => qr_error.set(e),
                        }
                    }),
            ),
    );

    // ── 页面：工具栏 + 当前端点 + 结果卡片（+ 连接/二维码弹窗浮层）──
    Element::stack()
        .fill()
        .child(
            Element::col()
                .padding(12)
                .spacing(8)
                .child(toolbar)
                .child(endpoint_row)
                .child(result_card.weight(1.0)),
        )
        .child(conn_dialog)
        .child(qr_dialog)
}
