//! 远程检测页 UI：连接（弹窗输入网址）+ Ping + SSL 证书状态 + 网页状态
//!
//! 页面状态封装在 [`RemoteUi`]：含全部信号 + 后台任务通道发送端 + 当前端点，
//! 后台消息统一由 [`RemoteUi::on_msg`] 消费；业务逻辑在 [`crate::core::remote`]。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windui::prelude::*;

use super::{card, icons, input_dialog, select_text, sink};
use crate::core;
use crate::core::qr::QrEntry;
use crate::core::remote::{self, RemoteEndpoint};
use crate::lang;

/// 检测项：结果区按 [`CHECK_ORDER`] 固定顺序分节展示。
/// 各项结果独立保存，单项完成只更新自己的节——并发检测（如连接后自动跑的
/// 响应体 + SEO）与手动点按钮互不冲刷，新结果追加在已有内容之后。
#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckKind {
    Ping,
    Ssl,
    Web,
    Sec,
    Seo,
}

/// 结果区分节展示的固定顺序
const CHECK_ORDER: [CheckKind; 5] = [
    CheckKind::Ping,
    CheckKind::Ssl,
    CheckKind::Web,
    CheckKind::Sec,
    CheckKind::Seo,
];

/// 分节之间的分隔行
const SECTION_SEP: &str = "\n──────────\n\n";

