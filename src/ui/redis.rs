//! Redis 页 UI：连接管理 + db/键列表 + 键详情 + 命令行
//!
//! 与 PG 页同构但更扁平：Redis 无 schema/表,左侧为「数据库编号 → 键列表」,
//! 右侧为选中键的详情（类型/TTL/值）与任意命令执行结果（同一表格渲染）。

use std::cell::RefCell;
use std::rc::Rc;

use windui::prelude::*;

use super::{icons, table};
use crate::core;
use crate::core::db::TablePage;
use crate::lang;

/// Redis 后台任务消息（独立通道,不与 DbMsg 共用——消息形状不同）
#[derive(Clone)]
pub enum RedisMsg {
    /// 连接结果：db 编号列表
    Connected(Result<Vec<String>, String>),
    /// 键前缀列表（逻辑库）
    Prefixes(Result<Vec<String>, String>),
    /// 键列表页：(键名, 是否还有下一页)
    Keys(Result<(Vec<String>, bool), String>),
    /// 键详情
    Detail(Result<core::redis::KeyEntry, String>),
    /// 命令执行结果（表格行,首行表头）
    CmdDone(Result<Vec<Vec<String>>, String>),
}

/// Redis 页状态：全部 UI 信号 + 后台任务通道发送端
#[derive(Clone)]
pub struct RedisUi {
    /// 站点（多连接）管理：列表 + 下拉选中 + 管理弹窗 + 编辑表单
    pub sites: Signal<Vec<crate::core::store::RedisSite>>,
    pub site_sel: Signal<usize>,
    pub site_mgr_show: Signal<bool>,
    pub site_edit_id: Signal<i64>,
    pub site_name: Signal<String>,
    pub site_url: Signal<String>,
    pub connected: Signal<bool>,
    /// 数据库编号列表（db0..dbN，连接条下拉）
    pub dbs: Signal<Vec<String>>,
    pub db_sel: Signal<usize>,
    /// 当前 db 的键前缀列表（逻辑库分层）
    pub prefixes: Signal<Vec<String>>,
    pub prefix_sel: Signal<String>,
    /// 当前前缀下键列表 + 分页
    pub keys: Signal<Vec<String>>,
    pub key_page: Signal<usize>,
    pub has_more: Signal<bool>,
    /// 当前选中键
    pub selected_key: Signal<String>,
    /// 键详情 / 命令结果（与其他 DB 页同构的表格快照）
    pub table_meta: Signal<Vec<TablePage>>,
    pub table_rows: Signal<Vec<Vec<String>>>,
    /// 命令行面板
    pub show_cmd: Signal<bool>,
    pub cmd_input: Signal<String>,
    pub loading: Signal<bool>,
    tx: Rc<RefCell<Option<Sender<RedisMsg>>>>,
}

impl RedisUi {
    pub fn new() -> Self {
        Self {
            sites: signal(crate::core::store::redis_site_list().unwrap_or_default()),
            site_sel: signal(0usize),
            site_mgr_show: signal(false),
            site_edit_id: signal(0i64),
            site_name: signal(String::new()),
            site_url: signal(String::new()),
            connected: signal(false),
            dbs: signal(Vec::new()),
            db_sel: signal(0usize),
            prefixes: signal(Vec::new()),
            prefix_sel: signal(String::new()),
            keys: signal(Vec::new()),
            key_page: signal(0usize),
            has_more: signal(false),
            selected_key: signal(String::new()),
            table_meta: signal(Vec::new()),
            table_rows: signal(Vec::new()),
            show_cmd: signal(false),
            cmd_input: signal(String::new()),
            loading: signal(false),
            tx: Rc::new(RefCell::new(None)),
        }
    }

    pub fn set_tx(&self, tx: Sender<RedisMsg>) {
        *self.tx.borrow_mut() = Some(tx);
    }

    fn tx(&self) -> Option<Sender<RedisMsg>> {
        self.tx.borrow().clone()
    }

    /// 重新加载站点列表（主口令解锁后调用：启动时未解锁，unprotect 失败会把
    /// `v2:` 密文按旧版明文兜底原样读出，须在解锁后重读一次）
    pub fn reload(&self) {
        self.sites
            .set(crate::core::store::redis_site_list().unwrap_or_default());
    }

