//! 密码页 UI：平台预设 + 算法选择 + 随机生成 + 加密输出
//!
//! 页面状态封装在 [`PasswordUi`]（run() 中创建，主题重建不丢状态）；
//! 业务逻辑在 [`crate::core::password`]。

use windui::prelude::*;

use super::{card, icons};
use crate::core::password;
use crate::lang;

/// 密码页状态信号集合
#[derive(Clone)]
pub struct PasswordUi {
    pub input: Signal<String>,
    pub output: Signal<String>,
    pub sql_out: Signal<String>,
    pub platform: Signal<usize>,
    pub algo: Signal<usize>,
}

impl PasswordUi {
    pub fn new() -> Self {
        Self {
            input: signal(String::new()),
            output: signal(String::new()),
            sql_out: signal(String::new()),
            platform: signal(0usize),
            algo: signal(0usize),
        }
    }
}

impl Default for PasswordUi {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_password_tab(ui: &PasswordUi) -> Element {
    let PasswordUi {
        input,
        output,
        sql_out,
        platform,
        algo,
    } = ui.clone();

    let preset_labels: Vec<&str> =
        password::PlatformPreset::all().iter().map(|p| p.label()).collect();
    let algo_labels: Vec<&str> =
        password::HashAlgorithm::all().iter().map(|a| a.label()).collect();

    let gen_btn = |len: usize| {
        Element::button(match len {
            8 => lang::PWD_GEN_8(),
            12 => lang::PWD_GEN_12(),
            _ => lang::PWD_GEN_16(),
        })
        .neutral()
        .icon_content(icons::stateful_icon(icons::DICE, Some(16)))
        .on_click(move |_| {
            if let Ok(pw) = password::generate_random_password(len) {
                input.set(pw);
                output.set(String::new());
            }
        })
    };

    let encrypt = Element::button(lang::PWD_ENCRYPT())
        .neutral()
        .icon_content(icons::stateful_icon(icons::LOCK, Some(16)))
        .on_click(move |_| {
        let text = input.get().trim().to_string();
        if text.is_empty() {
            return;
        }
        // 平台预设非 None 时自动采用该平台默认算法，并同步算法下拉
        let preset = platform.get();
        let hash_algo = if preset == 0 {
            password::HashAlgorithm::all()[algo.get()]
        } else {
            password::PlatformPreset::all()[preset].default_algorithm()
        };
        algo.set(
            password::HashAlgorithm::all()
                .iter()
                .position(|&a| a == hash_algo)
                .unwrap_or(1),
        );
        match password::hash_password(&text, hash_algo) {
            Ok(h) => {
                // 先拼 SQL 再移交 hash，避免为 set 多克隆一次
                let sql = format!(
                    "UPDATE admins SET password = '{}' WHERE username = 'your_username';",
                    h
                );
                output.set(h);
                sql_out.set(sql);
            }
            Err(e) => {
                output.set(e);
                sql_out.set(String::new());
            }
        }
    });

    // ── 算法下拉：与目标平台联动 ──
    // 平台变化时算法自动跟随该平台默认算法；非「自定义」时算法下拉禁用（算法由平台决定）。
    let algo_follow = platform.map(|p| vec![*p]);
    let algo_row = Element::host_signal(algo_follow, move |_| {
        // 平台为预设时，同步算法为该平台的默认算法
        let preset = platform.get();
        if preset != 0 {
            let def = password::PlatformPreset::all()[preset].default_algorithm();
            if let Some(i) = password::HashAlgorithm::all()
                .iter()
                .position(|&a| a == def)
            {
                algo.set(i);
            }
        }
        Element::row()
            .spacing(8)
            .cross(Align::Center)
            .child(Element::label(lang::PWD_ALGO()).font_size(13.0))
            .child(
                Element::dropdown(algo_labels.clone(), algo)
                    .width(220)
                    .enabled_when(move || platform.get() == 0),
            )
    });

    Element::col()
        .padding(20)
        .spacing(14)
        .child(Element::label(lang::PWD_TITLE()).font_size(18.0).font_weight(700))
        .child(card(
            lang::PWD_CARD_SETUP(),
            Element::col()
                .spacing(10)
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::PWD_PLATFORM()).font_size(13.0))
                        .child(Element::dropdown(preset_labels, platform).width(220)),
                )
                .child(algo_row)
                .child(
                    Element::row()
                        .spacing(8)
                        .child(gen_btn(8))
                        .child(gen_btn(12))
                        .child(gen_btn(16)),
                )
                .child(Element::label(lang::PWD_INPUT_LABEL()).font_size(14.0))
                .child(
                    Element::row()
                        .spacing(8)
                        .child(
                            Element::text_input(input, lang::PWD_INPUT_HINT())
                                .width_match()
                                .weight(1.0),
                        )
                        .child(encrypt),
                ),
        ))
        .child(card(
            lang::PWD_CARD_OUTPUT(),
            Element::row()
                .width_match()
                .spacing(16)
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(6)
                        .child(Element::label(lang::PWD_OUTPUT_LABEL()).font_size(14.0))
                        .child(
                            Element::text_input(output, "")
                                .multiline()
                                .width_match()
                                .height(120),
                        ),
                )
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(6)
                        .child(Element::label(lang::PWD_SQL_LABEL()).font_size(14.0))
                        .child(
                            Element::text_input(sql_out, "")
                                .multiline()
                                .width_match()
                                .height(120),
                        ),
                ),
        ))
}
