//! TOTP 页 UI：密钥生成/保存 + 账号/发行方 + 算法 + 验证码 + 二维码
//!
//! 页面状态封装在 [`TotpUi`]（run() 中创建，主题重建不丢状态）；
//! 业务逻辑在 [`crate::core::totp`]。

use windui::prelude::*;

use super::{card, icons};
use crate::core::totp;
use crate::lang;

/// TOTP 页状态信号集合
#[derive(Clone)]
pub struct TotpUi {
    pub key: Signal<String>,
    pub account: Signal<String>,
    pub issuer: Signal<String>,
    pub algo_sel: Signal<usize>,
    pub output: Signal<String>,
    pub qr: Signal<Vec<totp::QrEntry>>,
}

impl TotpUi {
    pub fn new() -> Self {
        Self {
            key: signal(totp::load_saved_key()),
            account: signal(String::from("user")),
            issuer: signal(String::from("Qi")),
            algo_sel: signal(0usize),
            output: signal(String::new()),
            qr: signal(Vec::new()),
        }
    }
}

impl Default for TotpUi {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_totp_tab(ui: &TotpUi) -> Element {
    let TotpUi {
        key,
        account,
        issuer,
        algo_sel,
        output,
        qr,
    } = ui.clone();

    // 生成密钥
    let gen_key = Element::button(lang::TOTP_GEN_KEY())
        .neutral()
        .icon_content(icons::stateful_icon(icons::KEY, Some(16)))
        .on_click(move |_| {
        key.set(totp::generate_secret_key());
        output.set(String::new());
        qr.set(Vec::new());
    });

    // 生成验证码
    let generate = Element::button(lang::TOTP_GENERATE())
        .neutral()
        .icon_content(icons::stateful_icon(icons::ZAP, Some(16)))
        .on_click(move |_| {
        let k = key.get();
        if !k.trim().is_empty() {
            output.set(totp::run(k.trim(), totp::algo_from_index(algo_sel.get())));
        }
    });

    // 保存密钥到本地文件
    let save = Element::button(lang::TOTP_SAVE())
        .neutral()
        .icon_content(icons::stateful_icon(icons::SAVE, Some(16)))
        .on_click(move |_| {
        let k = key.get().trim().to_string();
        if k.is_empty() {
            return;
        }
        match std::fs::write("qi_key.txt", &k) {
            Ok(_) => output.set(lang::TOTP_SAVED().replace("{}", &k)),
            Err(e) => output.set(format!(
                "{}: {}",
                lang::TOTP_SAVE_FAIL().trim_end_matches(": {}"),
                e
            )),
        }
    });

    // 生成二维码（RGBA → list_signal 渲染）
    let gen_qr = Element::button(lang::TOTP_QR())
        .neutral()
        .icon_content(icons::stateful_icon(icons::QR, Some(16)))
        .on_click(move |_| {
        let k = key.get();
        if k.trim().is_empty() {
            return;
        }
        let account_c = account.get();
        let issuer_c = issuer.get();
        let alg = totp::algo_from_index(algo_sel.get());
        match totp::generate_qr_entry(k.trim(), &account_c, &issuer_c, alg) {
            Some(entry) => qr.set(vec![entry]),
            None => qr.set(Vec::new()),
        }
    });

    let qr_view = Element::list_signal(
        qr,
        |e: &totp::QrEntry| e.id,
        |e: totp::QrEntry| {
            Element::col()
                .cross(Align::Center)
                .spacing(6)
                .child(
                    Element::image_rgba(e.w, e.h, &e.rgba)
                        .fit(Fit::Contain)
                        .corner(6.0)
                        .bg_role(Role::Surface),
                )
                .child(
                    Element::label(lang::TOTP_SCAN_HINT())
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                )
        },
    );

    Element::col()
        .padding(20)
        .spacing(14)
        .child(Element::label(lang::TOTP_TITLE()).font_size(18.0).font_weight(700))
        .child(card(
            lang::TOTP_CARD_SETUP(),
            Element::col()
                .spacing(10)
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(gen_key)
                        .child(
                            Element::label(lang::TOTP_GEN_HINT())
                                .font_size(12.0)
                                .fg_role(Role::TextMuted),
                        ),
                )
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::TOTP_KEY_LABEL()).font_size(14.0))
                        .child(Element::dropdown(vec!["SHA1", "SHA256", "SHA512"], algo_sel).width(140))
                        .child(
                            Element::label(lang::TOTP_KEY_WARN())
                                .font_size(12.0)
                                .fg_role(Role::Danger),
                        ),
                )
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::TOTP_ACCOUNT()).font_size(13.0))
                        .child(Element::text_input(account, lang::TOTP_ACCOUNT_HINT()).width(160))
                        .child(Element::label(lang::TOTP_ISSUER()).font_size(13.0))
                        .child(Element::text_input(issuer, lang::TOTP_ISSUER_HINT()).width(160)),
                )
                .child(
                    Element::row()
                        .spacing(8)
                        .child(
                            Element::text_input(key, lang::TOTP_KEY_INPUT_HINT())
                                .width_match()
                                .weight(1.0),
                        )
                        .child(generate)
                        .child(save)
                        .child(gen_qr),
                ),
        ))
        .child(card(
            lang::TOTP_CARD_OUTPUT(),
            Element::row()
                .width_match()
                .spacing(16)
                .child(
                    Element::col()
                        .weight(1.0)
                        .child(
                            Element::text_input(output, "")
                                .multiline()
                                .font_size(26.0)
                                .width_match()
                                .height(120),
                        ),
                )
                .child(
                    Element::col()
                        .weight(1.0)
                        .cross(Align::Center)
                        .child(qr_view),
                ),
        ))
}