impl CheckKind {
    /// 检测项名称（错误提示里标明来源）
    fn label(self) -> String {
        match self {
            CheckKind::Ping => lang::REMOTE_PING(),
            CheckKind::Ssl => lang::REMOTE_SSL(),
            CheckKind::Web => lang::REMOTE_WEB(),
            CheckKind::Sec => lang::REMOTE_SEC(),
            CheckKind::Seo => lang::SEO_TITLE(),
        }
    }
}

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
    /// 检测结果多行输出（Ping/SSL/网页状态/安全/SEO 分节拼接，见 [`CHECK_ORDER`]）
    pub result: Signal<String>,
    /// 响应体详情弹窗显隐（结果区只留状态摘要，点「响应体」按钮查看详情）
    pub show_web_detail: Signal<bool>,
    /// 响应体详情文本（网页状态完成时回填，弹窗内展示）
    pub web_detail: Signal<String>,
    /// 本次网页状态是否由「响应体」按钮发起（完成后自动弹详情窗；连接自动跑的不弹）
    web_requested: Signal<bool>,
    /// 各检测项的最新结果（None = 尚未检测/已被清空），展示时按 [`CHECK_ORDER`] 拼接
    sections: Rc<RefCell<[Option<String>; CHECK_ORDER.len()]>>,
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
    /// 已保存网址（store.db remote_urls 表，下拉选择/删除）
    pub saved_urls: Signal<Vec<String>>,
    /// 已存网址下拉选中下标
    pub url_sel: Signal<usize>,
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
            show_web_detail: signal(false),
            web_detail: signal(String::new()),
            web_requested: signal(false),
            sections: Rc::new(RefCell::new([const { None }; CHECK_ORDER.len()])),
            show_qr: signal(false),
            qr: signal(Vec::new()),
            qr_url: signal(String::new()),
            qr_error: signal(String::new()),
            tx: Rc::new(RefCell::new(None)),
            endpoint: Rc::new(RefCell::new(None)),
            saved_urls: signal(crate::core::store::remote_url_list().unwrap_or_default()),
            url_sel: signal(0usize),
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
        // 单项结果写回自己的节（按固定顺序拼接，不冲刷其它项），并清"检测中…"状态
        let set_section = |kind: CheckKind, text: String| {
            self.sections.borrow_mut()[kind as usize] = Some(text);
            self.refresh_result();
            self.status.set(String::new());
        };
        // 单项失败：就地提示（标明来源），同样清"检测中…"状态
        let set_err = |kind: CheckKind, e: String| {
            self.sections.borrow_mut()[kind as usize] = None;
            self.refresh_result();
            self.status.set(String::new());
            self.error.set(format!("{}: {}", kind.label(), e));
        };
        match msg {
            core::remote::RemoteMsg::Connected(Ok(info)) => {
                // 状态行只显示 RTT 与 IP（不重复端点网址，避免过长）
                self.status
                    .set(lang::REMOTE_CONNECTED(info.rtt_ms as u64, &info.ip));
                self.error.set(String::new());
                // 连接成功即自动跑全套基础检测：Ping + SSL 证书 + 响应体（网页状态）+ SEO
                if let Some(ep) = self.endpoint.borrow().clone() {
                    remote::spawn_ping(sink(self.tx()), ep.clone());
                    remote::spawn_check_ssl(sink(self.tx()), ep.clone());
                    remote::spawn_web_status(sink(self.tx()), ep);
                }
            }
            core::remote::RemoteMsg::Connected(Err(e)) => self.error.set(e),
            core::remote::RemoteMsg::PingDone(r) => match r {
                Ok(info) => {
                    let mut text = lang::REMOTE_PING_OK(
                        info.min_ms as u64,
                        info.avg_ms as u64,
                        info.max_ms as u64,
                        info.count as u64,
                    );
                    // 解析出的 IP（连接时记录；空 = 无此项，不显示）
                    if !info.ip.is_empty() {
                        text.push_str(&format!("\nIP {}", info.ip));
                    }
                    set_section(CheckKind::Ping, text);
                }
                Err(e) => set_err(CheckKind::Ping, e),
            },
            core::remote::RemoteMsg::SslDone(r) => match r {
                Ok(info) => set_section(CheckKind::Ssl, remote::format_ssl_info(&info)),
                Err(e) => set_err(CheckKind::Ssl, e),
            },
            core::remote::RemoteMsg::WebDone(r) => match r {
                Ok(info) => {
                    set_section(CheckKind::Web, remote::format_web_info(&info));
                    // 详情回填弹窗信号；按钮发起的查看请求自动弹窗，
                    // 连接自动跑的只回填（用户点按钮随时可再看）
                    self.web_detail.set(remote::format_web_detail(&info));
                    if self.web_requested.get() {
                        self.show_web_detail.set(true);
                    }
                }
                Err(e) => set_err(CheckKind::Web, e),
            },
            core::remote::RemoteMsg::SeoDone(r) => match r {
                Ok(info) => set_section(CheckKind::Seo, remote::format_seo_info(&info)),
                Err(e) => set_err(CheckKind::Seo, e),
            },
            core::remote::RemoteMsg::SecDone(r) => match r {
                Ok(info) => set_section(CheckKind::Sec, remote::format_sec_info(&info)),
                Err(e) => set_err(CheckKind::Sec, e),
            },
        }
    }

    /// 把各节结果按 [`CHECK_ORDER`] 拼接进 `result`（单项更新即整串刷新，
    /// 未检测的项跳过——展示区只出现已完成的节）。
    fn refresh_result(&self) {
        let sections = self.sections.borrow();
        let mut parts: Vec<&str> = Vec::new();
        for kind in CHECK_ORDER {
            if let Some(text) = sections[kind as usize].as_deref() {
                parts.push(text);
            }
        }
        self.result.set(parts.join(SECTION_SEP));
    }

    /// 清空全部检测结果（重连时调用，避免上一站点的旧内容混入）
    fn clear_result(&self) {
        *self.sections.borrow_mut() = [const { None }; CHECK_ORDER.len()];
        self.refresh_result();
    }
}

impl Default for RemoteUi {
    fn default() -> Self {
        Self::new()
    }
}

