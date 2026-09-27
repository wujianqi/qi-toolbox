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
//! - [`crate::ui::nav`]：侧栏导航与关于页
//! - [`crate::widgets`]：扩展基础组件（card / 输入弹窗 / 站点管理 / 文件选择等）

use windui::prelude::*;

use crate::core;
use crate::lang;
use layout::build_ui;
use shell::{app_icon, set_window_title, system_light_theme};

mod db_page;
mod icons;
mod layout;
pub(crate) mod master;
mod mysql;
/// 侧栏导航项 / 主题按钮 / 关于页（原 widgets 中导航相关部分）。
mod nav;
mod password;
mod pg;
mod remote;
mod s3;
mod sftp;
mod sftp_row;
mod shell;
mod sql;
mod ssh_cmd;
mod table;
mod theme;
mod totp;
mod turso;

pub(crate) use crate::widgets::{card, input_dialog, select_text};
#[allow(unused_imports)]
pub(crate) use nav::nav_item;

/// 更新检查状态（关于页展示；启动后台检查与手动检查共用）。
#[derive(Clone, PartialEq)]
pub(crate) enum UpdateStatus {
    /// 未检测（启动后台检查还没回结果，或确认无新版）
    Idle,
    /// 检查中（手动点击后、后台线程未返回）
    Checking,
    /// 发现有新版（携带最新版本号）
    New(String),
    /// 已是最新版本
    UpToDate,
    /// 检查失败（网络等，静默提示可重试）
    Failed,
}

/// 更新检查状态信号：全局只建一次（thread_local 槽位永生）。
/// 自愈：thread_local 首建若发生在整树重建的 SignalScope 内会被收走，
/// is_alive=false 时换新槽位重建，避免残留死句柄（读取即 panic）。
pub(crate) fn update_status() -> Signal<UpdateStatus> {
    APP_UPDATE_STATUS.with(|v| {
        let mut v = v.borrow_mut();
        if !v.is_alive() {
            *v = signal(UpdateStatus::Idle);
        }
        v.clone()
    })
}

thread_local! {
    static APP_UPDATE_STATUS: std::cell::RefCell<Signal<UpdateStatus>> =
        std::cell::RefCell::new(signal(UpdateStatus::Idle));
    /// 更新检查通道发送端：run() 注册 channel 后存入，手动「检查更新」按钮借用。
    static UPDATE_TX: std::cell::RefCell<Option<Sender<UpdateMsg>>> =
        std::cell::RefCell::new(None);
}

/// 更新检查通道消息（后台线程 → UI 线程；Signal 非 Send，不能跨线程直接 set）。
pub(crate) enum UpdateMsg {
    New(String),
    Latest,
    Failed,
}

/// 发起一次更新检查：阻塞请求在后台线程执行，结果经通道回 UI 线程写状态信号。
/// 启动时的自动检查也走这里（run() 注册 channel 后调用）。
pub(crate) fn spawn_update_check() {
    UPDATE_TX.with(|v| {
        if let Some(tx) = v.borrow().as_ref() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let msg = match core::update::check_latest() {
                    Ok(latest) if core::update::is_newer(&latest, env!("CARGO_PKG_VERSION")) => {
                        UpdateMsg::New(latest)
                    }
                    Ok(_) => UpdateMsg::Latest,
                    Err(_) => UpdateMsg::Failed,
                };
                let _ = tx.send(msg);
            });
        }
    });
}

/// 全部页面状态 + 全局状态（在 `run()` 中一次性创建；主题切换整树重建不丢状态）。
/// 新增页面只需在此挂一个状态 struct，页面自己的信号/消息处理都在对应文件里。
#[derive(Clone)]
struct AppState {
    /// 当前导航页（模块 id：0=TOTP 1=密码 2=Turso 3=SFTP 4=远程 5=关于 6=S3 7=MySQL 8=PG；
    /// 内容页按 id 显隐，与侧栏显示顺序解耦）
    tab: Signal<usize>,
    /// 侧栏菜单顺序：模块 id 序列（拖拽可调，持久化到 AppData，启动回填）。
    /// 默认 SFTP/SSH、Turso 置顶，S3 第三行（高频功能优先），其余按原相对顺序跟随。
    nav_order: Signal<Vec<usize>>,
    /// 左侧菜单显示/隐藏（折叠手柄切换，持久化到 AppData）
    sidebar_visible: Signal<bool>,
    /// 主题：0=浅色 1=深色。默认跟随系统（启动时按注册表自动判断深浅）
    theme_mode: Signal<usize>,
    /// 主题切换时整树重建：图标染色等「构建期定色」随当前主题重新解析
    /// （Role 底色/文字色由框架每帧跟随主题，无需重建）
    theme_epoch: Signal<Vec<()>>,
    /// 窗体标题「APP_NAME - 模块名」：0.19 起 `App::title` 收 `TextContent`，
    /// 绑 `Signal<String>` 后切页/切语言 set 即跟随，无需平台原生 hack
    window_title: Signal<String>,
    totp: totp::TotpUi,
    password: password::PasswordUi,
    turso: turso::TursoUi,
    mysql: mysql::MySqlUi,
    pg: pg::PgUi,
    sftp: sftp::SftpUi,
    s3: s3::S3Ui,
    remote: remote::RemoteUi,
    /// 主口令门控（首次设置 / 换环境解锁弹窗）
    master_gate: master::MasterGate,
}