    /// 当前连接串：取选中站点（无站点返回空串——连接按钮此时已禁用）
    pub fn selected_source(&self) -> String {
        self.sites
            .get()
            .get(
                self.site_sel
                    .get()
                    .min(self.sites.get().len().saturating_sub(1)),
            )
            .map(|s| s.url.clone())
            .unwrap_or_default()
    }

    /// 消费后台任务消息（channel 回 UI 线程时调用）
    pub fn on_msg(&self, msg: RedisMsg) {
        match msg {
            RedisMsg::Connected(res) => match res {
                Ok(dbs) => {
                    self.loading.set(false);
                    self.connected.set(true);
                    self.db_sel.set(0);
                    self.dbs.set(dbs);
                    self.prefixes.set(Vec::new());
                    self.prefix_sel.set(String::new());
                    self.keys.set(Vec::new());
                    self.key_page.set(0);
                    self.selected_key.set(String::new());
                    self.table_meta.set(Vec::new());
                    self.table_rows.set(Vec::new());
                    super::toast::ok(lang::REDIS_CONNECTED(0));
                }
                Err(e) => {
                    self.loading.set(false);
                    super::toast::err(e);
                }
            },
            RedisMsg::Prefixes(res) => {
                self.loading.set(false);
                match res {
                    Ok(list) => {
                        self.prefixes.set(list);
                        self.prefix_sel.set(String::new());
                        self.keys.set(Vec::new());
                        self.key_page.set(0);
                    }
                    Err(e) => super::toast::err(e),
                }
            }
            RedisMsg::Keys(res) => {
                self.loading.set(false);
                if let Ok((keys, more)) = res {
                    self.keys.set(keys);
                    self.has_more.set(more);
                }
            }
            RedisMsg::Detail(res) => match res {
                Ok(e) => {
                    let page = TablePage {
                        table_name: Some(e.key.clone()),
                        columns: vec!["KEY".into(), "TYPE".into(), "TTL".into(), "VALUE".into()],
                        rows: vec![vec![
                            e.key,
                            e.typ,
                            if e.ttl < 0 {
                                lang::REDIS_TTL_FOREVER()
                            } else {
                                format!("{}s", e.ttl)
                            },
                            e.value,
                        ]],
                        visible: vec![true; 4],
                        ..TablePage::empty()
                    };
                    self.table_meta.set(vec![page.clone()]);
                    self.table_rows.set(page.rows);
                }
                Err(e) => super::toast::err(e),
            },
            RedisMsg::CmdDone(res) => match res {
                Ok(rows) => {
                    let (cols, body) = match rows.split_first() {
                        Some((h, b)) => (h.clone(), b.to_vec()),
                        None => (vec!["value".to_string()], Vec::new()),
                    };
                    let page = TablePage {
                        table_name: None,
                        columns: cols,
                        rows: body,
                        visible: Vec::new(),
                        ..TablePage::empty()
                    };
                    self.table_meta.set(vec![page.clone()]);
                    self.table_rows.set(page.rows);
                }
                Err(e) => super::toast::err(e),
            },
        }
    }
}

impl Default for RedisUi {
    fn default() -> Self {
        Self::new()
    }
}

/// 后台线程跑 core::redis 的阻塞调用,结果经 sink 回 UI（f 返回消息本身）
fn spawn<F>(tx: Option<Sender<RedisMsg>>, f: F)
where
    F: FnOnce() -> RedisMsg + Send + 'static,
{
    let Some(tx) = tx else { return };
    std::thread::spawn(move || {
        let msg = f();
        let _ = tx.send(msg);
    });
}

