//! TOTP 页 UI：密钥生成/保存 + 账号/发行方 + 算法 + 实时验证码 + 倒计时 + 二维码
//!
//! 页面状态封装在 [`TotpUi`]（run() 中创建，主题重建不丢状态）；
//! 业务逻辑在 [`crate::core::totp`]。验证码实时刷新：`run()` 里挂 1s 定时器
//! 调 [`on_tick`]，验证码随时间自动滚动，倒计时条指示当前码剩余有效期。
//!
//! 已存密钥下拉的回填不走 `on_click`：windui 的 Dropdown 控件未实现
//! `Widget::take_click`，Builder 注入的点击回调会被静默丢弃（0.20 实测）。
//! 选中回填用 [`super::sig_watch`] 监听 `key_sel` 版本差分（选中即回填并出码）。

use windui::prelude::*;

use windui::core::EventCtx;

use super::{card, icons, select_text};
use crate::core::totp;
use crate::lang;

/// TOTP 周期（秒），与 core::totp 生成参数一致
const PERIOD: u64 = 30;

/// TOTP 页状态信号集合
#[derive(Clone)]
pub struct TotpUi {
    pub key: Signal<String>,
    pub account: Signal<String>,
    pub issuer: Signal<String>,
    pub algo_sel: Signal<usize>,
    pub output: Signal<String>,
    /// 当前验证码剩余有效秒数随动的时钟（unix 秒；倒计时/进度条派生自它）
    pub tick: Signal<u64>,
    /// 密钥非法时的状态行文案（空 = 无错误；不把错误文本混进验证码输出）
    pub status: Signal<String>,
    pub qr: Signal<Vec<totp::QrEntry>>,
    /// 已保存的密钥（store.db 多密钥管理）
    pub keys: Signal<Vec<crate::core::store::TotpKey>>,
    /// 密钥下拉当前选中索引（0 = 「选择已存密钥…」占位；i>0 = keys[i-1]）
    pub key_sel: Signal<usize>,
    /// 已消费的 key_sel 版本基准：sig_watch 与动作入口的兜底同步共用，
    /// 谁先消费掉一次新选择，另一方即空转
    pub saved_seen_ver: Signal<u64>,
    /// 生成密钥位数下拉索引：0 = 32 位（160bit），1 = 16 位（80bit）
    pub key_bits: Signal<usize>,
    /// 生成验证码后自动复制剪贴板：0 = 关，1 = 开（记忆于 settings）
    pub auto_copy: Signal<usize>,
}

impl TotpUi {
    pub fn new() -> Self {
        Self {
            key: signal(totp::load_saved_key()),
            account: signal(String::from("user")),
            issuer: signal(String::from("Qi")),
            algo_sel: signal(0usize),
            output: signal(String::new()),
            tick: signal(0),
            status: signal(String::new()),
            qr: signal(Vec::new()),
            keys: signal(crate::core::store::totp_list().unwrap_or_default()),
            key_sel: signal(0usize),
            saved_seen_ver: signal(0),
            key_bits: signal(0usize),
            auto_copy: signal(
                crate::core::settings::load()
                    .get("totp.auto_copy")
                    .map(|s| s == "1")
                    .unwrap_or(true) as usize,
            ),
        }
    }
}

