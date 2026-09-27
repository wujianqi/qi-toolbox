//! TOTP 页 UI：密钥生成/保存 + 账号/发行方 + 算法 + 验证码 + 二维码
//!
//! 页面状态封装在 [`TotpUi`]（run() 中创建，主题重建不丢状态）；
//! 业务逻辑在 [`crate::core::totp`]。

use windui::prelude::*;

use windui::core::EventCtx;

use super::{card, icons, select_text};
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
    /// 已保存的密钥（store.db 多密钥管理）
    pub keys: Signal<Vec<crate::core::store::TotpKey>>,
    /// 密钥下拉当前选中索引（0 = 未选择）
    pub key_sel: Signal<usize>,
    /// 生成密钥位数下拉索引：0 = 32 位（160bit），1 = 16 位（80bit）
    pub key_bits: Signal<usize>,
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
            keys: signal(crate::core::store::totp_list().unwrap_or_default()),
            key_sel: signal(0usize),
            key_bits: signal(0usize),
        }
    }
}

impl Default for TotpUi {
    fn default() -> Self {
        Self::new()
    }
}

/// TOTP 页 UI 构建（返回 (页面, 备份弹窗)：弹窗须挂根层级，遮罩才铺满全窗）
pub fn build_totp_tab(ui: &TotpUi) -> (Element, Element) {
    let TotpUi {
        key,
        account,
        issuer,
        algo_sel,
        output,
        qr,
        keys: _,
        key_sel: _,
        ..
    } = ui.clone();

    // 记忆当前 TOTP 配置（尽力而为：账号/发行方/算法明文，密钥属敏感项加密落盘）
    let remember_totp = |key: Signal<String>,
                         account: Signal<String>,
                         issuer: Signal<String>,
                         algo_sel: Signal<usize>| {
        let key_s = key.get();
        let account_s = account.get();
        let issuer_s = issuer.get();
        let algo_s = algo_sel.get().to_string();
        crate::core::settings::commit(&[
            ("totp.key", Some(key_s.as_str())),
            ("totp.account", Some(account_s.as_str())),
            ("totp.issuer", Some(issuer_s.as_str())),
            ("totp.algo", Some(algo_s.as_str())),
        ]);
    };

    // 生成密钥（位数下拉：16 位 / 32 位）
    let gen_key = {
        let (key, key_bits) = (key.clone(), ui.key_bits);
        Element::button(lang::TOTP_GEN_KEY())
            .neutral()
            .icon_content(icons::stateful_icon(icons::KEY, Some(16)))
            .on_click(move |_| {
                // 下拉索引 0 = 32 位（160bit），1 = 16 位（80bit）
                let bits = if key_bits.get() == 1 { 16 } else { 32 };
                key.set(totp::generate_secret_key_bits(bits));
                output.set(String::new());
                qr.set(Vec::new());
            })
    };

    // 生成验证码
    let generate = Element::button(lang::TOTP_GENERATE())
        .neutral()
        .icon_content(icons::stateful_icon(icons::ZAP, Some(16)))
        .on_click(move |_| {
            // 记忆当前配置（账号/发行方/算法/密钥；密钥属敏感项，加密落盘）
            remember_totp(key, account, issuer, algo_sel);
            let k = key.get();
            if !k.trim().is_empty() {
                output.set(totp::run(k.trim(), totp::algo_from_index(algo_sel.get())));
            }
        });

    // 保存密钥到本地 store 数据库（多密钥管理：名称 = 账号@发行方）
    let keys = ui.keys;
    let key_sel = ui.key_sel;
    let save = Element::button(lang::TOTP_SAVE())
        .neutral()
        .icon_content(icons::stateful_icon(icons::SAVE, Some(16)))
        .on_click(move |_| {
            let k = key.get().trim().to_string();
            if k.is_empty() {
                return;
            }
            let name = if issuer.get().trim().is_empty() {
                account.get().trim().to_string()
            } else {
                format!("{}@{}", account.get().trim(), issuer.get().trim())
            };
            let entry = crate::core::store::TotpKey {
                id: 0,
                name,
                secret: k,
                algo: algo_sel.get() as i64,
            };
            if crate::core::store::totp_upsert(&entry).is_ok() {
                keys.set(crate::core::store::totp_list().unwrap_or_default());
                key_sel.set(0);
                output.set(lang::TOTP_SAVED(&entry.secret));
            } else {
                output.set(lang::TOTP_SAVE_FAIL(String::from("store write failed")));
            }
        });

    // 已存密钥下拉 + 删除：选中即回填（含算法），可再生成验证码/二维码
    let key_opts = key_sel.map(move |idx| {
        let list = keys.get();
        if list.is_empty() {
            vec![lang::TOTP_KEYS_NONE()]
        } else {
            vec![list.get(*idx).map(|k| k.name.clone()).unwrap_or_default()]
        }
    });
    let (key_pick_keys, key_pick_key, key_pick_algo) = (ui.keys, key, algo_sel);
    let key_pick = move |_: &mut EventCtx| {
        if let Some(k) = key_pick_keys.get().get(key_sel.get()) {
            key_pick_key.set(k.secret.clone());
            key_pick_algo.set(k.algo.clamp(0, 2) as usize);
        }
    };
    let (del_keys, del_sel, del_key) = (ui.keys, ui.key_sel, key);
    let key_del_btn = Element::button(lang::TOTP_KEY_DEL())
        .small()
        .neutral()
        .danger()
        .on_click(move |_| {
            if let Some(k) = del_keys.get().get(del_sel.get()) {
                if crate::core::store::totp_del(k.id).is_ok() {
                    del_keys.set(crate::core::store::totp_list().unwrap_or_default());
                    del_sel.set(0);
                    del_key.set(String::new());
                    output.set(String::new());
                    qr.set(Vec::new());
                }
            }
        });

    // 生成二维码（RGBA → list_signal 渲染）
    let gen_qr = Element::button(lang::TOTP_QR())
        .neutral()
        .icon_content(icons::stateful_icon(icons::QR, Some(16)))
        .on_click(move |_| {
            remember_totp(key, account, issuer, algo_sel);
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

    // 保存二维码为 PNG：用户选路径，RGBA 原样编码写入（rfd 选路径，png crate 编码）。
    // 仅在二维码已生成（qr 非空）时显示——无图可存时不占位。
    let has_qr = qr.map(|v| !v.is_empty());
    let save_png = Element::button(lang::TOTP_SAVE_PNG())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::DOWNLOAD, Some(14)))
        .visible_when(move || has_qr.get())
        .on_click({
            move |_| {
                let Some(entry) = qr.get().first().cloned() else {
                    return;
                };
                match crate::widgets::save_qr_png("qrcode.png", entry.w, entry.h, &entry.rgba) {
                    Ok(Some(path)) => {
                        output.set(lang::TOTP_PNG_SAVED(&path));
                    }
                    Ok(None) => {}
                    Err(e) => output.set(lang::TOTP_PNG_SAVE_FAIL(e)),
                }
            }
        });

    // ── 已存密钥行底部留白：备份功能已移除（密钥入库 store.db，
    //    S3 页可整库备份 store.db，无需单独 .qtbak 导出）──

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

    let page = Element::col()
        .padding(16)
        .spacing(12)
        .child(card(
            &lang::TOTP_CARD_SETUP(),
            Element::col()
                .spacing(10)
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(gen_key)
                        .child(
                            // 生成位数：32 位（160bit，兼容性最佳）/ 16 位（80bit，输入方便）
                            Element::dropdown(
                                vec![lang::TOTP_BITS_32(), lang::TOTP_BITS_16()],
                                ui.key_bits,
                            )
                            .width(120),
                        )
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
                        .child(
                            Element::dropdown(vec!["SHA1", "SHA256", "SHA512"], algo_sel)
                                .width(140),
                        )
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
                )
                .child(
                    // 已存密钥管理：下拉选择回填 + 删除（store.db 多密钥存储）
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::TOTP_KEYS_LABEL()).font_size(13.0))
                        .child(
                            Element::dropdown_signal(key_opts, key_sel)
                                .width(180)
                                .on_click(key_pick),
                        )
                        .child(key_del_btn)
                        .child(Element::flex_spacer()),
                ),
        ))
        .child(card(
            &lang::TOTP_CARD_OUTPUT(),
            Element::row()
                .width_match()
                .spacing(16)
                .child(
                    Element::col().weight(1.0).child(
                        // 只读可选文本：验证码可拖选/Ctrl+C 复制（不再借输入框承载）
                        select_text(output)
                            .font_size(26.0)
                            .width_match()
                            .height(120),
                    ),
                )
                .child(
                    Element::col()
                        .weight(1.0)
                        .cross(Align::Center)
                        .spacing(8)
                        .child(qr_view)
                        .child(save_png),
                ),
        ));

    // 无弹窗需要挂根层级（备份弹窗已移除）
    (page, Element::col())
}
