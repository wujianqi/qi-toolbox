//! MySQL 页 UI：连接表单 + SQL 面板 + 库→表分级列表 + 数据表格
//!
//! 页面状态封装在 [`MySqlUi`]，后台消息统一由 [`MySqlUi::on_db_msg`] 消费。
//! 表格渲染复用 turso 页的 `table::render_data_table`（同一 `TablePage` 快照）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windui::prelude::*;

use super::{icons, sink, sql, table};
use crate::core;
use crate::core::db::DbSource;
use crate::core::mysql::MySqlSource;
use crate::lang;
use crate::widgets::syntax_input::LexerKind;

/// MySQL 页状态：全部 UI 信号 + 后台任务通道发送端
#[derive(Clone)]
pub struct MySqlUi {
    pub host: Signal<String>,
    pub port: Signal<String>,
    pub user: Signal<String>,
    pub pass: Signal<String>,
    /// 站点（多连接）管理：列表 + 下拉选中 + 管理弹窗 + 编辑表单
    pub sites: Signal<Vec<crate::core::store::MySqlSite>>,
    pub site_sel: Signal<usize>,
    pub site_mgr_show: Signal<bool>,
    pub site_edit_id: Signal<i64>,
    pub site_name: Signal<String>,
    pub site_host: Signal<String>,
    pub site_port: Signal<String>,
    pub site_user: Signal<String>,
    pub site_pass: Signal<String>,
    /// 分级列表：库 → 表
    pub groups: Signal<Vec<(String, Vec<String>)>>,
    /// 当前选中 "库.表"（高亮）
    pub selected_key: Signal<String>,
    /// 分级列表展开的库
    pub expanded: Signal<Vec<String>>,
    pub connected: Signal<bool>,
    pub table_meta: Signal<Vec<core::db::TablePage>>,
    pub table_rows: Signal<Vec<Vec<String>>>,
    pub table_title: Signal<String>,
    pub status: Signal<String>,
    pub error: Signal<String>,
    pub show_sql: Signal<bool>,
    /// 列设置弹窗显隐
    pub show_cols: Signal<bool>,
    pub sql_query: Signal<String>,
    pub sql_status: Signal<String>,
    pub table_loading: Signal<bool>,
    /// 排队的表加载请求（节流防连点）：(库, 表, 偏移)
    pub pending: Signal<Option<(String, String, usize)>>,
    pub export_progress: Signal<Vec<(usize, usize)>>,
    tx: Rc<RefCell<Option<Sender<core::db::DbMsg>>>>,
}

impl MySqlUi {
    pub fn new() -> Self {
        Self {
            host: signal(String::new()),
            port: signal(String::from("3306")),
            user: signal(String::from("root")),
            pass: signal(String::new()),
            sites: signal(crate::core::store::mysql_site_list().unwrap_or_default()),
            site_sel: signal(0usize),
            site_mgr_show: signal(false),
            site_edit_id: signal(0i64),
            site_name: signal(String::new()),
            site_host: signal(String::new()),
            site_port: signal(String::from("3306")),
            site_user: signal(String::new()),
            site_pass: signal(String::new()),
            groups: signal(Vec::new()),
            selected_key: signal(String::new()),
            expanded: signal(Vec::new()),
            connected: signal(false),
            table_meta: signal(Vec::new()),
            table_rows: signal(Vec::new()),
            table_title: signal(lang::DT_TABLE_LIST(0)),
            status: signal(String::new()),
            error: signal(String::new()),
            show_sql: signal(false),
            show_cols: signal(false),
            sql_query: signal(String::new()),
            sql_status: signal(String::new()),
            table_loading: signal(false),
            pending: signal(None),
            export_progress: signal(Vec::new()),
            tx: Rc::new(RefCell::new(None)),
        }
    }

    pub fn set_tx(&self, tx: Sender<core::db::DbMsg>) {
        *self.tx.borrow_mut() = Some(tx);
    }

    pub fn tx(&self) -> Sender<core::db::DbMsg> {
        self.tx
            .borrow()
            .as_ref()
            .expect("mysql tx 已在 run() 中回填")
            .clone()
    }

    /// 当前连接参数：取选中库址（无库址返回空配置——连接按钮此时已禁用）
    pub fn selected_source(&self) -> MySqlSource {
        self.sites
            .get()
            .get(
                self.site_sel
                    .get()
                    .min(self.sites.get().len().saturating_sub(1)),
            )
            .map(|s| MySqlSource {
                host: s.host.clone(),
                port: s.port.to_string(),
                user: s.user.clone(),
                pass: s.pass.clone(),
            })
            .unwrap_or_default()
    }

