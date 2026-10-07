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
    /// 校验哈希：待验证的哈希串输入
    pub verify_hash: Signal<String>,
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
            verify_hash: signal(String::new()),
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
        ..
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
        .icon_content(icons::stateful_icon(icons::DICE))
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
        .icon_content(icons::stateful_icon(icons::LOCK))
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

    // ── 校验哈希：输入哈希串，用当前输入框密码验证匹配 + 识别算法 ──
    let (v_input, v_hash) = (input, ui.verify_hash);
    let verify_btn = Element::button(lang::PWD_VERIFY())
        .neutral()
        .icon_content(icons::stateful_icon(icons::SHIELD))
        .on_click(move |_| {
            let pwd = v_input.get();
            let hash = v_hash.get().trim().to_string();
            if hash.is_empty() {
                return;
            }
            // 算法识别结果一并展示（无法识别时 verify 也会报错）
            let algo = password::detect_hash_algorithm(&hash)
                .map(|s| s.to_string())
                .unwrap_or_else(lang::PWD_HASH_UNKNOWN_ALGO);
            match password::verify_password(&pwd, &hash) {
                Ok(true) => super::toast::ok(lang::PWD_VERIFY_OK(algo.clone())),
                Ok(false) => super::toast::err(lang::PWD_VERIFY_FAIL(algo)),
                Err(e) => super::toast::err(e),
            }
        });

    // ── 算法下拉：与目标平台联动 ──
    // 平台变化时算法自动跟随该平台默认算法（在下拉 on_click 回调里同步——
    // 构建期禁止写信号，windui 会 panic；此前用 host_signal 在重建时 set，
    // 切主题整树重建即崩溃）；非「自定义」时算法下拉禁用（算法由平台决定）。
    let algo_dropdown = Element::dropdown(algo_labels.clone(), algo)
        .width(220)
        .enabled_when(move || platform.get() == 0);

    // ── 保存密码：「用于：」备注 + 保存按钮；成功后刷新列表并就地提示 ──
    let save_btn = Element::button(lang::PWD_SAVE())
        .neutral()
        .icon_content(icons::stateful_icon(icons::SAVE))
        .on_click(move |_| {
            let hash = output.get().trim().to_string();
            // 联动：保存的是「输出结果」里加密后的密码，尚无输出时提示先加密
            if hash.is_empty() {
                super::toast::err(lang::PWD_SAVE_NEED_ENCRYPT());
                return;
            }
            match crate::core::store::saved_pwd_add(used_for.get().trim(), &hash) {
                Ok(()) => {
                    saved.set(crate::core::store::saved_pwd_list().unwrap_or_default());
                    let note = used_for.get().trim().to_string();
                    super::toast::ok(lang::PWD_SAVED_DONE(if note.is_empty() {
                        hash
                    } else {
                        note
                    }));
                    used_for.set(String::new());
                }
                Err(e) => super::toast::err(lang::PWD_SAVED_FAIL(e)),
            }
        });

    // ── 已保存密码列表：用途 + 密码 + 复制/删除（store.db saved_passwords 表）──
    let saved_rows = Element::list_signal(
        saved,
        |e: &crate::core::store::SavedPassword| e.id,
        move |e: crate::core::store::SavedPassword| {
            let saved = saved;
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
                            super::toast::ok(lang::PWD_COPIED());
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
                    // 平台与算法同一行：平台为预设时算法下拉禁用并自动跟随默认算法
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
                        )
                        .child(Element::label(lang::PWD_ALGO()).font_size(13.0))
                        .child(algo_dropdown),
                )
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
                )
                // 校验哈希行：粘贴哈希串 → 用上方密码验证匹配 + 显示算法识别结果
                .child(
                    Element::row()
                        .spacing(8)
                        .child(
                            Element::text_input(ui.verify_hash, lang::PWD_VERIFY_HASH_HINT())
                                .width_match()
                                .weight(1.0),
                        )
                        .child(verify_btn),
                ),
        ))
        // 输出与保存联动：保存密码保存的是上方「加密的密码」输出结果
        .child(card(
            &lang::PWD_CARD_OUTPUT(),
            Element::col()
                .spacing(10)
                .child(
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
                )
                // 保存行：保存对象即上方加密输出；「用于：」为备注
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::PWD_USED_FOR()).font_size(13.0))
                        .child(Element::text_input(used_for, lang::PWD_USED_FOR_HINT()).width(220))
                        .child(save_btn),
                )
                .child(Element::label(lang::PWD_SAVED_LABEL()).font_size(14.0))
                .child(saved_rows),
        ))
}
