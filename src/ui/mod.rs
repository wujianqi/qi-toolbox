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
mod select_text;
mod sftp;
mod ssh_cmd;
mod sql;
mod table;
mod totp;
mod turso;
mod widgets;
pub(crate) mod syntax_input;

pub(crate) use widgets::{card, input_dialog, select_text};

/// 全部页面状态 + 全局状态（在 `run()` 中一次性创建；主题切换整树重建不丢状态）。
/// 新增页面只需在此挂一个状态 struct，页面自己的信号/消息处理都在对应文件里。
#[derive(Clone)]
struct AppState {
    /// 当前导航页（模块 id：0=TOTP 1=密码 2=Turso 3=SFTP 4=远程 5=关于；
    /// 内容页按 id 显隐，与侧栏显示顺序解耦）
    tab: Signal<usize>,
    /// 侧栏菜单顺序：模块 id 序列（拖拽可调，持久化到 AppData，启动回填）。
    /// 默认 SFTP/SSH、Turso 置顶（高频功能优先），其余按原相对顺序跟随。
    nav_order: Signal<Vec<usize>>,
    /// 左侧菜单显示/隐藏（折叠手柄切换，持久化到 AppData）
    sidebar_visible: Signal<bool>,
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
        let s = Self {
            tab: signal(0usize),
            nav_order: signal(vec![3, 2, 0, 1, 4, 5]),
            sidebar_visible: signal(true),
            theme_mode: signal(if system_light_theme() { 0 } else { 1 }),
            theme_epoch: signal(vec![()]),
            totp: totp::TotpUi::new(),
            password: password::PasswordUi::new(),
            turso: turso::TursoUi::new(),
            sftp: sftp::SftpUi::new(),
            remote: remote::RemoteUi::new(),
        };
        // ── 回填上次输入：AppData 记忆缓存（core::settings），尽力而为 ──
        // 文本输入为空时跳过（无记忆价值，保留默认占位）；数值/下拉索引做越界防护。
        let saved = core::settings::load();
        // ── 恢复侧栏菜单顺序（逗号分隔的模块 id；须 6 个且无重复才采纳）──
        if let Some(v) = saved.get("nav.order") {
            let ids: Vec<usize> = v
                .split(',')
                .filter_map(|p| p.trim().parse::<usize>().ok())
                .filter(|&i| i < 6)
                .collect();
            let mut seen = [false; 6];
            let ok = ids.len() == 6
                && ids.iter().all(|&i| {
                    if seen[i] {
                        false
                    } else {
                        seen[i] = true;
                        true
                    }
                });
            if ok {
                // 旧默认顺序 (0,1,2,3,4,5) 视为「未自定义」，迁移到新默认
                // （SFTP/SSH、Turso 置顶）；用户拖拽过的顺序原样保留。
                if ids != [0, 1, 2, 3, 4, 5] {
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
        fill("turso.db_path", s.turso.db_path);
        fill_idx("turso.mode", s.turso.turso_mode, 2);
        fill("turso.url", s.turso.turso_url);
        if let Some(v) = saved.get("turso.token") {
            s.turso.turso_token.set(v.clone());
        }
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

/// 模块 id → 模块名（窗体标题后缀，与侧栏名称一致：如「启途 - 关于」）
fn module_name(id: usize) -> &'static str {
    match id {
        0 => lang::TAB_2FA(),
        1 => lang::TAB_PASSWORD(),
        2 => lang::TAB_TURSO(),
        3 => lang::TAB_SFTP(),
        4 => lang::TAB_REMOTE(),
        _ => lang::TAB_ABOUT(),
    }
}

/// 同步主窗体标题为「APP_NAME - 模块名」。windui 不提供运行期改标题 API，
/// Windows 上直接调 SetWindowTextW（按自身 PID 找主窗口）；其它平台为 no-op
/// （标题保持 APP_NAME，未来跨平台版本可换平台原生实现）。
fn sync_window_title(tab: usize) {
    let text = format!("{} - {}", lang::APP_NAME(), module_name(tab));
    set_window_title(&text);
}

#[cfg(windows)]
fn set_window_title(text: &str) {
    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, SetWindowTextW};

    // EnumWindows 回调：找本进程的可见顶层窗口（主 GUI），把句柄写回上下文
    unsafe extern "system" fn find_main_window(
        hwnd: windows_sys::Win32::Foundation::HWND,
        lparam: LPARAM,
    ) -> i32 {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindowVisible};
        let ctx = lparam as *mut (u32, isize);
        let mut wpid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut wpid);
        if wpid == (*ctx).0 && IsWindowVisible(hwnd) != 0 {
            (*ctx).1 = hwnd;
            return 0; // FALSE：找到即停止枚举
        }
        1 // TRUE：继续
    }

