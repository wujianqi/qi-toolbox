//! Qi Toolbox — 界面层（windui）
//!
//! 只做渲染与交互：业务逻辑一律在 [`crate::core`]，本层把用户操作翻译成 core
//! 调用、把 core 结果写回状态信号。windui 是 retained-mode 命令式 Builder 框架：
//! UI 在 `run()` 里一次性构建成 `Element` 树，状态全部由 `Signal<T>`（Copy 句柄）
//! 承载，写入自动触发重绘。
//!
//! 板块按页独立成文件（状态 + 页面 + 后台消息处理同处一个文件，便于扩展）：
//! - [`crate::ui::totp`]：TOTP 页
//! - [`crate::ui::password`]：密码页
//! - [`crate::ui::turso`]：Turso 数据库浏览页
//! - [`crate::ui::sftp`]：SFTP 页
//! - [`crate::ui::table`]：表格列表 / 数据表格渲染
//! - [`crate::ui::sql`]：SQL 查询面板
//! - [`crate::ui::widgets`]：共享组件（card / nav_item / theme_toggle / about）

use windui::prelude::*;

use crate::core;
use crate::lang;

mod icons;
mod password;
mod remote;
mod sftp;
mod ssh_cmd;
mod sql;
mod table;
mod totp;
mod turso;
mod widgets;

pub(crate) use widgets::{card, input_dialog};

/// 全部页面状态 + 全局状态（在 `run()` 中一次性创建；主题切换整树重建不丢状态）。
/// 新增页面只需在此挂一个状态 struct，页面自己的信号/消息处理都在对应文件里。
#[derive(Clone)]
struct AppState {
    /// 当前导航页（0=TOTP 1=密码 2=Turso 3=SFTP 4=关于）
    tab: Signal<usize>,
    /// 主题：0=浅色 1=深色。默认跟随系统（启动时按注册表自动判断深浅）
    theme_mode: Signal<usize>,
    /// 主题切换时整树重建：图标染色等「构建期定色」随当前主题重新解析
    /// （Role 底色/文字色由框架每帧跟随主题，无需重建）
    theme_epoch: Signal<Vec<()>>,
    totp: totp::TotpUi,
    password: password::PasswordUi,
    turso: turso::TursoUi,
    sftp: sftp::SftpUi,
    remote: remote::RemoteUi,
}

impl AppState {
    fn new() -> Self {
        Self {
            tab: signal(0usize),
            theme_mode: signal(if system_light_theme() { 0 } else { 1 }),
            theme_epoch: signal(vec![()]),
            totp: totp::TotpUi::new(),
            password: password::PasswordUi::new(),
            turso: turso::TursoUi::new(),
            sftp: sftp::SftpUi::new(),
            remote: remote::RemoteUi::new(),
        }
    }
}

/// 把 windui channel 发送端包装为业务层的消息投递口（core 不感知 windui）。
pub(crate) fn sink<T: Send + 'static>(tx: Sender<T>) -> core::MsgSink<T> {
    Box::new(move |m| {
        let _ = tx.send(m);
    })
}

/// 应用图标：`logo.svg` 矢量源按需光栅化。平台在**不同场合要不同尺寸**
/// 的图标（任务栏取大档，标题栏取小档），`IconSource::sized` 让每档都按
/// 实际物理像素 1:1 现画，避免固定位图交给系统缩放导致高 DPI 发糊。
fn app_icon() -> windui::icon::IconSource {
    windui::icon::IconSource::sized(|size| {
        // Image::from_svg_bytes 返回 Result；失败走 1×1 透明像素兜底
        windui::render::image::Image::from_svg_bytes(icons::LOGO, Some(size))
            .ok()
            .and_then(|img| windui::icon::WindowIcon::from_image(&img))
            .unwrap_or_else(|| {
                // 解析失败兜底：1×1 透明像素（正常不会走到）
                windui::icon::WindowIcon::from_rgba(1, 1, vec![0, 0, 0, 0])
                    .expect("1×1 RGBA 必然合法")
            })
    })
}

// ══════════════════════════════════════════════════════════════════
// 应用入口：创建全部状态信号 + 注册后台任务通道，构建 Element 树，运行窗口
// ══════════════════════════════════════════════════════════════════

pub fn run() {
    let state = AppState::new();
    // 窗口标题跟随启动时语言（中文系统 → 骑途，其它 → Qi Toolbox）；
    // 应用图标 = logo.svg（任务栏/标题栏/Alt+Tab 均生效）
    let mut app = App::new(lang::APP_NAME(), 1000, 700).icon(app_icon());

    // ── Turso 后台任务通道：数据库操作在后台线程执行，结果经此回 UI 线程 ──
    // 消息统一交给 TursoUi::on_db_msg 消费（写信号 + 节流续接排队表）。
    let chan = state.clone();
    let tx = app.channel::<core::db::DbMsg>(move |_ctx, msg| chan.turso.on_db_msg(msg));
    state.turso.set_tx(tx.clone());

    // ── SFTP 后台工作线程：SSH/SFTP 会话跨命令存活，结果经通道回 UI 线程 ──
    let chan2 = state.clone();
    let tx_sftp = app.channel::<core::sftp::SftpMsg>(move |_ctx, msg| chan2.sftp.on_msg(msg));
    state
        .sftp
        .set_cmd(core::sftp::spawn_worker(sink(tx_sftp)));

    // ── 远程检测通道：网络探测在后台线程执行，结果经此回 UI 线程 ──
    let chan3 = state.clone();
    let tx_remote = app.channel::<core::remote::RemoteMsg>(move |_ctx, msg| {
        chan3.remote.on_msg(msg)
    });
    state.remote.set_tx(tx_remote);

    // 运行期主题句柄：克隆进主题按钮回调，set() 下一帧热切换
    let th = app.theme_handle();

    // UI 挂在 host_signal 上：theme_epoch 变化（主题切换）即整树重建
    let root = Element::host_signal(state.theme_epoch, move |_| {
        build_ui(&state, th.clone())
    });

    // 必须复用注册过 channel 的同一个 App 实例（否则后台线程消息无人接收）
    app.content(root).run();
}