/// Redis 页 UI 构建（返回 (页面, 连接管理弹窗)：弹窗须挂根层级,遮罩才铺满全窗）
pub fn build_redis_tab(ui: &RedisUi) -> (Element, Element) {
    let ui = ui.clone(); // 拥有所有权：供多个 'static 闭包捕获
    let RedisUi {
        sites,
        site_sel,
        site_mgr_show,
        connected,
        dbs,
        db_sel,
        prefixes,
        prefix_sel,
        keys,
        key_page,
        has_more,
        selected_key,
        table_meta,
        table_rows,
        show_cmd,
        cmd_input,
        loading,
        ..
    } = ui.clone();

    let _tx = ui.tx();

    // ── 连接条：站点下拉 + 管理 + 连接/断开 + 命令行开关 ──
    let conn_bar = Element::row()
        .width_match()
        .spacing(6)
        .cross(Align::Center)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding_xy(10, 8)
        .child(
            Element::dropdown_signal(
                site_sel.map(move |idx: &usize| {
                    let idx = *idx;
                    let list = sites.get();
                    if list.is_empty() {
                        vec![lang::S3_SITE_NONE()]
                    } else {
                        vec![list
                            .get(idx.min(list.len().saturating_sub(1)))
                            .map(|s| s.name.clone())
                            .unwrap_or_default()]
                    }
                }),
                site_sel,
            )
            .width(160),
        )
        .child(
            Element::button(lang::REDIS_CONN_MGR())
                .neutral()
                .icon_content(icons::stateful_icon(icons::SERVER))
                .small()
                .on_click({
                    let (show, id, name, url) =
                        (site_mgr_show, ui.site_edit_id, ui.site_name, ui.site_url);
                    move |_| {
                        id.set(0);
                        name.set("示例连接".to_string());
                        url.set("redis://:pass@127.0.0.1:6379/0".to_string());
                        show.set(true);
                    }
                }),
        )
        .child(
            Element::button(lang::TURSO_CONNECT())
                .neutral()
                .icon_content(icons::stateful_icon(icons::PLUG))
                .small()
                .visible_when(move || !connected.get())
                .enabled_when(move || !sites.get().is_empty() && !loading.get())
                .on_click({
                    let ui_c = ui.clone();
                    move |_| {
                        let src = ui_c.selected_source();
                        loading.set(true);
                        spawn(ui_c.tx(), move || {
                            // 连接后先切到 db0 并枚举 db 编号供下拉
                            let res = core::redis::connect(&src)
                                .and_then(|_| core::redis::list_databases(&src));
                            RedisMsg::Connected(res)
                        });
                    }
                }),
        )
        .child(
            // db 编号下拉：切换后重拉前缀列表
            Element::dropdown_signal(
                db_sel.map(move |idx: &usize| {
                    let idx = *idx;
                    let list = dbs.get();
                    if list.is_empty() {
                        vec![lang::REDIS_DB_NONE()]
                    } else {
                        vec![list
                            .get(idx.min(list.len().saturating_sub(1)))
                            .cloned()
                            .unwrap_or_default()]
                    }
                }),
                db_sel,
            )
            .width(110),
        )
        .child(
            Element::button(lang::TURSO_DISCONNECT())
                .neutral()
                .icon_content(icons::stateful_icon(icons::PLUG))
                .small()
                .visible_when(move || connected.get())
                .on_click(move |_| {
                    core::redis::disconnect();
                    connected.set(false);
                    dbs.set(Vec::new());
                    db_sel.set(0);
                    prefixes.set(Vec::new());
                    prefix_sel.set(String::new());
                    keys.set(Vec::new());
                    key_page.set(0);
                    has_more.set(false);
                    selected_key.set(String::new());
                    table_meta.set(Vec::new());
                    table_rows.set(Vec::new());
                    show_cmd.set(false);
                }),
        )
        .child(
            Element::button(lang::REDIS_CMD_PANEL())
                .neutral()
                .icon_content(icons::stateful_icon(icons::TERMINAL))
                .small()
                .enabled_signal(connected)
                .tooltip(lang::REDIS_CMD_HINT())
                .on_click(move |_| show_cmd.set(!show_cmd.get())),
        );

    // ── 左侧：键前缀（逻辑库）列表 + 键列表 ──
    let ui_db = ui.clone(); // 前缀列表闭包专用（Fn 回调内部还要再克隆）
    let ui_key = ui.clone(); // 键列表闭包专用
    let selected_key_pfx = ui.prefix_sel; // 键列表闭包专用（Fn 回调内读取）
    let prefix_list = Element::col()
        .width_match()
        .spacing(2)
        .child(
            Element::label(lang::REDIS_PREFIX_LIST())
                .font_size(11.0)
                .fg_role(Role::TextMuted)
                .padding_xy(8, 2),
        )
        .child(Element::list_signal(
            prefixes,
            |s: &String| s.clone(),
            move |prefix: String| {
                let (prefix_sel, key_page, selected_key, table_meta, table_rows, loading) = (
                    prefix_sel,
                    key_page,
                    selected_key,
                    table_meta,
                    table_rows,
                    loading,
                );
                let ui_db = ui_db.clone(); // Fn 回调：每次调用克隆一份供内部 'static 闭包
                let is_cur = prefix_sel.get() == prefix;
                Element::row()
                    .width_match()
                    .height(30)
                    .cross(Align::Center)
                    .padding_xy(8, 0)
                    .corner(4.0)
                    .bg_role(if is_cur { Role::Surface } else { Role::Bg })
                    .clickable()
                    .on_click({
                        let prefix = prefix.clone();
                        move |_| {
                            prefix_sel.set(prefix.clone());
                            selected_key.set(String::new());
                            table_meta.set(Vec::new());
                            table_rows.set(Vec::new());
                            key_page.set(0);
                            // 切前缀：按 `prefix:*` 过滤重扫第 0 页（连接串在 UI 线程取好,
                            // 不把含 Rc 的 ui 句柄带进后台线程）
                            let tx = ui_db.tx();
                            let src = ui_db.selected_source();
                            let pfx = prefix.clone();
                            loading.set(true);
                            spawn(tx, move || {
                                RedisMsg::Keys(core::redis::scan_keys(&src, Some(&pfx), 0))
                            });
                        }
                    })
                    .child(
                        Element::label(prefix.clone())
                            .font_size(13.0)
                            .fg_role(if is_cur { Role::Accent } else { Role::Text })
                            .font_weight(if is_cur { 600 } else { 400 }),
                    )
            },
        ));
    let key_list_col = Element::col()
        .width_match()
        .spacing(2)
        .child(
            Element::label(lang::REDIS_KEY_LIST(keys.get().len()))
                .font_size(11.0)
                .fg_role(Role::TextMuted)
                .padding_xy(8, 2),
        )
        .child(Element::list_signal(
            keys,
            |k: &String| k.clone(),
            move |key: String| {
                // 已选前缀时键名都带 `prefix:`，列表里去掉该前缀显示，节省横向空间
                let pfx = selected_key_pfx.get();
                let shown = if pfx.is_empty() {
                    key.clone()
                } else {
                    key.strip_prefix(&format!("{}:", pfx))
                        .map(String::from)
                        .unwrap_or(key.clone())
                };
                let ui_key = ui_key.clone(); // Fn 回调：每次调用克隆一份
                let is_cur = selected_key.get() == key;
                Element::row()
                    .width_match()
                    .height(30)
                    .cross(Align::Center)
                    .padding_xy(8, 0)
                    .corner(4.0)
                    .bg_role(if is_cur { Role::Surface } else { Role::Bg })
                    .clickable()
                    .on_click({
                        let key = key.clone();
                        move |_| {
                            selected_key.set(key.clone());
                            let src = ui_key.selected_source();
                            let k = key.clone();
                            let tx = ui_key.tx();
                            spawn(tx, move || {
                                RedisMsg::Detail(core::redis::read_key(&src, &k))
                            });
                        }
                    })
                    .child(
                        Element::label(shown)
                            .font_size(13.0)
                            .fg_role(if is_cur { Role::Accent } else { Role::Text })
                            .weight(1.0)
                            .max_lines(1)
                            .truncate(Truncate::End),
                    )
            },
        ));

    // 键分页条：上一页 / 下一页
    let page_bar = Element::row()
        .width_match()
        .spacing(6)
        .cross(Align::Center)
        .child(
            Element::button(lang::DT_PREV())
                .small()
                .neutral()
                .outline_soft()
                .enabled_when(move || key_page.get() > 0 && !loading.get())
                .on_click({
                    let ui_c = ui.clone();
                    move |_| {
                        let p = ui_c.key_page.get().saturating_sub(1);
                        ui_c.key_page.set(p);
                        let src = ui_c.selected_source();
                        let pfx = ui_c.prefix_sel.get();
                        let pfx = (!pfx.is_empty()).then_some(pfx);
                        loading.set(true);
                        spawn(ui_c.tx(), move || {
                            RedisMsg::Keys(core::redis::scan_keys(&src, pfx.as_deref(), p))
                        });
                    }
                }),
        )
        .child(
            Element::button(lang::DT_NEXT())
                .small()
                .neutral()
                .outline_soft()
                .enabled_when(move || has_more.get() && !loading.get())
                .on_click({
                    let ui_c = ui.clone();
                    move |_| {
                        let p = ui_c.key_page.get() + 1;
                        ui_c.key_page.set(p);
                        let src = ui_c.selected_source();
                        let pfx = ui_c.prefix_sel.get();
                        let pfx = (!pfx.is_empty()).then_some(pfx);
                        loading.set(true);
                        spawn(ui_c.tx(), move || {
                            RedisMsg::Keys(core::redis::scan_keys(&src, pfx.as_deref(), p))
                        });
                    }
                }),
        )
        .child(
            Element::button(lang::TURSO_REFRESH())
                .small()
                .neutral()
                .outline_soft()
                .icon_content(icons::stateful_icon(icons::REFRESH))
                .on_click({
                    let ui_c = ui.clone();
                    move |_| {
                        core::redis::invalidate();
                        let p = ui_c.key_page.get();
                        let src = ui_c.selected_source();
                        let pfx = ui_c.prefix_sel.get();
                        let pfx = (!pfx.is_empty()).then_some(pfx);
                        loading.set(true);
                        spawn(ui_c.tx(), move || {
                            RedisMsg::Keys(core::redis::scan_keys(&src, pfx.as_deref(), p))
                        });
                    }
                }),
        );

    let left_col = Element::col()
        .width(240)
        .height_match()
        .spacing(8)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding_xy(10, 10)
        .child(prefix_list)
        // 卡片容器收住列表：weight 占剩余高度，列表多时内部滚动不撑出
        .child(Element::scroll().weight(1.0).child(key_list_col))
        .child(page_bar);

    // ── 右侧：键详情表格 + 命令行 ──
    let data_table = Element::col()
        .width_match()
        .height_match()
        .spacing(8)
        .child(table::render_data_table(table_meta, table_rows));
    let right_col = Element::col()
        .width_match()
        .height_match()
        .spacing(8)
        .child(
            // weight 占剩余高度：表格内容超长时内部滚动，不把页面撑出窗体
            Element::scroll().weight(1.0).child(data_table),
        )
        .child(
            Element::col()
                .width_match()
                .spacing(6)
                .visible_when(move || show_cmd.get())
                .child(
                    Element::row()
                        .width_match()
                        .spacing(6)
                        .cross(Align::Center)
                        .child(Element::text_input(cmd_input, lang::REDIS_CMD_HINT()).width_match())
                        .child(Element::button(lang::SFTP_CMD_EXEC()).small().on_click({
                            let ui_c = ui.clone();
                            move |_| {
                                let cmd = ui_c.cmd_input.get().trim().to_string();
                                if cmd.is_empty() {
                                    return;
                                }
                                let src = ui_c.selected_source();
                                spawn(ui_c.tx(), move || {
                                    RedisMsg::CmdDone(core::redis::execute_command(&src, &cmd))
                                });
                            }
                        })),
                ),
        );

    let page = Element::col()
        .width_match()
        .height_match()
        .spacing(8)
        .padding_xy(12, 12)
        .child(conn_bar)
        .child(
            Element::row()
                .width_match()
                // col 内 height_match 不参与剩余高度分配，会按内容撑出窗体底部
                // （PG 页同款注释先例）：必须用 weight 占满剩余高度
                .weight(1.0)
                .spacing(8)
                .child(left_col)
                .child(right_col),
        );

    // db 编号下拉切换监听（Dropdown 不走 on_click）：切 db 后清空前缀/键并
    // 重拉该 db 的前缀列表。连接成功也会写 db_sel（重置为 0），由此统一触发。
    let db_watch = {
        let ui_w = ui.clone();
        super::sig_watch::sig_watch(
            move || ui_w.db_sel.version(),
            false,
            move |_| {
                let idx = ui_w.db_sel.get();
                ui_w.prefixes.set(Vec::new());
                ui_w.prefix_sel.set(String::new());
                ui_w.keys.set(Vec::new());
                ui_w.key_page.set(0);
                ui_w.selected_key.set(String::new());
                ui_w.table_meta.set(Vec::new());
                ui_w.table_rows.set(Vec::new());
                // 断开时也会写 db_sel（复位为 0），此时不应向已断开的连接拉前缀
                if !ui_w.connected.get() {
                    return;
                }
                // 连接串在 UI 线程换好 db 号再进后台线程
                let src = core::redis::url_for_db(&ui_w.selected_source(), idx as i64);
                ui_w.loading.set(true);
                let tx = ui_w.tx();
                spawn(tx, move || {
                    RedisMsg::Prefixes(core::redis::list_prefixes(&src))
                });
            },
        )
        .reactive() // 必须：零尺寸叶子须注册进框架才参与版本跟踪（sftp bmk_watch 先例）
    };

    (
        page,
        // 容器必须 stack（fill）：col 会让首个 fill 子吃掉全部高度，弹窗拿不到
        // 尺寸、遮罩不铺满全窗（PG 页同款注释先例）
        Element::stack()
            .fill()
            .child(build_redis_site_mgr(&ui))
            .child(db_watch),
    )
}