/// 远程检测页：连接（弹窗输入网址）+ Ping + SSL 证书状态 + 网页状态 + 网址二维码
pub fn build_remote_tab(ui: &RemoteUi) -> (Element, Element) {
    let RemoteUi {
        show_conn,
        url_input,
        connected,
        endpoint_display,
        status,
        error,
        result,
        show_web_detail,
        web_detail,
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

    // ── 响应体：连接前不可用（仅 http/https）。点按钮发起网页状态检测，
    // 完成后自动弹详情窗（结果区只留状态摘要）──
    let web_btn = Element::button(lang::REMOTE_BODY())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::GLOBE, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let tx = tx.clone();
            let endpoint = endpoint.clone();
            let ui = ui.clone();
            move |_| {
                let ep = endpoint.borrow().clone();
                if let Some(ep) = ep {
                    ui.web_requested.set(true);
                    status.set(lang::REMOTE_BUSY().to_string());
                    remote::spawn_web_status(sink(tx.clone()), ep);
                }
            }
        });

    // ── SEO 分析：连接前不可用（仅 http/https，解析首页元信息）──
    let seo_btn = Element::button(lang::SEO_TITLE())
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
                    remote::spawn_seo(sink(tx.clone()), ep);
                }
            }
        });

    // ── 安全检测：连接前不可用（仅 http/https，扫描漏洞项）──
    let sec_btn = Element::button(lang::REMOTE_SEC())
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
                    remote::spawn_security_check(sink(tx.clone()), ep);
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
                let url = endpoint
                    .borrow()
                    .as_ref()
                    .map(|e| e.display.clone())
                    .unwrap_or_default();
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

    // ── 导出报告：把各节检测结果拼成 Markdown 保存（无结果时置错误提示）──
    let export_btn = Element::button(lang::REMOTE_EXPORT_REPORT())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::DOWNLOAD, Some(16)))
        .on_click({
            let ui = ui.clone();
            move |ctx| {
                // 组装 Markdown：标题 + 端点 + 各节（文本块内嵌，保留原始分节格式）
                let sections = ui.sections.borrow();
                let mut md = String::from("# 远程检测报告\n\n");
                if let Some(ep) = ui.endpoint.borrow().as_ref() {
                    md.push_str(&format!(
                        "- 目标: `{}`\n- 时间: {}\n\n",
                        ep.display,
                        local_stamp()
                    ));
                }
                let mut any = false;
                for kind in CHECK_ORDER {
                    if let Some(text) = sections[kind as usize].as_deref() {
                        any = true;
                        md.push_str(&format!("## {}\n\n```\n{}\n```\n\n", kind.label(), text));
                    }
                }
                if !any {
                    ui.error.set(lang::REMOTE_EXPORT_REPORT_EMPTY());
                    return;
                }
                ui.error.set(String::new());
                ctx.request_save_file(
                    PickDialog::new()
                        .title(lang::REMOTE_EXPORT_REPORT_TITLE())
                        .filter("Markdown", &["md"])
                        .file_name("remote-check-report.md"),
                    move |path: Option<PathBuf>| {
                        if let Some(dest) = path {
                            match std::fs::write(&dest, md.as_bytes()) {
                                Ok(()) => ui.status.set(lang::REMOTE_EXPORT_REPORT_DONE(
                                    dest.display().to_string(),
                                )),
                                Err(e) => ui.error.set(e.to_string()),
                            }
                        }
                    },
                );
            }
        });

    // ── 工具栏：连接 / Ping / SSL / 网页状态 ‖ 二维码（不依赖连接）+ 状态条 ──
    let toolbar = Element::row()
        .spacing(6)
        .cross(Align::Center)
        .child(connect_btn)
        .child(web_btn)
        .child(seo_btn)
        .child(sec_btn)
        .child(Element::leaf().width(1).height(20).bg_role(Role::Divider))
        .child(qr_btn)
        .child(export_btn)
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

    // ── 结果卡片：多行只读输出区（撑满卡片剩余高度，内容多时控件内部滚动）──
    // 滚动容器用 weight 弹性瓜分剩余高度（fill=Match 会按父高整占，
    // 叠加免责提示行与间距后把 card 底边顶穿）
    let result_card = card(
        &lang::REMOTE_CARD_RESULT(),
        Element::col()
            .spacing(6)
            .height_match()
            .child(
                // 免责提示：主动探测仅限自有/已授权服务器
                Element::label(lang::SEC_DISCLAIMER())
                    .font_size(11.0)
                    .fg_role(Role::TextMuted)
                    .max_lines(2),
            )
            // 检测结果包进滚动容器：内容超出卡片高度时内部滚动，不再把下方
            // 布局顶穿（SelectText 无约束时按内容自然撑高）
            .child(
                Element::scroll().width_match().weight(1.0).child(
                    select_text(result)
                        .font_family("Consolas")
                        .font_size(13.0)
                        .width_match(),
                ),
            ),
    )
    .height_match();

    // ── 连接弹窗：网址输入 + 已存网址管理（下拉选择/保存/删除）+ 确定/取消 ──
    // 已存网址下拉：选中即回填输入框
    let url_opts = ui.url_sel.map({
        let saved = ui.saved_urls.clone();
        move |idx: &usize| {
            let list = saved.get();
            if list.is_empty() {
                vec![lang::REMOTE_URL_NONE().to_string()]
            } else {
                vec![list
                    .get((*idx).min(list.len().saturating_sub(1)))
                    .cloned()
                    .unwrap_or_default()]
            }
        }
    });
    let url_pick = {
        let saved = ui.saved_urls.clone();
        let url_sel = ui.url_sel.clone();
        let url_input = url_input.clone();
        move |_: &mut windui::core::EventCtx| {
            if let Some(u) = saved.get().get(url_sel.get()) {
                url_input.set(u.clone());
            }
        }
    };
    let conn_dialog = input_dialog(
        show_conn,
        lang::REMOTE_DIALOG_TITLE(),
        620,
        move |_| show_conn.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(Element::label(lang::REMOTE_URL_LABEL()).font_size(13.0))
            .child(
                Element::text_input(url_input, lang::REMOTE_URL_HINT())
                    .autofocus()
                    .width_match(),
            )
            // 已存网址：下拉选择回填 + 保存当前输入 + 删除选中项
            .child(
                Element::row()
                    .width_match()
                    .spacing(8)
                    .cross(Align::Center)
                    .child(
                        Element::dropdown_signal(url_opts, ui.url_sel)
                            .width_match()
                            .weight(1.0)
                            .enabled_when({
                                let urls = ui.saved_urls.clone();
                                move || !urls.get().is_empty()
                            })
                            .on_click(url_pick),
                    )
                    .child(
                        Element::button(lang::REMOTE_URL_SAVE())
                            .small()
                            .neutral()
                            .on_click({
                                let ui = ui.clone();
                                move |_| {
                                    let u = ui.url_input.get().trim().to_string();
                                    if u.is_empty() {
                                        return;
                                    }
                                    if crate::core::store::remote_url_add(&u).is_ok() {
                                        ui.saved_urls.set(
                                            crate::core::store::remote_url_list()
                                                .unwrap_or_default(),
                                        );
                                    }
                                }
                            }),
                    )
                    .child(
                        Element::button(lang::REMOTE_URL_DEL())
                            .small()
                            .neutral()
                            .danger()
                            .visible_when({
                                let urls = ui.saved_urls.clone();
                                move || !urls.get().is_empty()
                            })
                            .on_click({
                                let ui = ui.clone();
                                move |_| {
                                    let list = ui.saved_urls.get();
                                    if let Some(u) =
                                        list.get(ui.url_sel.get().min(list.len().saturating_sub(1)))
                                    {
                                        let u = u.clone();
                                        if crate::core::store::remote_url_del(&u).is_ok() {
                                            ui.saved_urls.set(
                                                crate::core::store::remote_url_list()
                                                    .unwrap_or_default(),
                                            );
                                            ui.url_sel.set(0);
                                        }
                                    }
                                }
                            }),
                    ),
            )
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
            .child(Element::button(lang::SFTP_OK()).small().on_click({
                let tx = tx.clone();
                let endpoint = endpoint.clone();
                let ui = ui.clone();
                move |_| {
                    // 记忆上次连接网址（尽力而为，下次启动自动回填）
                    let url_s = url_input.get();
                    crate::core::settings::commit(&[("remote.url", Some(url_s.trim()))]);
                    match remote::parse_endpoint(&url_input.get()) {
                        Ok(ep) => {
                            let display = ep.display.clone();
                            *endpoint.borrow_mut() = Some(ep.clone());
                            connected.set(true);
                            endpoint_display.set(display);
                            status.set(String::new());
                            error.set(String::new());
                            ui.clear_result();
                            // 解析成功即发起连接探测（TCP + 可选 TLS）
                            remote::spawn_connect(sink(tx.clone()), ep);
                            show_conn.set(false);
                        }
                        // 解析失败：不关闭弹窗，错误在弹窗内显示
                        Err(e) => error.set(e),
                    }
                }
            })),
    );

    // ── 响应体详情弹窗：结果区只留状态摘要，点「响应体」按钮检测完成后自动弹出；
    // 内容为「状态摘要 + 分隔线 + 截断的响应体预览」，只读可选可复制 ──
    let web_detail_dialog = input_dialog(
        show_web_detail.clone(),
        lang::REMOTE_BODY(),
        620,
        move |_| show_web_detail.set(false),
        Element::col().width_match().spacing(10).child(
            Element::scroll().width_match().max_height(360).child(
                select_text(web_detail)
                    .font_family("Consolas")
                    .font_size(12.0)
                    .width_match(),
            ),
        ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_OK())
                    .small()
                    .on_click(move |_| show_web_detail.set(false)),
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
        620,
        move |_| show_qr.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(
                Element::col()
                    .width_match()
                    .cross(Align::Center)
                    .child(qr_view),
            )
            .child(
                Element::label(lang::REMOTE_URL_LABEL())
                    .font_size(12.0)
                    .fg_role(Role::TextMuted),
            )
            // 网址用可换行输入框承载（label 不支持 wrap，长 URL 会被裁掉），支持手动编辑
            .child(
                Element::text_input(qr_url, lang::REMOTE_URL_HINT())
                    .autofocus()
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
                Element::button(lang::TOTP_SAVE_PNG())
                    .small()
                    .neutral()
                    .on_click({
                        move |_| {
                            let Some(entry) = qr.get().first().cloned() else {
                                return;
                            };
                            match crate::widgets::save_qr_png(
                                "qrcode.png",
                                entry.w,
                                entry.h,
                                &entry.rgba,
                            ) {
                                Ok(Some(path)) => qr_error.set(lang::TOTP_PNG_SAVED(&path)),
                                Ok(None) => {}
                                Err(e) => qr_error.set(lang::TOTP_PNG_SAVE_FAIL(e)),
                            }
                        }
                    }),
            )
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

    // ── 页面：工具栏 + 当前端点 + 结果卡片（连接/二维码弹窗在根层级，遮罩满屏）──
    let page = Element::stack().fill().child(
        Element::col()
            .padding(12)
            .spacing(8)
            .child(toolbar)
            .child(endpoint_row)
            .child(result_card.weight(1.0)),
    );

    // 弹窗集中为根层级浮层：ModalScrim 铺满根节点，遮罩覆盖整窗（含侧栏）；
    // 弹窗隐藏时不渲染、不拦截命中，无需穿透处理
    let dialogs = Element::stack()
        .fill()
        .child(conn_dialog)
        .child(web_detail_dialog)
        .child(qr_dialog);

    (page, dialogs)
}

/// 本地时间戳 `YYYY-MM-DD HH:MM:SS`（无时间 crate：civil-from-days 手算，用于报告头部）
fn local_stamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64 + 8 * 3600; // 东八区偏移
    let days = secs.div_euclid(86_400);
    let (h, m, s) = (
        secs.rem_euclid(86_400) / 3600,
        secs.rem_euclid(3600) / 60,
        secs.rem_euclid(60),
    );
    // civil-from-days（Howard Hinnant 算法）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, m, s)
}