    let mut ctx: (u32, isize) = (std::process::id(), 0);
    unsafe {
        EnumWindows(Some(find_main_window), &mut ctx as *mut (u32, isize) as LPARAM);
        // windows-sys 中 HWND = isize：找到的主窗口句柄非 0 即有效
        let hwnd = ctx.1;
        if hwnd != 0 {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            SetWindowTextW(hwnd, wide.as_ptr());
        }
    }
}

#[cfg(not(windows))]
fn set_window_title(_text: &str) {}

// ══════════════════════════════════════════════════════════════════
// 应用入口：创建全部状态信号 + 注册后台任务通道，构建 Element 树，运行窗口
// ══════════════════════════════════════════════════════════════════

pub fn run() {
    let state = AppState::new();
    // 窗口标题跟随启动时语言（中文系统 → 骑途，其它 → Qi Toolbox）；
    // 应用图标 = logo.svg（任务栏/标题栏/Alt+Tab 均生效）
    let mut app = App::new(lang::APP_NAME(), 1000, 700).icon(app_icon());
    // 主窗口就绪后把「APP_NAME - 默认模块」写入窗体标题（一次性后台线程，
    // 之后每次切页/切语言由对应回调同步；windui 无运行期标题接口，见 set_window_title）
    // 默认模块 = 菜单第一项（见 AppState::new 的回填逻辑）
    let first_tab = state.tab.get();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        sync_window_title(first_tab);
    });

    // ── Turso 后台任务通道：数据库操作在后台线程执行，结果经此回 UI 线程 ──
    // 消息统一交给 TursoUi::on_db_msg 消费（写信号 + 节流续接排队表）。
    let chan = state.clone();
    let tx = app.channel::<core::db::DbMsg>(move |_ctx, msg| chan.turso.on_db_msg(msg));
    state.turso.set_tx(tx.clone());

    // ── SFTP 后台工作线程：SSH/SFTP 会话跨命令存活，结果经通道回 UI 线程 ──
    let chan2 = state.clone();
    let tx_sftp = app.channel::<core::sftp::SftpMsg>(move |_ctx, msg| chan2.sftp.on_msg(msg));
    // spawn_worker 返回（命令发送端, Exec 中断标志），命令窗口「停止」复用同一句柄
    let (sftp_cmd_tx, sftp_cancel) = core::sftp::spawn_worker(sink(tx_sftp));
    state.sftp.set_cmd(sftp_cmd_tx);
    state.sftp.set_cancel(sftp_cancel);
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
    let sidebar_visible = state.sidebar_visible;

    /// 侧栏宽度（逻辑 px）。折叠手柄的停靠位置以它为基准（贴齐右侧分隔线）。
    const SIDEBAR_W: i32 = 196;
    /// 折叠手柄宽度。分隔线与内容区之间的空隙也用这个值——手柄恰好嵌在
    /// 空隙里，不遮挡内容。
    const HANDLE_W: i32 = 9;

    // ── 左侧栏：品牌区 + 可拖拽排序的导航（2FA / 密码 / Turso / SFTP/SSH / 远程 / 关于）──
    // 模块定义：数组下标即模块 id，内容页按 id 显隐；拖拽只调侧栏顺序、不改 id。
    let nav_items: [(&str, &[u8]); 6] = [
        (lang::TAB_2FA(), icons::ZAP),
        (lang::TAB_PASSWORD(), icons::LOCK),
        (lang::TAB_TURSO(), icons::TABLE_ICON),
        (lang::TAB_SFTP(), icons::SERVER),
        (lang::TAB_REMOTE(), icons::GLOBE),
        (lang::TAB_ABOUT(), icons::INFO),
    ];
    // 数据驱动重排：顺序真值源 = nav_order 信号（拖拽后应用自行改信号 → 整列重建，
    // 反向同步天然成立，恢复默认/重新载入配置都只需要 set 信号）
    let nav_order = state.nav_order;
    let nav_list = Element::reorder_list_signal(nav_order, {
        move |id: usize, handle: Element| {
            let (name, icon) = nav_items[id];
            // 手柄以覆盖层与可点行并列（不能嵌进 clickable 祖先，否则冒泡被 Clickable 截断）；
            // 覆盖层空白区命中穿透（与 nav_item 的指示条先例一致），只有手柄可按住拖动
            Element::stack()
                .width_match()
                .height(38)
                .child(widgets::nav_item(name, icon, id, tab, move |mid| {
                    // 切页后把当前模块写入窗体标题（如「启途 - 关于」）
                    sync_window_title(mid);
                }))
                .child(
                    Element::row()
                        .fill()
                        .child(Element::flex_spacer())
                        .child(
                            Element::stack()
                                .width(22)
                                .height(38)
                                .child(handle.align(Align::Center)),
                        ),
                )
        }
    })
    .on_reorder(move |_ctx, from, to| {
        // 移动模块 id（顺序未变时框架不触发回调）
        nav_order.update(|v| {
            let x = v.remove(from);
            v.insert(to.min(v.len()), x);
        });
        // 持久化菜单顺序（尽力而为，下次启动回填）
        let joined = nav_order
            .get()
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        crate::core::settings::commit(&[("nav.order", Some(joined.as_str()))]);
    });
    let nav_col = Element::col().width_match().spacing(3).child(nav_list);

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
                // 界面语言切换后，窗体标题中的模块名跟随当前语言刷新
                sync_window_title(tab.get());
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
        .width(SIDEBAR_W)
        .height_match()
        .bg_role(Role::Bg)
        .padding_xy(10, 12)
        .spacing(12)
        .visible_when(move || sidebar_visible.get())
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

    // ── 菜单折叠手柄：贴在侧栏分隔线右侧的**右半胶囊**（SVG 实心形状，9×46，
    // 左边缘与分隔线严丝合缝，染 Divider 色后视觉上是"线条鼓出的一块"），
    // 点击收起/展开左侧菜单。两个方向变体由 visible_when 逐帧二选一（图标随
    // 开合翻转，无需整树重建）；外层 row 无 clickable，空白区命中穿透，
    // 只有手柄本体吃点击。
    let make_handle = move |chevron: &[u8]| {
        Element::stack()
            .width(HANDLE_W)
            .height(46)
            // hover 反馈（clickable 的半透明叠层）用同半径圆角，贴合半胶囊轮廓
            .corner(4.5)
            .clickable()
            .tooltip(lang::TOGGLE_SIDEBAR())
            .on_click(move |_| {
                sidebar_visible.set(!sidebar_visible.get());
                // 记忆开合状态（尽力而为，下次启动回填）
                let show = sidebar_visible.get();
                crate::core::settings::commit(&[(
                    "sidebar.show",
                    Some(if show { "1" } else { "0" }),
                )]);
            })
            .child(
                Element::image_content(
                    ImageContent::from_svg_bytes(icons::HANDLE_TAB, Some(9))
                        .tint(Role::Divider.resolve(&windui::theme::current())),
                )
                .align(Align::Center),
            )
            .child(
                Element::image_content(
                    ImageContent::from_svg_bytes(chevron, Some(14))
                        .tint(Role::TextMuted.resolve(&windui::theme::current())),
                )
                .align(Align::Center),
            )
    };
    // 展开态：分隔线占 [SIDEBAR_W, SIDEBAR_W+1)，手柄左缘从线右侧 (SIDEBAR_W+1) 起贴齐。
    let handle_open = Element::row()
        .fill()
        .cross(Align::Center)
        .visible_when(move || sidebar_visible.get())
        .child(Element::leaf().width(SIDEBAR_W + 1))
        .child(make_handle(icons::CHEVRON_LEFT));
    // 收起态：分隔线退到最左 [0,1)，手柄同样贴其右侧。
    let handle_closed = Element::row()
        .fill()
        .cross(Align::Center)
        .visible_when(move || !sidebar_visible.get())
        .child(Element::leaf().width(1))
        .child(make_handle(icons::CHEVRON_RIGHT));

    // ── 根节点：左侧栏 + 分隔线 + 手柄空隙 + 右侧内容区（+ 折叠手柄浮层）──
    // 分隔线后留 HANDLE_W 空隙：手柄整条嵌在空隙内（展开/收起都贴线右侧），
    // 不遮挡内容。
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
                .child(Element::leaf().width(HANDLE_W))
                .child(content),
        )
        .child(handle_open)
        .child(handle_closed)
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