// ══════════════════════════════════════════════════════════════════
// 主界面构建：左侧栏导航 + 右侧内容区
// （主题切换时经 host_signal 以 theme_epoch 为信号整树重建）
// ══════════════════════════════════════════════════════════════════

fn build_ui(state: &AppState, th: ThemeHandle) -> Element {
    // Signal 为 Copy 句柄：复制出来供「构建期」闭包共享
    let tab = state.tab;
    let theme_mode = state.theme_mode;
    let theme_epoch = state.theme_epoch;

    // ── 左侧栏：品牌区 + 六个导航项（2FA / 密码 / Turso / SFTP / 远程检测 / 关于）──
    let mut nav_col = Element::col().width_match().spacing(3);
    // 导航项：文案为运行时函数（语言切换后随整树重建刷新），故用 let 而非 const
    let nav_items: [(&str, &[u8]); 6] = [
        (lang::TAB_2FA(), icons::ZAP),
        (lang::TAB_PASSWORD(), icons::LOCK),
        (lang::TAB_TURSO(), icons::TABLE_ICON),
        (lang::TAB_SFTP(), icons::SERVER),
        (lang::TAB_REMOTE(), icons::GLOBE),
        (lang::TAB_ABOUT(), icons::INFO),
    ];
    for (i, (name, icon)) in nav_items.iter().enumerate() {
        nav_col = nav_col.child(widgets::nav_item(name, icon, i, tab));
    }

    // ── 侧栏底部：语言 + 主题 两个二选一 toggle 并列一行 ──
    // 语言 toggle：无底色 clickable 行，仅文字随状态变化（中↔EN），点击切到另一种
    let lang_toggle = {
        let is_zh = lang::current() == lang::LANG_ZH;
        Element::stack()
            .clickable()
            .on_click(move |_| {
                lang::set_current(if lang::current() == lang::LANG_ZH {
                    lang::LANG_EN
                } else {
                    lang::LANG_ZH
                });
                theme_epoch.update(|v| v.push(()));
            })
            .weight(1.0)
            .height(28)
            .corner(6.0)
            .tooltip(lang::TOGGLE_LANG())
            .child(
                Element::label(if is_zh { "中" } else { "EN" })
                    .font_size(12.0)
                    .font_weight(600)
                    .fg_role(Role::TextMuted)
                    .align(Align::Center),
            )
    };
    // 主题 toggle：图标显示当前深浅色，点击切换；启动时默认跟随系统
    let theme_btn = widgets::theme_toggle(theme_mode, th, theme_epoch);
    let footer_row = Element::row()
        .width_match()
        .spacing(4)
        .child(lang_toggle)
        .child(theme_btn);

    let sidebar = Element::col()
        .width(196)
        .height_match()
        .bg_role(Role::Bg)
        .padding_xy(10, 12)
        .spacing(12)
        .child(
            Element::col()
                .width_match()
                .spacing(2)
                .padding_xy(10, 6)
                .child(
                    Element::label(lang::APP_NAME())
                        .font_size(16.0)
                        .font_weight(700)
                        .fg_role(Role::Text),
                )
                .child(
                    Element::label(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                ),
        )
        .child(Element::scroll().weight(1.0).child(nav_col))
        .child(footer_row);

    // ── 内容区：六个页面按导航显隐 ──
    let content = Element::stack()
        .height_match()
        .weight(1.0)
        .child(
            totp::build_totp_tab(&state.totp).visible_when(move || tab.get() == 0),
        )
        .child(
            password::build_password_tab(&state.password).visible_when(move || tab.get() == 1),
        )
        .child(turso::build_turso_tab(&state.turso).visible_when(move || tab.get() == 2))
        .child(sftp::build_sftp_tab(&state.sftp).visible_when(move || tab.get() == 3))
        .child(remote::build_remote_tab(&state.remote).visible_when(move || tab.get() == 4))
        .child(widgets::build_about_page().visible_when(move || tab.get() == 5));

    // ── 根节点：左侧栏 + 分隔线 + 右侧内容区 ──
    Element::stack()
        .fill()
        .bg_role(Role::Bg)
        .child(
            Element::row()
                .fill()
                .child(sidebar)
                .child(
                    Element::leaf()
                        .width(1)
                        .height_match()
                        .bg_role(Role::Divider),
                )
                .child(content),
        )
}

/// 读取 Windows「应用」深浅色偏好（注册表 AppsUseLightTheme：1=浅色 0=深色）。
/// 读取失败或非 Windows 平台一律按浅色处理。
#[cfg(windows)]
fn system_light_theme() -> bool {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ,
    };
    let path: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let name: Vec<u16> = "AppsUseLightTheme"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut key = 0isize;
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut key) == 0
            && RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                &mut value as *mut u32 as *mut u8,
                &mut size,
            ) == 0
            && size == 4
    };
    if key != 0 {
        unsafe {
            RegCloseKey(key);
        }
    }
    ok && value != 0
}

#[cfg(not(windows))]
fn system_light_theme() -> bool {
    true
}