/// Redis 连接管理弹窗：单层双栏（左站点列表 + 右表单），保存/删除同层完成
fn build_redis_site_mgr(ui: &RedisUi) -> Element {
    let mgr_close = ui.site_mgr_show;
    let mgr_form_ui = ui.clone();
    let mgr_list = Element::list_signal(
        ui.sites,
        |s: &crate::core::store::RedisSite| s.id,
        move |s: crate::core::store::RedisSite| {
            let (sel, sites) = (mgr_form_ui.site_sel, mgr_form_ui.sites);
            let form_ui = mgr_form_ui.clone();
            let row_id = s.id;
            Element::row()
                .width_match()
                .height(30)
                .cross(Align::Center)
                .spacing(8)
                .padding_xy(8, 0)
                .corner(4.0)
                .clickable()
                .on_click(move |_| {
                    let list = sites.get();
                    if let Some(idx) = list.iter().position(|x| x.id == row_id) {
                        sel.set(idx);
                    }
                    if let Some(x) = list.iter().find(|x| x.id == row_id) {
                        form_ui.site_edit_id.set(x.id);
                        form_ui.site_name.set(x.name.clone());
                        form_ui.site_url.set(x.url.clone());
                    }
                })
                .child(
                    Element::label(s.name.clone())
                        .font_size(13.0)
                        .fg_role(Role::Text)
                        .weight(1.0)
                        .max_lines(1)
                        .truncate(Truncate::End),
                )
                .child(
                    Element::label(core::redis::display_of(&s.url))
                        .font_size(11.0)
                        .fg_role(Role::TextMuted)
                        .max_lines(1)
                        .truncate(Truncate::End),
                )
        },
    );
    let form_save = Element::button(lang::SFTP_SITE_SAVE()).small().on_click({
        let (id, _show) = (ui.site_edit_id, ui.site_mgr_show);
        let (name, url) = (ui.site_name, ui.site_url);
        let sites = ui.sites;
        move |_| {
            let n = name.get().trim().to_string();
            if n.is_empty() {
                return;
            }
            let site = crate::core::store::RedisSite {
                id: id.get(),
                name: n,
                url: url.get().trim().to_string(),
            };
            if crate::core::store::redis_site_upsert(&site).is_ok() {
                sites.set(crate::core::store::redis_site_list().unwrap_or_default());
            }
            // 保存后不关弹窗：继续编辑/新建（关闭仅靠 X / Esc / 点遮罩）
        }
    });
    let form_del = Element::button(lang::S3_SITE_DEL())
        .small()
        .neutral()
        .danger()
        .on_click({
            let id = ui.site_edit_id;
            let sites = ui.sites;
            move |_| {
                let idv = id.get();
                if idv > 0 && crate::core::store::redis_site_del(idv).is_ok() {
                    sites.set(crate::core::store::redis_site_list().unwrap_or_default());
                    // 删除后也不关窗，表单重置为新建态
                    id.set(0);
                }
            }
        });
    let form_new = Element::button(lang::S3_SITE_NEW())
        .small()
        .neutral()
        .on_click({
            let ui = ui.clone();
            move |_| {
                ui.site_edit_id.set(0);
                // 新建态默认填示例数据，可直接改后保存
                ui.site_name.set("示例连接".to_string());
                ui.site_url
                    .set("redis://:pass@127.0.0.1:6379/0".to_string());
            }
        });
    let mgr_body = Element::row()
        .width_match()
        .height(320)
        .spacing(12)
        .child(crate::widgets::mgr_list_col(
            mgr_list,
            ui.sites.map(|s| s.is_empty()),
        ))
        .child(crate::widgets::mgr_form_col(
            Element::col()
                .width_match()
                .height_match()
                .spacing(8)
                .child(
                    Element::text_input(ui.site_name, lang::SFTP_SITE_NAME())
                        .autofocus()
                        .width_match(),
                )
                .child(Element::text_input(ui.site_url, lang::REDIS_CONN_HINT()).width_match())
                .child(Element::flex_spacer()),
        ));
    crate::widgets::mgr_dialog(
        ui.site_mgr_show,
        lang::REDIS_SITE_TITLE(),
        620,
        move |_| mgr_close.set(false),
        mgr_body,
        form_new,
        form_del,
        form_save,
    )
}