    pub fn make_db_source(&self) -> DbSource {
        DbSource::MySql(self.selected_source())
    }

    /// 消费后台数据库任务消息（`App::channel` 回 UI 线程时调用；共享骨架分发）
    pub fn on_db_msg(&self, msg: core::db::DbMsg) {
        let signals = super::db_page::DbPageSignals {
            groups: self.groups,
            connected: self.connected,
            table_meta: self.table_meta,
            table_rows: self.table_rows,
            table_title: self.table_title,
            status: self.status,
            error: self.error,
            sql_status: self.sql_status,
            table_loading: self.table_loading,
            pending: self.pending,
            export_progress: self.export_progress,
            tx: self.tx(),
        };
        let hooks = super::db_page::DbPageHooks {
            on_connected_groups: Box::new(|res, s| match res {
                Ok((groups, disp)) => {
                    core::mysql::invalidate();
                    let count = groups.len();
                    let table_count: usize = groups.iter().map(|(_, t)| t.len()).sum();
                    s.groups.set(groups);
                    s.connected.set(true);
                    s.table_meta.set(Vec::new());
                    s.table_rows.set(Vec::new());
                    s.table_title.set(lang::DT_TABLE_LIST(table_count));
                    s.error.set(String::new());
                    s.status.set(lang::MYSQL_CONNECTED(count));
                    let _ = disp;
                }
                Err(e) => s.error.set(e),
            }),
            make_db_source: {
                let ui = self.clone();
                Box::new(move || ui.make_db_source())
            },
            export_done_text: Box::new(|path| lang::MYSQL_EXPORT_DONE(path)),
        };
        if !super::db_page::on_db_msg_shared(msg, &signals, &hooks) {
            // 未消费的消息（其它页通道串扰等）静默丢弃
        }
    }
}

impl Default for MySqlUi {
    fn default() -> Self {
        Self::new()
    }
}