impl AppState {
    fn new() -> Self {
        // ── 主口令门控：必须最先执行（settings::load 解密敏感记忆项依赖已解锁）──
        // 首次启动强制设置；换机/恢复备份后弹解锁；同环境静默解锁不弹
        let master_gate = master::MasterGate::init();
        let s = Self {
            tab: signal(0usize),
            nav_order: signal(vec![3, 6, 2, 7, 8, 0, 1, 4, 5]),
            sidebar_visible: signal(true),
            theme_mode: signal(if system_light_theme() { 0 } else { 1 }),
            theme_epoch: signal(vec![()]),
            window_title: signal(String::new()),
            totp: totp::TotpUi::new(),
            password: password::PasswordUi::new(),
            turso: turso::TursoUi::new(),
            mysql: mysql::MySqlUi::new(),
            pg: pg::PgUi::new(),
            sftp: sftp::SftpUi::new(),
            s3: s3::S3Ui::new(),
            remote: remote::RemoteUi::new(),
            master_gate,
        };
        // ── 回填上次输入：AppData 记忆缓存（core::settings），尽力而为 ──
        // 文本输入为空时跳过（无记忆价值，保留默认占位）；数值/下拉索引做越界防护。
        let saved = core::settings::load();
        // ── 恢复侧栏菜单顺序（逗号分隔的模块 id；须 9 个且无重复才采纳）──
        if let Some(v) = saved.get("nav.order") {
            let ids: Vec<usize> = v
                .split(',')
                .filter_map(|p| p.trim().parse::<usize>().ok())
                .filter(|&i| i < 9)
                .collect();
            let mut seen = [false; 9];
            let ok = ids.len() == 9
                && ids.iter().all(|&i| {
                    if seen[i] {
                        false
                    } else {
                        seen[i] = true;
                        true
                    }
                });
            if ok {
                // 旧 7 项顺序视为「未含 MySQL/PG」，迁移到新默认（追加在 S3 之后）；
                // 已含 9 项的用户拖拽顺序原样保留。
                if ids.len() == 9 && ids != [0, 1, 2, 3, 4, 5, 6, 7, 8] {
                    s.nav_order.set(ids);
                }
            }
        }
        // ── 默认模块 = 菜单第一项：用户改过菜单则以当前菜单顺序为准 ──
        if let Some(&first) = s.nav_order.get().first() {
            s.tab.set(first);
        }
        // ── 恢复侧栏开合状态（"0"=收起；缺省/其它值=展开）──
        if saved.get("sidebar.show").map(String::as_str) == Some("0") {
            s.sidebar_visible.set(false);
        }
        let fill = |key: &str, sig: Signal<String>| {
            if let Some(v) = saved.get(key).filter(|v| !v.is_empty()) {
                sig.set(v.clone());
            }
        };
        let fill_idx = |key: &str, sig: Signal<usize>, max: usize| {
            if let Some(i) = saved.get(key).and_then(|v| v.parse::<usize>().ok()) {
                if i < max {
                    sig.set(i);
                }
            }
        };
        fill("sftp.host", s.sftp.host);
        fill("sftp.port", s.sftp.port);
        fill("sftp.user", s.sftp.user);
        if let Some(v) = saved.get("sftp.pass") {
            s.sftp.pass.set(v.clone());
        }
        fill("mysql.host", s.mysql.host);
        fill("mysql.port", s.mysql.port);
        fill("mysql.user", s.mysql.user);
        if let Some(v) = saved.get("mysql.pass") {
            s.mysql.pass.set(v.clone());
        }
        fill("pg.url", s.pg.url);
        fill("totp.account", s.totp.account);
        fill("totp.issuer", s.totp.issuer);
        fill_idx("totp.algo", s.totp.algo_sel, 3);
        if let Some(v) = saved.get("totp.key") {
            s.totp.key.set(v.clone());
        }
        fill("pwd.input", s.password.input);
        fill_idx(
            "pwd.platform",
            s.password.platform,
            core::password::PlatformPreset::all().len(),
        );
        fill_idx(
            "pwd.algo",
            s.password.algo,
            core::password::HashAlgorithm::all().len(),
        );
        fill("remote.url", s.remote.url_input);
        s
    }
}