#[cfg(test)]
mod tests {
    //! RedisUi 状态机：on_msg 每条消息分支 + loading 生命周期回归。
    //! toast 是全局 GUI 状态，测试进程无窗口时只写内部缓冲，不影响断言。
    use super::*;

    /// Connected(Ok)：置 connected、db 列表、清空键区，且必须复位 loading
    /// （回归：此前成功分支漏复位 → 连接按钮点一次即永久禁用）
    #[test]
    fn on_msg_connected_ok_resets_loading_and_state() {
        let ui = RedisUi::new();
        ui.loading.set(true);
        ui.on_msg(RedisMsg::Connected(Ok(vec!["db0".into(), "db1".into()])));
        assert!(ui.connected.get());
        assert_eq!(ui.dbs.get(), vec!["db0", "db1"]);
        assert_eq!(ui.db_sel.get(), 0);
        assert!(!ui.loading.get(), "成功后必须复位 loading");
        // 键区被清空
        assert!(ui.keys.get().is_empty());
        assert!(ui.prefixes.get().is_empty());
        assert!(ui.prefix_sel.get().is_empty());
        assert!(ui.table_meta.get().is_empty());
    }

    /// Connected(Err)：保持未连接、必须复位 loading
    /// （回归：此前失败分支漏复位 → 连接失败一次按钮就点不动了）
    #[test]
    fn on_msg_connected_err_resets_loading() {
        let ui = RedisUi::new();
        ui.loading.set(true);
        ui.on_msg(RedisMsg::Connected(Err("boom".into())));
        assert!(!ui.connected.get(), "失败不得置 connected");
        assert!(!ui.loading.get(), "失败后必须复位 loading");
        assert!(ui.dbs.get().is_empty());
    }