/// MySQL 页 UI 构建（返回 (页面, 库址管理弹窗)：弹窗须挂根层级，遮罩才铺满全窗）
pub fn build_mysql_tab(ui: &MySqlUi) -> (Element, Element, Element) {
    let ui = ui.clone(); // 拥有所有权：供多个 'static 闭包捕获
    let MySqlUi {
        host: _,
        port: _,
        user: _,
        pass: _,
        sites,
        site_sel,
        site_mgr_show,
        groups,
        selected_key,
        expanded,
        connected,
        table_meta,
        table_rows,
        table_title,
        status,
        error,
        show_sql,
        sql_query,
        sql_status,
        table_loading,
        pending,
        export_progress,
        ..
    } = ui.clone();

    let tx = ui.tx();

    // ── 连接条（单行紧凑条）：库址下拉 + 管理 + 连接/断开 + SQL 开关（表单全在弹层）──
    let conn_bar = Element::row()
        .width_match()
        .spacing(6)
        .cross(Align::Center)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding_xy(10, 8)
        .child(
            // 库址下拉：连接参数直接取选中库址（管理弹窗内增删改）
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
            Element::button(lang::MYSQL_CONN_MGR())
                .neutral()
                .icon_content(icons::stateful_icon(icons::SERVER, Some(16)))
                .small()
                .on_click(move |_| site_mgr_show.set(true)),
        )
        .child(
            // 连接（未连接时显示）：参数取自选中库址，无库址则禁用
            Element::button(lang::TURSO_CONNECT())
                .neutral()
                .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
                .small()
                .visible_when(move || !connected.get())
                .enabled_when(move || !sites.get().is_empty())
                .on_click({
                    let tx_c = tx.clone();
                    let ui_c = ui.clone();
                    move |_| {
                        let src = ui_c.selected_source();
                        core::db::spawn_connect_grouped(sink(tx_c.clone()), DbSource::MySql(src));
                    }
                }),
        )
        .child(
            // 断开（已连接时显示）
            Element::button(lang::TURSO_DISCONNECT())
                .neutral()
                .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
                .small()
                .visible_when(move || connected.get())
                .on_click(move |_| {
                    core::mysql::disconnect();
                    connected.set(false);
                    groups.set(Vec::new());
                    selected_key.set(String::new());
                    expanded.set(Vec::new());
                    table_meta.set(Vec::new());
                    table_rows.set(Vec::new());
                    table_title.set(lang::DT_TABLE_LIST(0));
                    show_sql.set(false);
                    status.set(String::new());
                    error.set(String::new());
                }),
        )
        .child(
            // SQL 面板开关
            Element::button(lang::TURSO_OPEN_SQL())
                .neutral()
                .icon_content(icons::stateful_icon(icons::TERMINAL, Some(16)))
                .small()
                .enabled_signal(connected)
                .on_click(move |_| show_sql.set(!show_sql.get())),
        )
        .child({
            // 列设置开关：弹出列勾选弹窗（连接且选中表后可用）
            let selected_key = ui.selected_key;
            let show_cols = ui.show_cols;
            Element::button(lang::DT_COLS())
                .neutral()
                .icon_content(icons::stateful_icon(icons::TABLE_ICON, Some(16)))
                .small()
                .enabled_when(move || connected.get() && !selected_key.get().is_empty())
                .on_click(move |_| show_cols.set(!show_cols.get()))
        })
        .child(Element::flex_spacer())
        .child(
            Element::label_signal(error)
                .font_size(11.0)
                .fg_role(Role::Danger),
        )
        .child(
            Element::label_signal(status)
                .font_size(11.0)
                .fg_role(Role::TextMuted),
        );

    // ── 左侧分级列表：点击表 → 节流加载 ──
    let table_list = table::render_grouped_table_list(groups, selected_key, expanded, {
        let tx_c = tx.clone();
        let ui2 = ui.clone();
        move |g: String, t: String| {
            let key = format!("{}.{}", g, t);
            selected_key.set(key.clone());
            if table_loading.get() {
                pending.set(Some((g, t, 0)));
                return;
            }
            table_loading.set(true);
            core::db::spawn_load_table_grouped(sink(tx_c.clone()), ui2.make_db_source(), g, t, 0);
        }
    });

    // ── 右侧数据表格 + 分页条 ──
    let data_table = table::render_data_table(table_meta, table_rows);
    let page_bar_tx = tx.clone();
    let ui_pb = ui.clone(); // page_bar 闭包独占一份，内部回调再各自克隆
    let page_bar = Element::host_signal(table_meta, move |t: core::db::TablePage| {
        let ui = ui_pb.clone(); // host_signal 闭包是 Fn：每次重建克隆一份供内部回调
        if t.selected_row.is_some() {
            return Element::leaf();
        }
        let Some(name) = t.table_name.clone() else {
            return Element::leaf();
        };
        // name = "库.表"；拆回 (库, 表) 供翻页/导出用（库表分隔必是最后一个点
        // ——MySQL 库名可含点，表名亦可，但 spawn 存的是 pair 拆装，显示侧尽力拆）
        let (g, tb) = split_key(&name);
        let total_pages = if t.row_count == 0 {
            1
        } else {
            t.row_count.div_ceil(core::mysql::PAGE_SIZE)
        };
        let cur = t.page_offset / core::mysql::PAGE_SIZE;

        let go = std::rc::Rc::new({
            let tx = page_bar_tx.clone();
            let ui2 = ui.clone();
            let loading = table_loading;
            let pend = pending;
            let (g, tb) = (g.clone(), tb.clone());
            move |offset: usize| {
                if loading.get() {
                    pend.set(Some((g.clone(), tb.clone(), offset)));
                    return;
                }
                loading.set(true);
                core::db::spawn_load_table_grouped(
                    sink(tx.clone()),
                    ui2.make_db_source(),
                    g.clone(),
                    tb.clone(),
                    offset,
                );
            }
        });
        let (_eg, _et) = (g.clone(), tb.clone());

        let prev = Element::button(lang::DT_PREV())
            .small()
            .neutral()
            .outline_soft()
            .enabled_when(move || cur > 0)
            .on_click({
                let go = go.clone();
                move |_| go(cur.saturating_sub(1) * core::mysql::PAGE_SIZE)
            });
        let next = Element::button(lang::DT_NEXT())
            .small()
            .neutral()
            .outline_soft()
            .enabled_when(move || cur + 1 < total_pages)
            .on_click({
                let go = go.clone();
                move |_| go((cur + 1) * core::mysql::PAGE_SIZE)
            });

        // 导出全表 CSV
        let export_tx = page_bar_tx.clone();
        let ui3 = ui.clone();
        let (eg, et) = (g.clone(), tb.clone());
        let export = Element::button(lang::DT_EXPORT())
            .small()
            .neutral()
            .outline_soft()
            .enabled_when(move || !table_loading.get() && export_progress.get().is_empty())
            .on_click(move |ctx| {
                let tx_c = export_tx.clone();
                let ui_c = ui3.clone();
                let (g_c, t_c) = (eg.clone(), et.clone());
                ctx.request_save_file(
                    PickDialog::new()
                        .title(lang::DT_EXPORT_TITLE())
                        .filter("CSV", &["csv"])
                        .file_name(format!("{}.csv", t_c)),
                    move |path: Option<PathBuf>| {
                        if let Some(dest) = path {
                            let dest = if dest
                                .extension()
                                .map(|e| e.eq_ignore_ascii_case("csv"))
                                .unwrap_or(false)
                            {
                                dest
                            } else {
                                dest.with_extension("csv")
                            };
                            core::db::spawn_export_csv_grouped(
                                sink(tx_c),
                                ui_c.make_db_source(),
                                g_c,
                                t_c,
                                dest,
                            );
                        }
                    },
                );
            });

        let exporting = export_progress.map(|v| !v.is_empty());
        let export_frac = export_progress.map(|v| {
            v.first()
                .map(|(d, t)| if *t == 0 { 0.0 } else { *d as f32 / *t as f32 })
                .unwrap_or(0.0)
        });
        let export_label = export_progress.map(|v| match v.first() {
            Some((d, t)) => lang::TURSO_EXPORT_STARTED(*d, *t),
            None => String::new(),
        });
        let progress_row = Element::row()
            .spacing(6)
            .cross(Align::Center)
            .visible_when(move || exporting.get())
            .child(Element::progress(export_frac).width(120).height(6))
            .child(
                Element::label_signal(export_label)
                    .font_size(11.0)
                    .fg_role(Role::TextMuted),
            );

        Element::row()
            .width_match()
            .spacing(8)
            .cross(Align::Center)
            .child(prev)
            .child(
                Element::label(lang::DT_PAGE(cur + 1, total_pages))
                    .font_size(12.0)
                    .fg_role(Role::TextMuted),
            )
            .child(next)
            .child(export)
            .child(progress_row)
    });

    let page = Element::col()
        .padding(12)
        .spacing(8)
        .child(conn_bar)
        .child(
            Element::row()
                .fill()
                .weight(1.0)
                .spacing(8)
                .child(
                    Element::col()
                        .width(210)
                        .spacing(4)
                        .child(
                            Element::row()
                                .width_match()
                                .height(26)
                                .cross(Align::Center)
                                .spacing(6)
                                .padding_xy(4, 0)
                                .child(
                                    Element::image_content(
                                        ImageContent::from_svg_bytes(icons::TABLE_ICON, Some(14))
                                            .tint(
                                                Role::TextMuted.resolve(&windui::theme::current()),
                                            ),
                                    )
                                    .align(Align::Center),
                                )
                                .child(
                                    Element::label_signal(table_title)
                                        .font_size(12.0)
                                        .font_weight(600)
                                        .fg_role(Role::TextMuted)
                                        .max_lines(1)
                                        .weight(1.0),
                                ),
                        )
                        .child(
                            Element::scroll()
                                .width_match()
                                .height_match()
                                .child(table_list.weight(1.0)),
                        ),
                )
                .child(
                    Element::leaf()
                        .width(1)
                        .height_match()
                        .bg_role(Role::Divider),
                )
                .child(
                    Element::col()
                        .weight(1.0)
                        .child(data_table.weight(1.0))
                        .child(page_bar),
                ),
        )
        .child(
            sql::render_sql_panel(
                sql_query,
                sql_status,
                connected,
                {
                    let ui2 = ui.clone();
                    move || ui2.make_db_source()
                },
                tx,
                LexerKind::SqlMySql,
                "mysql",
            )
            .visible_when(move || show_sql.get()),
        );

    // 列勾选弹窗：列出所有列，点击行切换该列显示/隐藏（meta 驱动同步刷新）
    let col_body = Element::host_signal(table_meta, move |t: core::db::TablePage| {
        if t.columns.is_empty() {
            return Element::leaf();
        }
        let mut rows = Element::col().spacing(2);
        for (i, col_name) in t.columns.iter().enumerate() {
            let shown = t.visible.get(i).copied().unwrap_or(true);
            let meta = table_meta; // Signal 是 Copy，直接捕获
            rows = rows.child(
                Element::row()
                    .width_match()
                    .height(26)
                    .cross(Align::Center)
                    .spacing(8)
                    .padding_xy(8, 0)
                    .clickable()
                    .on_click(move |_| {
                        meta.update(|v| {
                            if let Some(p) = v.first_mut() {
                                if let Some(b) = p.visible.get_mut(i) {
                                    *b = !*b;
                                }
                            }
                        });
                    })
                    .child(
                        Element::label(if shown { "\u{2713}" } else { "\u{25CB}" })
                            .font_size(13.0)
                            .fg_role(if shown { Role::Accent } else { Role::TextMuted })
                            .width(24),
                    )
                    .child(
                        Element::label(col_name)
                            .font_size(12.0)
                            .max_lines(1)
                            .fg_role(if shown { Role::Text } else { Role::TextMuted }),
                    ),
            );
        }
        Element::scroll()
            .width_match()
            .height(320)
            .child(rows.width_match())
    });
    let col_dialog = Element::dialog_panel(
        ui.show_cols,
        lang::DT_COLS(),
        620,
        move |_| ui.show_cols.set(false),
        col_body,
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::DT_CLOSE())
                    .small()
                    .neutral()
                    .outline_soft()
                    .on_click(move |_| ui.show_cols.set(false)),
            ),
    );

    // 列选择弹窗由调用方挂根层级：ModalScrim 遮罩铺满根节点，模态覆盖整窗
    (page, col_dialog, build_mysql_site_mgr(&ui))
}