/// 把 windui channel 发送端包装为业务层的消息投递口（core 不感知 windui）。
pub(crate) fn sink<T: Send + 'static>(tx: Sender<T>) -> core::MsgSink<T> {
    Box::new(move |m| {
        let _ = tx.send(m);
    })
}

// ══════════════════════════════════════════════════════════════════
// 应用入口：创建全部状态信号 + 注册后台任务通道，构建 Element 树，运行窗口
// ══════════════════════════════════════════════════════════════════

pub fn run() {
    let state = AppState::new();
    // 窗口标题绑 Signal：初始为「APP_NAME - 默认模块」，切页/切语言时 set 即跟随
    // （应用图标 = logo.svg，任务栏/标题栏/Alt+Tab 均生效）
    let first_tab = state.tab.get();
    let title_sig = state.window_title;
    set_window_title(title_sig, first_tab);
    let mut app = App::new(lang::APP_NAME(), 1000, 700)
        .title(title_sig)
        .icon(app_icon())
        .theme(if system_light_theme() {
            theme::light()
        } else {
            theme::dark()
        });

    // 更新检查通道：后台线程请求 GitHub Releases，结果回 UI 线程写状态信号
    // （Signal 非 Send，不能跨线程直接 set）；发送端存全局，供手动「检查更新」复用
    let tx_update = app.channel::<UpdateMsg>(move |_ctx, msg| {
        update_status().set(match msg {
            UpdateMsg::New(v) => UpdateStatus::New(v),
            UpdateMsg::Latest => UpdateStatus::UpToDate,
            UpdateMsg::Failed => UpdateStatus::Failed,
        });
    });
    UPDATE_TX.with(|v| *v.borrow_mut() = Some(tx_update));
    // 启动后台检查一次（尽力而为，失败静默不占位）
    spawn_update_check();

    // ── 数据库后台任务通道：Turso/MySQL/PG 各自独立通道 ──
    // 三页共用一个通道时 TableLoaded 等消息无来源标记，会被其它页的
    // on_db_msg 误消费（MySQL 页显示 PG 数据）；各页 spawn 时传自己的
    // tx()，拆分通道后消息只回到发起页。
    let chan_t = state.clone();
    let tx_turso = app.channel::<core::db::DbMsg>(move |_ctx, msg| {
        chan_t.turso.on_db_msg(msg);
    });
    let chan_m = state.clone();
    let tx_mysql = app.channel::<core::db::DbMsg>(move |_ctx, msg| {
        chan_m.mysql.on_db_msg(msg);
    });
    let chan_p = state.clone();
    let tx_pg = app.channel::<core::db::DbMsg>(move |_ctx, msg| {
        chan_p.pg.on_db_msg(msg);
    });
    state.turso.set_tx(tx_turso);
    state.mysql.set_tx(tx_mysql);
    state.pg.set_tx(tx_pg);

    // ── SFTP 后台工作线程：SSH/SFTP 会话跨命令存活，结果经通道回 UI 线程 ──
    let chan2 = state.clone();
    let tx_sftp = app.channel::<core::sftp::SftpMsg>(move |_ctx, msg| chan2.sftp.on_msg(msg));
    // spawn_worker 返回（命令发送端, Exec 中断标志），命令窗口「停止」复用同一句柄
    let (sftp_cmd_tx, sftp_cancel) = core::sftp::spawn_worker(sink(tx_sftp));
    state.sftp.set_cmd(sftp_cmd_tx);
    state.sftp.set_cancel(sftp_cancel);
    // ── 远程检测通道：网络探测在后台线程执行，结果经此回 UI 线程 ──
    let chan3 = state.clone();
    let tx_remote =
        app.channel::<core::remote::RemoteMsg>(move |_ctx, msg| chan3.remote.on_msg(msg));
    state.remote.set_tx(tx_remote);

    // ── S3 后台工作线程：阻塞 HTTP 在后台执行，结果经通道回 UI 线程 ──
    let chan4 = state.clone();
    let tx_s3 = app.channel::<core::s3::S3Msg>(move |_ctx, msg| chan4.s3.on_msg(msg));
    state.s3.set_cmd(core::s3::spawn_worker(sink(tx_s3)));

    // 运行期主题句柄：克隆进主题按钮回调，set() 下一帧热切换
    let th = app.theme_handle();

    // 关闭软件时清空剪贴板：验证码/密码等敏感内容不残留（用户要求；不做定时清空）
    let app = app.on_close_request(|ctx| {
        ctx.clipboard_set("");
        true // 返回 true 继续默认关闭流程
    });

    // UI 挂在 host_signal 上：theme_epoch 变化（主题切换）即整树重建
    let root = Element::host_signal(state.theme_epoch, move |_| build_ui(&state, th.clone()));

    // 必须复用注册过 channel 的同一个 App 实例（否则后台线程消息无人接收）
    app.content(root).run();
}