impl Default for TotpUi {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 按当前密钥/算法计算并显示（含倒计时基准推进）。密钥为空清空显示；
/// 非法时清空验证码、状态行显示原因——错误文本不混进验证码输出区。
/// 仅内容变化时写信号，避免每秒无谓重绘。返回是否得到合法验证码。
fn display_now(ui: &TotpUi) -> bool {
    let key = ui.key.get();
    let key = key.trim().to_string();
    if key.is_empty() {
        if !ui.output.get().is_empty() {
            ui.output.set(String::new());
        }
        if !ui.status.get().is_empty() {
            ui.status.set(String::new());
        }
        return false;
    }
    ui.tick.set(unix_now());
    match totp::generate_totp(&key, totp::algo_from_index(ui.algo_sel.get())) {
        Ok(code) => {
            if ui.output.get() != code {
                ui.output.set(code.clone());
            }
            if !ui.status.get().is_empty() {
                ui.status.set(String::new());
            }
            true
        }
        Err(e) => {
            if !ui.output.get().is_empty() {
                ui.output.set(String::new());
            }
            if ui.status.get() != e {
                ui.status.set(e);
            }
            false
        }
    }
}

/// 立即出码。`announce` 时（且自动复制开启）复制到剪贴板并轻提示——
/// 只复制合法验证码，错误文本绝无可能被当验证码复制走。
fn apply_now(ui: &TotpUi, ctx: &mut EventCtx, announce: bool) {
    if display_now(ui) && announce && ui.auto_copy.get() == 1 {
        let code = ui.output.get();
        ctx.clipboard_set(&code);
        super::toast::ok(lang::TOTP_COPIED());
    }
}

/// 每秒定时回调（App::on_interval 挂入）：推进倒计时并让验证码实时跟随当前时间。
pub(crate) fn on_tick(ui: &TotpUi) {
    display_now(ui);
}

/// 消费已存密钥下拉的新选择：key_sel 版本与已消费基准不同 → 回填密钥/算法并
/// 立即出码（0 = 占位行，不回填）。返回是否发生了回填。
///
/// 双通道触发、共用同一基准：key_watch（sig_watch 事件驱动）为主；生成/保存/
/// 二维码等动作入口再兜底同步一次——sig_watch 若因框架版本差异未投递，
/// 点「生成」也必然能拿到选中密钥（旧版「选了密钥点生成无效」的保底修复）。
fn sync_saved_selection(ui: &TotpUi, ctx: &mut EventCtx) -> bool {
    let ver = ui.key_sel.version();
    if ui.saved_seen_ver.get() == ver {
        return false;
    }
    ui.saved_seen_ver.set(ver);
    let idx = ui.key_sel.get().checked_sub(1);
    let list = ui.keys.get();
    if let Some(k) = idx.and_then(|i| list.get(i)) {
        ui.key.set(k.secret.clone());
        ui.algo_sel.set(k.algo.clamp(0, 2) as usize);
        apply_now(ui, ctx, false);
        return true;
    }
    false
}

/// TOTP 页 UI 构建（返回 (页面, 备份弹窗)：弹窗须挂根层级，遮罩才铺满全窗）
pub fn build_totp_tab(ui: &TotpUi) -> (Element, Element) {
    let TotpUi {
        key,
        account,
        issuer,
        algo_sel,
        output,
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
        let ui_gk = ui.clone();
        Element::button(lang::TOTP_GEN_KEY())
            .neutral()
            .icon_content(icons::stateful_icon(icons::KEY))
            .on_click(move |_| {
                // 下拉索引 0 = 32 位（160bit），1 = 16 位（80bit）
                let bits = if ui_gk.key_bits.get() == 1 { 16 } else { 32 };
                ui_gk.key.set(totp::generate_secret_key_bits(bits));
                ui_gk.output.set(String::new());
                ui_gk.status.set(String::new());
                ui_gk.qr.set(Vec::new());
                // 输入区已是新密钥：下拉退回占位行，选中回填（key_watch）不动作
                ui_gk.key_sel.set(0);
            })
    };

    // 生成验证码：点击即出码并显示（自动复制开启时额外复制，不再只复制不显示）
    let generate = {
        let ui_gen = ui.clone();
        Element::button(lang::TOTP_GENERATE())
            .neutral()
            .icon_content(icons::stateful_icon(icons::ZAP))
            .on_click(move |ctx| {
                // 先兜底同步：下拉新选中尚未回填时（sig_watch 未投递）也能出码
                sync_saved_selection(&ui_gen, ctx);
                // 记忆当前配置（账号/发行方/算法/密钥；密钥属敏感项，加密落盘）
                remember_totp(ui_gen.key, ui_gen.account, ui_gen.issuer, ui_gen.algo_sel);
                apply_now(&ui_gen, ctx, true);
            })
    };

    // 保存密钥到本地 store 数据库（多密钥管理：名称 = 账号@发行方）
    let save = {
        let ui_save = ui.clone();
        Element::button(lang::TOTP_SAVE())
            .neutral()
            .icon_content(icons::stateful_icon(icons::SAVE))
            .on_click(move |ctx| {
                sync_saved_selection(&ui_save, ctx);
                let k = ui_save.key.get().trim().to_string();
                if k.is_empty() {
                    return;
                }
                let name = if ui_save.issuer.get().trim().is_empty() {
                    ui_save.account.get().trim().to_string()
                } else {
                    format!(
                        "{}@{}",
                        ui_save.account.get().trim(),
                        ui_save.issuer.get().trim()
                    )
                };
                let entry = crate::core::store::TotpKey {
                    id: 0,
                    name,
                    secret: k,
                    algo: ui_save.algo_sel.get() as i64,
                };
                if crate::core::store::totp_upsert(&entry).is_ok() {
                    let list = crate::core::store::totp_list().unwrap_or_default();
                    // 下拉对准刚保存的这条（索引 +1 跳过占位行）；key_sel 版本
                    // 变化会触发回填，回填的是同一条密钥，语义无害
                    let pos = list
                        .iter()
                        .position(|k| k.secret == entry.secret)
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    ui_save.keys.set(list);
                    ui_save.key_sel.set(pos);
                    super::toast::ok(lang::TOTP_SAVED(&entry.secret));
                } else {
                    super::toast::err(lang::TOTP_SAVE_FAIL(String::from("store write failed")));
                }
            })
    };

    // 已存密钥下拉 + 删除。选项 = 占位行 + 各密钥名；选中回填由 key_watch
    //（sig_watch 监听 key_sel 版本）完成，Dropdown 的 on_click 不可用。
    let key_opts = ui.keys.map(|list| {
        if list.is_empty() {
            vec![lang::TOTP_KEYS_NONE()]
        } else {
            let mut opts = Vec::with_capacity(list.len() + 1);
            opts.push(lang::TOTP_KEY_PICK());
            opts.extend(list.iter().map(|k| k.name.clone()));
            opts
        }
    });
    // 选中已存密钥 → 回填密钥/算法并立即出码；复制仍走「生成」/自动复制开关。
    // 回填本体在 sync_saved_selection（key_watch 与动作入口共用）。
    let key_watch = {
        let ui_w = ui.clone();
        super::sig_watch::sig_watch(
            move || ui_w.key_sel.version(),
            false,
            move |ctx| {
                sync_saved_selection(&ui_w, ctx);
            },
        )
        .reactive()
    };
    let key_del_btn = {
        let ui_del = ui.clone();
        Element::button(lang::TOTP_KEY_DEL())
            .small()
            .neutral()
            .danger()
            .on_click(move |_| {
                // 索引 -1 跳过占位行；0 = 未选择，不删
                let idx = ui_del.key_sel.get().checked_sub(1);
                let list = ui_del.keys.get();
                if let Some(k) = idx.and_then(|i| list.get(i)) {
                    if crate::core::store::totp_del(k.id).is_ok() {
                        ui_del
                            .keys
                            .set(crate::core::store::totp_list().unwrap_or_default());
                        ui_del.key_sel.set(0);
                        ui_del.key.set(String::new());
                        ui_del.output.set(String::new());
                        ui_del.status.set(String::new());
                        ui_del.qr.set(Vec::new());
                    }
                }
            })
    };

    // 导入 otpauth:// URI：粘贴 URI → 解析回填（密钥/账号/发行方/算法）→ 入库
    // → 立即出码（旧版只回填不生成，验证码区空着，是「导入后看不到验证码」的直接原因）
    let import = {
        let ui_imp = ui.clone();
        Element::button(lang::TOTP_IMPORT())
            .neutral()
            .icon_content(icons::stateful_icon(icons::DOWNLOAD))
            .on_click(move |ctx| {
                let uri = ui_imp.key.get().trim().to_string();
                if uri.is_empty() {
                    return;
                }
                match totp::parse_otpauth(&uri) {
                    Ok(u) => {
                        ui_imp.key.set(u.secret.clone());
                        ui_imp.account.set(u.account.clone());
                        ui_imp.issuer.set(u.issuer.clone());
                        ui_imp.algo_sel.set(u.algo);
                        let entry = crate::core::store::TotpKey {
                            id: 0,
                            name: if u.issuer.is_empty() {
                                u.account.clone()
                            } else {
                                format!("{}@{}", u.account, u.issuer)
                            },
                            secret: u.secret.clone(),
                            algo: u.algo as i64,
                        };
                        if crate::core::store::totp_upsert(&entry).is_ok() {
                            let list = crate::core::store::totp_list().unwrap_or_default();
                            let pos = list
                                .iter()
                                .position(|k| k.secret == entry.secret)
                                .map(|i| i + 1)
                                .unwrap_or(0);
                            ui_imp.keys.set(list);
                            ui_imp.key_sel.set(pos);
                            super::toast::ok(lang::TOTP_IMPORT_DONE(entry.name));
                        } else {
                            super::toast::err(lang::TOTP_SAVE_FAIL(String::from(
                                "store write failed",
                            )));
                        }
                        apply_now(&ui_imp, ctx, true);
                    }
                    Err(e) => super::toast::err(e),
                }
            })
    };

    // 自动复制开关（开/关下拉，记忆于 settings）。持久化由 auto_copy_watch
    // 监听 auto_copy 版本完成——挂在下拉上的 on_click 会被框架静默丢弃。
    let auto_copy_dd = Element::dropdown(
        vec![lang::TOTP_AUTO_COPY_ON(), lang::TOTP_AUTO_COPY_OFF()],
        ui.auto_copy,
    )
    .width(110);
    let auto_copy_watch = {
        let ui_ac = ui.clone();
        super::sig_watch::sig_watch(
            move || ui_ac.auto_copy.version(),
            false,
            move |_| {
                crate::core::settings::commit(&[(
                    "totp.auto_copy",
                    Some(if ui_ac.auto_copy.get() == 1 { "1" } else { "0" }),
                )]);
            },
        )
        .reactive()
    };

    // 生成二维码（RGBA → list_signal 渲染）
    let gen_qr = {
        let ui_qr = ui.clone();
        Element::button(lang::TOTP_QR())
            .neutral()
            .icon_content(icons::stateful_icon(icons::QR))
            .on_click(move |ctx| {
                sync_saved_selection(&ui_qr, ctx);
                remember_totp(ui_qr.key, ui_qr.account, ui_qr.issuer, ui_qr.algo_sel);
                let k = ui_qr.key.get();
                if k.trim().is_empty() {
                    return;
                }
                let account_c = ui_qr.account.get();
                let issuer_c = ui_qr.issuer.get();
                let alg = totp::algo_from_index(ui_qr.algo_sel.get());
                match totp::generate_qr_entry(k.trim(), &account_c, &issuer_c, alg) {
                    Some(entry) => ui_qr.qr.set(vec![entry]),
                    None => ui_qr.qr.set(Vec::new()),
                }
            })
    };

    // 保存二维码为 PNG：用户选路径，RGBA 原样编码写入（rfd 选路径，png crate 编码）。
    // 仅在二维码已生成（qr 非空）时显示——无图可存时不占位。
    let has_qr = ui.qr.map(|v| !v.is_empty());
    let save_png = {
        let ui_png = ui.clone();
        Element::button(lang::TOTP_SAVE_PNG())
            .small()
            .neutral()
            .icon_content(icons::stateful_icon(icons::DOWNLOAD))
            .visible_when(move || has_qr.get())
            .on_click(move |_| {
                let Some(entry) = ui_png.qr.get().first().cloned() else {
                    return;
                };
                match crate::widgets::save_qr_png("qrcode.png", entry.w, entry.h, &entry.rgba) {
                    Ok(Some(path)) => {
                        super::toast::ok(lang::TOTP_PNG_SAVED(&path));
                    }
                    Ok(None) => {}
                    Err(e) => super::toast::err(lang::TOTP_PNG_SAVE_FAIL(e)),
                }
            })
    };

    // ── 实时输出区：验证码 + 状态行 + 倒计时（tick 派生，每秒随动）──
    let key_present = ui.key.map(|k| !k.trim().is_empty());
    let status_nonempty = ui.status.map(|s| !s.is_empty());
    let status_label = Element::label_signal(ui.status)
        .font_size(12.0)
        .fg_role(Role::Danger)
        .visible_when(move || status_nonempty.get());
    let remain_frac = ui
        .tick
        .map(|t| (PERIOD - t % PERIOD) as f32 / PERIOD as f32);
    let remain_text = ui.tick.map(|t| lang::TOTP_REFRESH_IN(PERIOD - t % PERIOD));
    let live_hint = Element::label(lang::TOTP_LIVE_HINT())
        .font_size(12.0)
        .fg_role(Role::TextMuted);
    let countdown = Element::col()
        .width_match()
        .spacing(6)
        .child(status_label)
        .child(
            Element::row()
                .width_match()
                .spacing(10)
                .cross(Align::Center)
                .child(Element::progress(remain_frac).width_match().weight(1.0))
                .child(
                    Element::label_signal(remain_text)
                        .font_size(12.0)
                        .fg_role(Role::TextMuted),
                ),
        );

    let qr_view = Element::list_signal(
        ui.qr,
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
                        .child(import)
                        .child(gen_qr),
                )
                .child(
                    // 导入提示：粘贴 otpauth:// URI 后点「导入」，回填并立即出码
                    Element::label(lang::TOTP_IMPORT_HINT())
                        .font_size(12.0)
                        .fg_role(Role::TextMuted),
                )
                .child(
                    // 已存密钥管理：下拉选择回填 + 删除（store.db 多密钥存储）
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::TOTP_KEYS_LABEL()).font_size(13.0))
                        .child(Element::dropdown_signal(key_opts, ui.key_sel).width(180))
                        .child(key_del_btn)
                        .child(Element::flex_spacer())
                        .child(Element::label(lang::TOTP_AUTO_COPY_LABEL()).font_size(13.0))
                        .child(auto_copy_dd),
                ),
        ))
        .child(card(
            &lang::TOTP_CARD_OUTPUT(),
            Element::row()
                .width_match()
                .spacing(16)
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(8)
                        .child(
                            // 只读可选文本：验证码可拖选/Ctrl+C 复制（不再借输入框承载）
                            select_text(output).font_size(26.0).width_match().height(96),
                        )
                        // 倒计时仅在配置了密钥时出现；无密钥时留一句引导提示
                        .child(
                            countdown
                                .width_match()
                                .visible_when(move || key_present.get()),
                        )
                        .child(live_hint.visible_when(move || !key_present.get())),
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

    // 无弹窗需要挂根层级（备份弹窗已移除）；根层列顺带挂载两个 sig_watch
    // （零尺寸叶子，参与信号版本跟踪，不占布局）：
    // - key_watch：已存密钥下拉选中 → 回填 + 出码
    // - auto_copy_watch：自动复制开关变化 → 持久化到 settings
    (page, Element::col().child(key_watch).child(auto_copy_watch))
}