    /// Prefixes(Ok)：填充前缀、清空键区、复位 loading
    #[test]
    fn on_msg_prefixes_ok() {
        let ui = RedisUi::new();
        ui.loading.set(true);
        ui.keys.set(vec!["stale".into()]);
        ui.key_page.set(3);
        ui.on_msg(RedisMsg::Prefixes(Ok(vec![
            "hltqh".into(),
            "sys_dict".into(),
        ])));
        assert!(!ui.loading.get());
        assert_eq!(ui.prefixes.get(), vec!["hltqh", "sys_dict"]);
        assert!(ui.prefix_sel.get().is_empty(), "新列表不应预选前缀");
        assert!(ui.keys.get().is_empty(), "切前缀源须清旧键");
        assert_eq!(ui.key_page.get(), 0);
    }

    /// Prefixes(Err)：仅报错，不动既有前缀/键区
    #[test]
    fn on_msg_prefixes_err_keeps_state() {
        let ui = RedisUi::new();
        ui.loading.set(true);
        ui.prefixes.set(vec!["keep".into()]);
        ui.on_msg(RedisMsg::Prefixes(Err("net down".into())));
        assert!(!ui.loading.get());
        assert_eq!(ui.prefixes.get(), vec!["keep"], "失败保留旧列表");
    }

