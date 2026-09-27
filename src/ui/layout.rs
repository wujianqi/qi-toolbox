//! 主界面构建（从 `mod.rs` 拆出）：左侧栏导航 + 右侧内容区 + 折叠手柄。
//! （主题切换时经 host_signal 以 theme_epoch 为信号整树重建）

use windui::prelude::*;

use super::nav;
use super::shell::set_window_title;
use super::{icons, mysql, password, pg, remote, s3, sftp, totp, turso, AppState};
use crate::lang;

// ══════════════════════════════════════════════════════════════════
// 主界面构建：左侧栏导航 + 右侧内容区
// （主题切换时经 host_signal 以 theme_epoch 为信号整树重建）
// ══════════════════════════════════════════════════════════════════

pub(super) fn build_ui(state: &AppState, th: ThemeHandle) -> Element {
    // Signal 为 Copy 句柄：复制出来供「构建期」闭包共享
    let tab = state.tab;
    let theme_mode = state.theme_mode;
    let theme_epoch = state.theme_epoch;
    let sidebar_visible = state.sidebar_visible;
    let gate_show = state.master_gate.show;

    /// 侧栏宽度（逻辑 px）。折叠手柄的停靠位置以它为基准（贴齐右侧分隔线）。
    const SIDEBAR_W: i32 = 196;
    /// 折叠手柄宽度。分隔线与内容区之间的空隙也用这个值——手柄恰好嵌在
    /// 空隙里，不遮挡内容。
    const HANDLE_W: i32 = 9;

    // ── 左侧栏：品牌区 + 可拖拽排序的导航（2FA / 密码 / Turso / SFTP/SSH / 远程 / 关于）──
    // 模块定义：数组下标即模块 id，内容页按 id 显隐；拖拽只调侧栏顺序、不改 id。
    let nav_items: [(String, &[u8]); 9] = [
        (lang::TAB_2FA(), icons::ZAP),
        (lang::TAB_PASSWORD(), icons::LOCK),
        (lang::TAB_TURSO(), icons::SQLITE),
        (lang::TAB_SFTP(), icons::SERVER),
        (lang::TAB_REMOTE(), icons::GLOBE),
        (lang::TAB_ABOUT(), icons::INFO),
        (lang::S3_TAB(), icons::CLOUD),
        (lang::MYSQL_TAB(), icons::MYSQL),
        (lang::PG_TAB(), icons::POSTGRESQL),
    ];
    // 数据驱动重排：顺序真值源 = nav_order 信号（拖拽后应用自行改信号 → 整列重建，
    // 反向同步天然成立，恢复默认/重新载入配置都只需要 set 信号）
    let nav_order = state.nav_order;
    let title_sig = state.window_title;
    let nav_list = Element::reorder_list_signal(nav_order, {
        move |id: usize, handle: Element| {
            let (name, icon) = (&nav_items[id].0, nav_items[id].1);
            // 手柄以覆盖层与可点行并列（不能嵌进 clickable 祖先，否则冒泡被 Clickable 截断）；
            // 覆盖层空白区命中穿透（与 nav_item 的指示条先例一致），只有手柄可按住拖动
            Element::stack()
                .width_match()
                .height(38)
                .child(nav::nav_item(name, icon, id, tab, move |mid| {
                    // 切页后把当前模块写入窗体标题（如「启途 - 关于」）
                    set_window_title(title_sig, mid);
                }))
                .child(
                    Element::row().fill().child(Element::flex_spacer()).child(
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
        let locale = lang::LocaleHandle::new();
        let title = state.window_title;
        let is_zh = lang::is_zh();
        Element::stack()
            .clickable()
            .on_click(move |_| {
                // zh-CN ↔ en 热切换：Catalog 失效后下一帧整树跟随
                if lang::is_zh() {
                    locale.set("en");
                } else {
                    locale.set("zh-CN");
                }
                theme_epoch.update(|v| v.push(()));
                // 界面语言切换后，窗体标题中的模块名跟随当前语言刷新
                set_window_title(title, tab.get());
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
    let theme_btn = nav::theme_toggle(theme_mode, th, theme_epoch);
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

    // ── 内容区：七个页面按导航显隐（站点管理/远程检测弹窗由根层级挂载，遮罩满屏）──
    // SFTP/S3/远程 各页弹窗由根层级挂载：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
    let (sftp_page, sftp_dialogs) = sftp::build_sftp_tab(&state.sftp);
    let (s3_page, s3_dialogs) = s3::build_s3_tab(&state.s3);
    let (remote_page, remote_dialogs) = remote::build_remote_tab(&state.remote);
    // MySQL/PG 库址管理+列选择弹窗挂根层级：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
    let (mysql_page, mysql_col_dialog, mysql_site_mgr) = mysql::build_mysql_tab(&state.mysql);
    let (pg_page, pg_col_dialog, pg_site_mgr) = pg::build_pg_tab(&state.pg);
    // Turso 列设置/库源管理弹窗挂根层级：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
    let (turso_page, turso_col_dialog, turso_db_mgr) = turso::build_turso_tab(&state.turso);
    // TOTP 备份弹窗挂根层级：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
    let (totp_page, totp_backup_dialog) = totp::build_totp_tab(&state.totp);
    let content = Element::stack()
        .height_match()
        .weight(1.0)
        .child(totp_page.visible_when(move || tab.get() == 0))
        .child(password::build_password_tab(&state.password).visible_when(move || tab.get() == 1))
        // Turso 弹窗挂根层级：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
        .child(turso_page.visible_when(move || tab.get() == 2))
        .child(mysql_page.visible_when(move || tab.get() == 7))
        .child(pg_page.visible_when(move || tab.get() == 8))
        .child(sftp_page.visible_when(move || tab.get() == 3))
        .child(remote_page.visible_when(move || tab.get() == 4))
        .child(s3_page.visible_when(move || tab.get() == 6))
        .child(nav::build_about_page().visible_when(move || tab.get() == 5));

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
    let main_ui = Element::stack()
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
        // 站点管理弹窗挂根层级：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
        .child(sftp_dialogs)
        .child(remote_dialogs)
        .child(s3_dialogs)
        .child(mysql_site_mgr)
        .child(pg_site_mgr)
        .child(mysql_col_dialog)
        .child(pg_col_dialog)
        .child(turso_col_dialog)
        .child(turso_db_mgr)
        .child(totp_backup_dialog);
    // 主口令门控（首次设置/换环境解锁）：主界面照常构建渲染，门控以自绘
    // 遮罩浮层叠在其上（背景界面可见不显空白，且未注册模态——窗体 ✕ 直接退出应用）
    main_ui.child(
        super::master::build_gate(&state.master_gate).visible_when(move || gate_show.get()),
    )
}
