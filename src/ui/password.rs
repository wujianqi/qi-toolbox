//! 密码页 UI：平台预设 + 算法选择 + 随机生成 + 加密输出
//!
//! 页面状态封装在 [`PasswordUi`]（run() 中创建，主题重建不丢状态）；
//! 业务逻辑在 [`crate::core::password`]。

use windui::prelude::*;

use super::{card, icons, select_text};
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
    /// 「用于：」用途备注输入（保存密码时一并入库）
    pub used_for: Signal<String>,
    /// 已保存的密码列表（store.db saved_passwords 表，密码已解密）
    pub saved: Signal<Vec<crate::core::store::SavedPassword>>,
    /// 就地提示（保存成功/失败等）
    pub msg: Signal<String>,
}

impl PasswordUi {
    pub fn new() -> Self {
        Self {
            input: signal(String::new()),
            output: signal(String::new()),
            sql_out: signal(String::new()),
            platform: signal(0usize),
            algo: signal(0usize),
            used_for: signal(String::new()),
            saved: signal(crate::core::store::saved_pwd_list().unwrap_or_default()),
            msg: signal(String::new()),
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
        used_for,
        saved,
        msg,
    } = ui.clone();

    // 记忆本次密码页输入与选择（尽力而为：输入明文属敏感项，加密落盘）
    let remember_pwd = |input: Signal<String>, platform: Signal<usize>, algo: Signal<usize>| {
        let input_s = input.get();
        let platform_s = platform.get().to_string();
        let algo_s = algo.get().to_string();
        crate::core::settings::commit(&[
            ("pwd.input", Some(input_s.as_str())),
            ("pwd.platform", Some(&platform_s)),
            ("pwd.algo", Some(&algo_s)),
        ]);
    };

    let preset_labels: Vec<String> = password::PlatformPreset::all()
        .iter()
        .map(|p| p.label())
        .collect();
    let algo_labels: Vec<String> = password::HashAlgorithm::all()
        .iter()
        .map(|a| a.label().to_string())
        .collect();

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
                // 记忆生成的明文与当前选择（明文属敏感项，加密落盘）
                remember_pwd(input, platform, algo);
            }
        })
    };

    let encrypt = Element::button(lang::PWD_ENCRYPT())
        .neutral()
        .icon_content(icons::stateful_icon(icons::LOCK, Some(16)))
        .on_click(move |_| {
            // 记忆本次输入与选择（明文属敏感项，加密落盘）
            remember_pwd(input, platform, algo);
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
    // 平台变化时算法自动跟随该平台默认算法（在下拉 on_click 回调里同步——
    // 构建期禁止写信号，windui 会 panic；此前用 host_signal 在重建时 set，
    // 切主题整树重建即崩溃）；非「自定义」时算法下拉禁用（算法由平台决定）。
    let algo_row = Element::row()
        .spacing(8)
        .cross(Align::Center)
        .child(Element::label(lang::PWD_ALGO()).font_size(13.0))
        .child(
            Element::dropdown(algo_labels.clone(), algo)
                .width(220)
                .enabled_when(move || platform.get() == 0),
        );

    // ── 保存密码：「用于：」备注 + 保存按钮；成功后刷新列表并就地提示 ──
    let save_btn = Element::button(lang::PWD_SAVE())
        .neutral()
        .icon_content(icons::stateful_icon(icons::SAVE, Some(16)))
        .on_click(move |_| {
            let text = input.get().trim().to_string();
            if text.is_empty() {
                return;
            }
            match crate::core::store::saved_pwd_add(used_for.get().trim(), &text) {
                Ok(()) => {
                    saved.set(crate::core::store::saved_pwd_list().unwrap_or_default());
                    let note = used_for.get().trim().to_string();
                    msg.set(lang::PWD_SAVED_DONE(if note.is_empty() {
                        text
                    } else {
                        note
                    }));
                    used_for.set(String::new());
                }
                Err(e) => msg.set(lang::PWD_SAVED_FAIL(e)),
            }
        });

    // ── 已保存密码列表：用途 + 密码 + 复制/删除（store.db saved_passwords 表）──
    let saved_rows = Element::list_signal(
        saved,
        |e: &crate::core::store::SavedPassword| e.id,
        move |e: crate::core::store::SavedPassword| {
            let msg = msg.clone();
            let saved = saved.clone();
            let id = e.id;
            let copy_pwd = e.password.clone();
            Element::row()
                .width_match()
                .spacing(8)
                .cross(Align::Center)
                .child(
                    Element::label(if e.used_for.is_empty() {
                        "—".to_string()
                    } else {
                        e.used_for.clone()
                    })
                    .font_size(12.0)
                    .width(120)
                    .max_lines(1),
                )
                .child(
                    // 密码整串展示；复制由右侧按钮承担（不依赖拖选）
                    Element::label(e.password)
                        .font_family("Consolas")
                        .font_size(13.0)
                        .weight(1.0)
                        .max_lines(1),
                )
                .child(
                    Element::button(lang::PWD_COPY())
                        .small()
                        .neutral()
                        .on_click(move |ctx| {
                            ctx.clipboard_set(&copy_pwd);
                            msg.set(lang::PWD_COPIED());
                        }),
                )
                .child(
                    Element::button(lang::PWD_DELETE())
                        .small()
                        .neutral()
                        .danger()
                        .on_click(move |_| {
                            if crate::core::store::saved_pwd_del(id).is_ok() {
                                saved.set(crate::core::store::saved_pwd_list().unwrap_or_default());
                                msg.set(String::new());
                            }
                        }),
                )
        },
    );

    Element::col()
        .padding(16)
        .spacing(12)
        .child(card(
            &lang::PWD_CARD_SETUP(),
            Element::col()
                .spacing(10)
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::PWD_PLATFORM()).font_size(13.0))
                        .child(
                            Element::dropdown(preset_labels, platform)
                                .width(220)
                                .on_click(move |_| {
                                    // 平台为预设时，算法自动跟随该平台默认算法
                                    let preset = platform.get();
                                    if preset != 0 {
                                        let def = password::PlatformPreset::all()[preset]
                                            .default_algorithm();
                                        if let Some(i) = password::HashAlgorithm::all()
                                            .iter()
                                            .position(|&a| a == def)
                                        {
                                            algo.set(i);
                                        }
                                    }
                                }),
                        ),
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
            &lang::PWD_CARD_SAVE(),
            Element::col()
                .spacing(10)
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::PWD_USED_FOR()).font_size(13.0))
                        .child(
                            Element::text_input(used_for, lang::PWD_USED_FOR_HINT())
                                .width_match()
                                .weight(1.0),
                        )
                        .child(save_btn),
                )
                .child(Element::label(lang::PWD_SAVED_LABEL()).font_size(14.0))
                .child(saved_rows)
                // 就地提示：保存成功/失败、复制成功
                .child(
                    Element::label_signal(msg)
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                ),
        ))
        .child(card(
            &lang::PWD_CARD_OUTPUT(),
            Element::row()
                .width_match()
                .spacing(16)
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(6)
                        .child(Element::label(lang::PWD_OUTPUT_LABEL()).font_size(14.0))
                        // 只读可选文本：输出可拖选/Ctrl+C 复制（不再借输入框承载）
                        .child(
                            select_text(output)
                                .font_family("Consolas")
                                .font_size(13.0)
                                .width_match()
                                .height(120),
                        ),
                )
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(6)
                        .child(Element::label(lang::PWD_SQL_LABEL()).font_size(14.0))
                        // 只读可选文本：SQL 可拖选/Ctrl+C 复制（不再借输入框承载）
                        .child(
                            select_text(sql_out)
                                .font_family("Consolas")
                                .font_size(13.0)
                                .width_match()
                                .height(120),
                        ),
                ),
        ))
}