    /// Keys(Ok)：写键列表与 has_more、复位 loading
    #[test]
    fn on_msg_keys_ok() {
        let ui = RedisUi::new();
        ui.loading.set(true);
        ui.on_msg(RedisMsg::Keys(Ok((
            vec!["hltqh:1".into(), "hltqh:2".into()],
            true,
        ))));
        assert!(!ui.loading.get());
        assert_eq!(ui.keys.get(), vec!["hltqh:1", "hltqh:2"]);
        assert!(ui.has_more.get());
    }

    /// Keys(Err)：复位 loading、键列表保持旧值（不清空,避免闪烁）
    #[test]
    fn on_msg_keys_err_resets_loading_keeps_keys() {
        let ui = RedisUi::new();
        ui.loading.set(true);
        ui.keys.set(vec!["old".into()]);
        ui.on_msg(RedisMsg::Keys(Err("timeout".into())));
        assert!(!ui.loading.get());
        assert_eq!(ui.keys.get(), vec!["old"]);
    }

    /// Detail(Ok)：拼 KEY/TYPE/TTL/VALUE 四列表格；TTL<0 显示「永久」
    #[test]
    fn on_msg_detail_ok_builds_table() {
        let ui = RedisUi::new();
        ui.on_msg(RedisMsg::Detail(Ok(core::redis::KeyEntry {
            key: "hltqh:user:1".into(),
            typ: "string".into(),
            ttl: -1,
            value: "alice".into(),
        })));
        let meta = ui.table_meta.get();
        assert_eq!(meta.len(), 1);
        let m = &meta[0];
        assert_eq!(m.table_name.as_deref(), Some("hltqh:user:1"));
        assert_eq!(m.columns, vec!["KEY", "TYPE", "TTL", "VALUE"]);
        assert_eq!(
            m.rows,
            vec![vec![
                "hltqh:user:1".into(),
                "string".into(),
                lang::REDIS_TTL_FOREVER(),
                "alice".into(),
            ]]
        );
        // 行数据同步写 table_rows（表格控件读它）
        assert_eq!(ui.table_rows.get().len(), 1);
    }