/// MySQL 站点管理弹窗：单层双栏（左站点列表 + 右表单），保存/删除同层完成
fn build_mysql_site_mgr(ui: &MySqlUi) -> Element {
    let mgr_close = ui.site_mgr_show;
    let mgr_form_ui = ui.clone();
    let mgr_list = Element::list_signal(
        ui.sites,
        |s: &crate::core::store::MySqlSite| s.id,
        move |s: crate::core::store::MySqlSite| {
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
                        form_ui.site_host.set(x.host.clone());
                        form_ui.site_port.set(x.port.to_string());
                        form_ui.site_user.set(x.user.clone());
                        form_ui.site_pass.set(x.pass.clone());
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
                    Element::label(format!("{}:{}", s.host, s.port))
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                )
        },
    );
    let form_save = Element::button(lang::SFTP_SITE_SAVE()).small().on_click({
        let (id, _show) = (ui.site_edit_id, ui.site_mgr_show);
        let (name, host, port, user, pass) = (
            ui.site_name,
            ui.site_host,
            ui.site_port,
            ui.site_user,
            ui.site_pass,
        );
        let sites = ui.sites;
        move |_| {
            let n = name.get().trim().to_string();
            if n.is_empty() {
                return;
            }
            let site = crate::core::store::MySqlSite {
                id: id.get(),
                name: n,
                host: host.get().trim().to_string(),
                port: port.get().trim().parse().unwrap_or(3306),
                user: user.get().trim().to_string(),
                pass: pass.get(),
            };
            if crate::core::store::mysql_site_upsert(&site).is_ok() {
                sites.set(crate::core::store::mysql_site_list().unwrap_or_default());
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
                if idv > 0 && crate::core::store::mysql_site_del(idv).is_ok() {
                    sites.set(crate::core::store::mysql_site_list().unwrap_or_default());
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
                ui.site_name.set("示例库址".to_string());
                ui.site_host.set("127.0.0.1".to_string());
                ui.site_port.set("3306".to_string());
                ui.site_user.set("root".to_string());
                ui.site_pass.set(String::new());
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
                .child(
                    Element::row()
                        .spacing(8)
                        .child(Element::text_input(ui.site_host, lang::MYSQL_HOST()).weight(1.0))
                        .child(Element::text_input(ui.site_port, lang::MYSQL_PORT()).width(80)),
                )
                .child(
                    Element::row()
                        .spacing(8)
                        .child(Element::text_input(ui.site_user, lang::MYSQL_USER()).weight(1.0))
                        .child(
                            Element::text_input(ui.site_pass, lang::MYSQL_PASS())
                                .password()
                                .weight(1.0),
                        ),
                )
                .child(Element::flex_spacer()),
        ));
    crate::widgets::mgr_dialog(
        ui.site_mgr_show,
        lang::MYSQL_SITE_TITLE(),
        620,
        move |_| mgr_close.set(false),
        mgr_body,
        form_new,
        form_del,
        form_save,
    )
}

/// "库.表" → (库, 表)：最后一个 '.' 分隔（库/表名都可能含点，展示键按最后一段拆）
fn split_key(key: &str) -> (String, String) {
    match key.rfind('.') {
        Some(pos) => (key[..pos].to_string(), key[pos + 1..].to_string()),
        None => (key.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_key_splits_at_last_dot() {
        assert_eq!(split_key("db.tbl"), ("db".into(), "tbl".into()));
        // 库名可含点：按最后一个点拆
        assert_eq!(split_key("my.db.tbl"), ("my.db".into(), "tbl".into()));
        assert_eq!(split_key("tbl"), ("tbl".into(), String::new()));
    }
}