    /// Detail(Ok)：TTL>=0 显示秒数
    #[test]
    fn on_msg_detail_ttl_seconds() {
        let ui = RedisUi::new();
        ui.on_msg(RedisMsg::Detail(Ok(core::redis::KeyEntry {
            key: "k".into(),
            typ: "string".into(),
            ttl: 120,
            value: "v".into(),
        })));
        assert_eq!(ui.table_meta.get()[0].rows[0][2], "120s");
    }

    /// Detail(Err)：不动表格
    #[test]
    fn on_msg_detail_err_keeps_table() {
        let ui = RedisUi::new();
        ui.table_meta.set(vec![TablePage::empty()]);
        let before = ui.table_meta.get().len();
        ui.on_msg(RedisMsg::Detail(Err("missing".into())));
        assert_eq!(ui.table_meta.get().len(), before);
    }

    /// CmdDone(Ok)：首行作表头，其余为数据行
    #[test]
    fn on_msg_cmd_done_ok_splits_header() {
        let ui = RedisUi::new();
        ui.on_msg(RedisMsg::CmdDone(Ok(vec![
            vec!["name".into(), "age".into()],
            vec!["alice".into(), "30".into()],
            vec!["bob".into(), "25".into()],
        ])));
        let m = &ui.table_meta.get()[0];
        assert_eq!(m.columns, vec!["name", "age"]);
        assert_eq!(m.rows.len(), 2);
        assert_eq!(ui.table_rows.get(), m.rows);
    }

    /// CmdDone(Ok)：空结果兜底单列 value 表头
    #[test]
    fn on_msg_cmd_done_empty_fallback() {
        let ui = RedisUi::new();
        ui.on_msg(RedisMsg::CmdDone(Ok(vec![])));
        let m = &ui.table_meta.get()[0];
        assert_eq!(m.columns, vec!["value"]);
        assert!(m.rows.is_empty());
    }

    /// selected_source：越界索引收敛到最后一项；无站点返回空串
    #[test]
    fn selected_source_clamps() {
        let ui = RedisUi::new();
        // new() 会从 store.db 读真实站点列表，测试须显式清空以构造"无站点"前提
        ui.sites.set(Vec::new());
        assert_eq!(ui.selected_source(), "", "无站点返回空串");
        ui.sites.set(vec![
            crate::core::store::RedisSite {
                id: 1,
                name: "a".into(),
                url: "redis://h1:6379/0".into(),
            },
            crate::core::store::RedisSite {
                id: 2,
                name: "b".into(),
                url: "redis://h2:6379/0".into(),
            },
        ]);
        ui.site_sel.set(99);
        assert_eq!(
            ui.selected_source(),
            "redis://h2:6379/0",
            "越界索引收敛到最后一项"
        );
    }
}
