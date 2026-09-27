//! Turso（libSQL/SQLite）数据浏览页：库源多库管理 + 表列表 + 数据表格 + SQL 查询。
//! 库源（本地文件 / 网络连接）统一在「管理库址」弹窗维护，存 store.db `turso_dbs` 表。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windui::prelude::*;

use super::{icons, sink, sql, table};
use crate::core;
use crate::core::store::TursoDb;
use crate::core::turso::{self, TursoSource};
use crate::lang;
use crate::widgets::syntax_input::LexerKind;

/// 兜底连接源：SQL 面板回调要求非 Option，无库时给空本地源（连接按钮已禁用，实际到不了）
fn fallback_source(src: Option<TursoSource>) -> TursoSource {
    src.unwrap_or(TursoSource::Local(String::new()))
}

#[derive(Clone)]
pub struct TursoUi {
    /// 库源（多连接）管理：列表 + 下拉选中 + 管理弹窗 + 编辑表单
    pub dbs: Signal<Vec<TursoDb>>,
    pub db_sel: Signal<usize>,
    pub db_mgr_show: Signal<bool>,
    pub db_edit_id: Signal<i64>,
    pub db_name: Signal<String>,
    /// 0=本地文件 1=网络连接（编辑表单内的模式切换）
    pub db_kind: Signal<usize>,
    pub db_path: Signal<String>,
    pub db_url: Signal<String>,
    pub db_token: Signal<String>,
    pub tables: Signal<Vec<String>>,
    pub selected: Signal<Option<String>>,
    pub connected: Signal<bool>,
    pub table_meta: Signal<Vec<core::db::TablePage>>,
    pub table_rows: Signal<Vec<Vec<String>>>,
    pub table_title: Signal<String>,
    pub status: Signal<String>,
    pub error: Signal<String>,
    pub show_sql: Signal<bool>,
    pub show_cols: Signal<bool>,
    pub sql_query: Signal<String>,
    pub sql_status: Signal<String>,
    pub table_loading: Signal<bool>,
    pub pending_table: Signal<Option<(String, usize)>>,
    /// 导出状态：空=空闲，[(已完成行, 总行)]=导出中（Vec 包装以配合 host_signal）
    pub export_progress: Signal<Vec<(usize, usize)>>,
    /// 后台任务结果通道（`App::channel` 创建后回填；节流续接加载时经此再发命令）
    tx: Rc<RefCell<Option<Sender<core::db::DbMsg>>>>,
}

impl TursoUi {
    pub fn new() -> Self {
        Self {
            dbs: signal(core::store::turso_db_list().unwrap_or_default()),
            db_sel: signal(0usize),
            db_mgr_show: signal(false),
            db_edit_id: signal(0i64),
            db_name: signal(String::new()),
            db_kind: signal(0usize),
            db_path: signal(String::new()),
            db_url: signal(String::new()),
            db_token: signal(String::new()),
            tables: signal(Vec::new()),
            selected: signal(None),
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
            pending_table: signal(None),
            export_progress: signal(Vec::new()),
            tx: Rc::new(RefCell::new(None)),
        }
    }

    /// 回填后台任务通道发送端（`App::channel` 返回后调用一次）
    pub fn set_tx(&self, tx: Sender<core::db::DbMsg>) {
        *self.tx.borrow_mut() = Some(tx);
    }

    /// 取后台任务通道发送端（UI 构建前已回填）
    pub fn tx(&self) -> Sender<core::db::DbMsg> {
        self.tx
            .borrow()
            .as_ref()
            .expect("turso tx 已在 run() 中回填")
            .clone()
    }

    /// 当前选中的库源（无库返回 None——连接按钮此时已禁用）
    pub fn selected_db(&self) -> Option<TursoDb> {
        let list = self.dbs.get();
        let idx = self.db_sel.get().min(list.len().saturating_sub(1));
        list.get(idx).cloned()
    }

    /// 当前连接源：取选中库源；无选中或空配置返回 None
    pub fn make_source(&self) -> Option<TursoSource> {
        self.selected_db().map(|db| {
            if db.kind == 1 {
                TursoSource::Remote {
                    url: db.url.trim().to_string(),
                    token: db.token.trim().to_string(),
                }
            } else {
                TursoSource::Local(db.path)
            }
        })
    }

    /// 消费后台数据库任务消息（`App::channel` 回 UI 线程时调用）
    pub fn on_db_msg(&self, msg: core::db::DbMsg) {
        match msg {
            core::db::DbMsg::ConnectedGroups(_) => {} // MySQL/PG 页消息，本页不消费
            core::db::DbMsg::Connected(Ok((tables_list, fname))) => {
                // 换库/重连后旧库的浏览缓存可能残留，先整体失效
                core::turso::invalidate();
                // 先取长度再整体移交，避免 set 克隆整张表列表
                let count = tables_list.len();
                self.tables.set(tables_list);
                self.connected.set(true);
                self.table_meta.set(Vec::new());
                self.table_rows.set(Vec::new());
                self.table_title.set(lang::DT_TABLE_LIST(count));
                self.error.set(String::new());
                self.status.set(lang::TURSO_CONNECTED(&fname, count));
            }
            core::db::DbMsg::Connected(Err(e)) => self.error.set(e),
            core::db::DbMsg::TableLoaded(Ok(page_data)) => {
                self.apply_page(page_data);
                // 节流续接：本次加载完成，若连点期间有排队表则继续加载
                self.table_loading.set(false);
                if let Some(t) = self.pending_table.get() {
                    self.pending_table.set(None);
                    self.table_loading.set(true);
                    if let Some(src) = self.make_source() {
                        core::db::spawn_load_table(sink(self.tx()), src, t.0, t.1);
                    }
                }
            }
            core::db::DbMsg::TableLoaded(Err(e)) => {
                self.error.set(e);
                self.table_loading.set(false);
                if let Some(t) = self.pending_table.get() {
                    self.pending_table.set(None);
                    self.table_loading.set(true);
                    if let Some(src) = self.make_source() {
                        core::db::spawn_load_table(sink(self.tx()), src, t.0, t.1);
                    }
                }
            }
            core::db::DbMsg::SqlDone(Ok(page_data)) => {
                let cols = page_data.columns.len();
                let rows = page_data.row_count;
                self.apply_page(page_data);
                self.sql_status.set(lang::TURSO_SQL_STATUS(cols, rows));
                self.error.set(String::new());
            }
            core::db::DbMsg::SqlDone(Err(e)) => {
                self.sql_status.set(format!("\u{274C} {}", e));
            }
            core::db::DbMsg::ExportProgress { done, total } => {
                self.export_progress.set(vec![(done, total)]);
            }
            core::db::DbMsg::ExportDone(Ok(path)) => {
                self.export_progress.set(Vec::new());
                self.status.set(lang::TURSO_EXPORT_DONE(&path));
            }
            core::db::DbMsg::ExportDone(Err(e)) => {
                self.export_progress.set(Vec::new());
                self.error.set(e);
            }
        }
    }

    /// 表格快照落地：行数据交给虚拟滚动数据源，元信息（表名/列/选中行）单独存
    fn apply_page(&self, page_data: core::db::TablePage) {
        let core::db::TablePage {
            table_name,
            columns,
            rows,
            selected_row,
            row_count,
            page_offset,
            ..
        } = page_data;
        // 默认只显示前 DEFAULT_VISIBLE_COLS 列（其余可在列设置面板勾选）
        let visible = table::default_visible(columns.len());
        self.table_meta.set(vec![core::db::TablePage {
            table_name,
            columns,
            rows: Vec::new(),
            visible,
            selected_row,
            row_count,
            page_offset,
            sql_status: None,
        }]);
        self.table_rows.set(rows);
        self.error.set(String::new());
    }
}

impl Default for TursoUi {
    fn default() -> Self {
        Self::new()
    }
}

/// Turso 页 UI：库源管理 + 连接/断开/刷新 + SQL 面板 + 表列表 + 数据表格
/// （返回 (页面, 列设置弹窗, 库址管理弹窗)：弹窗须挂根层级，遮罩才铺满全窗）
pub fn build_turso_tab(ui: &TursoUi) -> (Element, Element, Element) {
    let ui = ui.clone(); // 拥有所有权：供多个 'static 闭包捕获
    let TursoUi {
        dbs,
        db_sel,
        tables,
        selected,
        connected,
        table_meta,
        table_rows,
        table_title,
        status,
        error,
        show_sql,
        show_cols,
        sql_query,
        sql_status,
        table_loading,
        pending_table,
        export_progress,
        ..
    } = ui.clone();

    // 连接源取自选中库源。连接/刷新/翻页/SQL/表列表多处共用同一构造闭包；
    // 无选中库时返回 None（各调用点跳过发命令）。Rc 共享（闭包非 Copy）。
    let make_source = {
        let ui = ui.clone();
        std::rc::Rc::new(move || ui.make_source())
    };
    // 工作线程命令发送端（Sender 非 Copy，各按钮闭包自行 clone）
    let tx = ui.tx();

    let connect_btn = || {
        Element::button(lang::TURSO_CONNECT())
            .neutral()
            .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
            .small()
            .visible_when(move || !connected.get())
            .enabled_when(move || !dbs.get().is_empty())
            .on_click({
                let tx_connect = tx.clone();
                let ui_c = ui.clone();
                move |_| {
                    let Some(src) = ui_c.make_source() else {
                        return;
                    };
                    core::db::spawn_connect(sink(tx_connect.clone()), src);
                }
            })
    };

    // 断开连接：与「连接」为 toggle，连接态仅显示断开（清缓存 + 复位页面状态）
    let disconnect_btn = || {
        Element::button(lang::TURSO_DISCONNECT())
            .neutral()
            .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
            .small()
            .visible_when(move || connected.get())
            .on_click(move |_| {
                turso::disconnect();
                connected.set(false);
                tables.set(Vec::new());
                selected.set(None);
                table_meta.set(Vec::new());
                table_rows.set(Vec::new());
                table_title.set(lang::DT_TABLE_LIST(0));
                show_sql.set(false);
                show_cols.set(false);
                status.set(String::new());
                error.set(String::new());
            })
    };

    // 刷新当前表（后台线程执行）
    let tx_refresh = tx.clone();
    let ms_refresh = make_source.clone();
    let refresh = Element::button(lang::TURSO_REFRESH())
        .neutral()
        .icon_content(icons::stateful_icon(icons::REFRESH, Some(16)))
        .small()
        .enabled_signal(connected)
        .on_click(move |_| {
            if let Some(table) = selected.get() {
                // 刷新：先清浏览缓存，确保本次重拉拿到最新数据（而非命中旧页）
                core::turso::invalidate();
                // 刷新当前页（保留分页位置）
                let offset = table_meta.get().first().map(|t| t.page_offset).unwrap_or(0);
                let ms = ms_refresh.clone();
                if let Some(src) = ms() {
                    core::db::spawn_load_table(sink(tx_refresh.clone()), src, table, offset);
                }
                status.set(lang::TURSO_REFRESHED().to_string());
            }
        });

    // SQL 面板开关（文案固定，面板显隐由 visible_when 控制）
    let sql_toggle = Element::button(lang::TURSO_OPEN_SQL())
        .neutral()
        .icon_content(icons::stateful_icon(icons::TERMINAL, Some(16)))
        .small()
        .enabled_signal(connected)
        .on_click(move |_| show_sql.set(!show_sql.get()));

    // 列设置开关：弹出列勾选弹窗（默认只显示前几列，其余列在此勾选显示）。
    // 需连接且已选中表才可设置。
    let cols_toggle = Element::button(lang::DT_COLS())
        .neutral()
        .icon_content(icons::stateful_icon(icons::TABLE_ICON, Some(16)))
        .small()
        .enabled_when(move || connected.get() && selected.get().is_some())
        .on_click(move |_| show_cols.set(!show_cols.get()));

    // ── 连接 + 操作：单行紧凑条（库源下拉/管理/连接/刷新/SQL/列设置 一条放下）──
    let conn_bar = Element::row()
        .width_match()
        .spacing(6)
        .cross(Align::Center)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding_xy(10, 8)
        .child(
            // 库源下拉：连接参数直接取选中库源（管理弹窗内增删改）
            Element::dropdown_signal(
                db_sel.map(move |idx: &usize| {
                    let idx = *idx;
                    let list = dbs.get();
                    if list.is_empty() {
                        vec![lang::S3_SITE_NONE()]
                    } else {
                        vec![list
                            .get(idx.min(list.len().saturating_sub(1)))
                            .map(|s| s.name.clone())
                            .unwrap_or_default()]
                    }
                }),
                db_sel,
            )
            .width(160),
        )
        .child(
            Element::button(lang::PG_CONN_MGR())
                .neutral()
                .icon_content(icons::stateful_icon(icons::SERVER, Some(16)))
                .small()
                .on_click(move |_| ui.db_mgr_show.set(true)),
        )
        .child(connect_btn())
        .child(disconnect_btn())
        .child(Element::leaf().width(1).height(20).bg_role(Role::Divider))
        .child(refresh)
        .child(sql_toggle)
        .child(cols_toggle)
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

    // 左侧表列表（数据驱动，选中行加亮；点击异步加载，节流防连点卡死）
    let table_list = table::render_table_list(
        tables,
        selected,
        {
            let ui2 = ui.clone();
            move || ui2.make_source()
        },
        tx.clone(),
        table_loading,
        pending_table,
    );

    // 右侧数据表格（元信息驱动视图切换 + 整表行虚拟滚动）
    let data_table = table::render_data_table(table_meta, table_rows);

    // 列勾选弹窗内容：列出所有列，点击行切换该列在表格中的显示/隐藏。
    // meta 信号驱动重建——勾选后 visible 变化 → 表格与弹窗同步刷新。
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
        Element::scroll().width_match().height(240).child(
            Element::col()
                .width_match()
                .spacing(2)
                .padding_xy(4, 4)
                .child(rows),
        )
    });

    // 列设置弹窗：连接且选中表后才可设置（cols_toggle 已做启用门槛）
    let col_dialog = Element::dialog_panel(
        show_cols,
        lang::DT_COLS(),
        620,
        move |_| show_cols.set(false),
        col_body,
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::DT_CLOSE())
                    .small()
                    .neutral()
                    .outline_soft()
                    .on_click(move |_| show_cols.set(false)),
            ),
    );

    // ── 分页条：上一页 / 第 x / y 页 / 下一页（每页 PAGE_SIZE 行，节流防连点）──
    // 构建闭包是 move，预克隆 sender（原 tx 留给下方 SQL 面板）
    let page_bar_tx = tx.clone();
    let ui_pb = ui.clone(); // host_signal 闭包是 Fn：每次重建克隆一份供内部回调
    let page_bar = Element::host_signal(table_meta, move |t: core::db::TablePage| {
        let ui = ui_pb.clone();
        // 未选中表、或正查看某行详情（非列表视图）时都不显示分页——
        // 详情视图没有"页"的概念，分页条只服务列表浏览。
        if t.selected_row.is_some() {
            return Element::leaf();
        }
        let Some(name) = t.table_name.clone() else {
            return Element::leaf();
        };
        let total_pages = if t.row_count == 0 {
            1
        } else {
            t.row_count.div_ceil(turso::PAGE_SIZE)
        };
        let cur = t.page_offset / turso::PAGE_SIZE;

        // 翻页走与表名点击同一节流机制：加载中则排队，完成后续接。
        // Rc 包装：prev/next 两个按钮回调各自持有一份
        let go = std::rc::Rc::new({
            let tx = page_bar_tx.clone();
            let name = name.clone();
            let ui2 = ui.clone();
            move |offset: usize| {
                if table_loading.get() {
                    pending_table.set(Some((name.clone(), offset)));
                    return;
                }
                table_loading.set(true);
                if let Some(src) = ui2.make_source() {
                    core::db::spawn_load_table(sink(tx.clone()), src, name.clone(), offset);
                }
            }
        });

        let prev = Element::button(lang::DT_PREV())
            .small()
            .neutral()
            .outline_soft()
            .enabled_when(move || cur > 0)
            .on_click({
                let go = go.clone();
                move |_| go(cur.saturating_sub(1) * turso::PAGE_SIZE)
            });

        let next = Element::button(lang::DT_NEXT())
            .small()
            .neutral()
            .outline_soft()
            .enabled_when(move || cur + 1 < total_pages)
            .on_click({
                let go = go.clone();
                move |_| go((cur + 1) * turso::PAGE_SIZE)
            });

        // 导出全表 CSV：先弹保存对话框（默认 表名.csv），选好后后台分批导出，
        // 进度经 export_progress 信号驱动下方进度条显示；导出中禁用按钮防重复。
        let export_tx = page_bar_tx.clone();
        let export_name = name.clone();
        let ui3 = ui.clone();
        let export = Element::button(lang::DT_EXPORT())
            .small()
            .neutral()
            .outline_soft()
            .enabled_when(move || !table_loading.get() && export_progress.get().is_empty())
            .on_click(move |ctx| {
                let tx_c = export_tx.clone();
                let table_c = export_name.clone();
                let src_c = ui3.make_source();
                ctx.request_save_file(
                    PickDialog::new()
                        .title(lang::DT_EXPORT_TITLE())
                        .filter("CSV", &["csv"])
                        .file_name(format!("{}.csv", table_c)),
                    move |path: Option<PathBuf>| {
                        if let (Some(dest), Some(src)) = (path, src_c) {
                            let dest = if dest
                                .extension()
                                .map(|e| e.eq_ignore_ascii_case("csv"))
                                .unwrap_or(false)
                            {
                                dest
                            } else {
                                dest.with_extension("csv")
                            };
                            core::db::spawn_export_csv(sink(tx_c), src, table_c, dest);
                        }
                    },
                );
            });

        // 导出进度指示：从 export_progress 派生标量信号（比例 + 文案），
        // 用 visible_when 控制显隐。不要用嵌套 host_signal——它返回 col().fill()
        // 容器，嵌在分页条行内会抢布局空间把上方数据表格挤没。
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
                            // 表列表面板标题：图标 + 表数，与内容区顶部对齐更有分区感
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
                move || {
                    core::db::DbSource::Turso(fallback_source(make_source.clone()()))
                },
                tx,
                LexerKind::Sql,
                "turso",
            )
            .visible_when(move || show_sql.get()),
        );

    // 页面（弹窗由调用方挂根层级：ModalScrim 遮罩铺满根节点，模态覆盖整窗）
    (page, col_dialog, build_turso_db_mgr(&ui))
}

/// Turso 库源管理弹窗：单层双栏（左库列表 + 右表单），保存/删除同层完成。
/// 表单支持两种模式：本地文件（文件选择器）与网络连接（URL + 令牌）。
fn build_turso_db_mgr(ui: &TursoUi) -> Element {
    let ui = ui.clone(); // 拥有所有权：供 'static 闭包捕获
    let mgr_close = ui.db_mgr_show;
    let mgr_form_ui = ui.clone();
    let mgr_list = Element::list_signal(
        ui.dbs,
        |s: &TursoDb| s.id,
        move |s: TursoDb| {
            let (sel, dbs) = (mgr_form_ui.db_sel, mgr_form_ui.dbs);
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
                    let list = dbs.get();
                    if let Some(idx) = list.iter().position(|x| x.id == row_id) {
                        sel.set(idx);
                    }
                    if let Some(x) = list.iter().find(|x| x.id == row_id) {
                        form_ui.db_edit_id.set(x.id);
                        form_ui.db_name.set(x.name.clone());
                        form_ui.db_kind.set(x.kind.max(0) as usize);
                        form_ui.db_path.set(x.path.clone());
                        form_ui.db_url.set(x.url.clone());
                        form_ui.db_token.set(x.token.clone());
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
                    Element::label(turso_db_display(&s))
                        .font_size(11.0)
                        .fg_role(Role::TextMuted)
                        .max_lines(1)
                        .truncate(Truncate::End),
                )
        },
    );

    // 本地文件选择：必须用 ctx.request_pick_file 延迟到事件分发结束后弹出（见页内注释）
    let pick_btn = Element::button(lang::TURSO_OPEN())
        .small()
        .neutral()
        .outline_soft()
        .icon_content(icons::stateful_icon(icons::FOLDER, Some(14)))
        .on_click({
            let db_path = ui.db_path;
            move |ctx| {
                let db_path = db_path;
                ctx.request_pick_file(
                    PickDialog::new()
                        .title(lang::TURSO_FILE_TITLE())
                        .filter(lang::TURSO_FILE_FILTER1(), &["db", "sqlite", "libsql"])
                        .filter(lang::TURSO_FILE_FILTER2(), &["*"]),
                    move |path: Option<PathBuf>| {
                        if let Some(path) = path {
                            db_path.set(path.to_string_lossy().to_string());
                        }
                    },
                );
            }
        });

    let form_save = Element::button(lang::SFTP_SITE_SAVE()).small().on_click({
        let (id, _show) = (ui.db_edit_id, ui.db_mgr_show);
        let (name, kind, path, url, token) = (
            ui.db_name,
            ui.db_kind,
            ui.db_path,
            ui.db_url,
            ui.db_token,
        );
        let dbs = ui.dbs;
        move |_| {
            let n = name.get().trim().to_string();
            if n.is_empty() {
                return;
            }
            let db = TursoDb {
                id: id.get(),
                name: n,
                kind: kind.get() as i64,
                path: path.get().trim().to_string(),
                url: url.get().trim().to_string(),
                token: token.get().trim().to_string(),
            };
            if core::store::turso_db_upsert(&db).is_ok() {
                dbs.set(core::store::turso_db_list().unwrap_or_default());
            }
            // 保存后不关弹窗：继续编辑/新建（关闭仅靠 X / Esc / 点遮罩）
        }
    });
    let form_del = Element::button(lang::S3_SITE_DEL())
        .small()
        .neutral()
        .danger()
        .on_click({
            let id = ui.db_edit_id;
            let dbs = ui.dbs;
            move |_| {
                let idv = id.get();
                if idv > 0 && core::store::turso_db_del(idv).is_ok() {
                    dbs.set(core::store::turso_db_list().unwrap_or_default());
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
                ui.db_edit_id.set(0);
                ui.db_name.set("示例库".to_string());
                ui.db_kind.set(0);
                ui.db_path.set(String::new());
                ui.db_url.set(String::new());
                ui.db_token.set(String::new());
            }
        });

    // 表单：名称一行；模式切换（本地/网络）；本地=路径输入+选择按钮，网络=URL+令牌
    let kind_row = Element::row()
        .width_match()
        .spacing(6)
        .child(
            Element::segmented(
                vec![lang::TURSO_MODE_LOCAL(), lang::TURSO_MODE_REMOTE()],
                ui.db_kind,
            )
            .font_size(12.0),
        )
        .child(Element::flex_spacer());
    let local_row = Element::row()
        .width_match()
        .spacing(6)
        .visible_when(move || ui.db_kind.get() == 0)
        .child(
            Element::text_input(ui.db_path, lang::TURSO_PATH_HINT())
                .width_match(),
        )
        .child(pick_btn);
    let remote_rows = Element::col()
        .width_match()
        .spacing(8)
        .visible_when(move || ui.db_kind.get() == 1)
        .child(
            Element::text_input(ui.db_url, lang::TURSO_URL_HINT())
                .autofocus()
                .width_match(),
        )
        .child(
            Element::text_input(ui.db_token, lang::TURSO_TOKEN_HINT())
                .width_match(),
        );
    let mgr_body = Element::row()
        .width_match()
        .height(320)
        .spacing(12)
        .child(crate::widgets::mgr_list_col(
            mgr_list,
            ui.dbs.map(|s| s.is_empty()),
        ))
        .child(crate::widgets::mgr_form_col(
            Element::col()
                .width_match()
                .height_match()
                .spacing(8)
                .child(
                    Element::text_input(ui.db_name, lang::SFTP_SITE_NAME())
                        .autofocus()
                        .width_match(),
                )
                .child(kind_row)
                .child(local_row)
                .child(remote_rows)
                .child(Element::flex_spacer()),
        ));
    crate::widgets::mgr_dialog(
        ui.db_mgr_show,
        lang::TURSO_DB_TITLE(),
        620,
        move |_| mgr_close.set(false),
        mgr_body,
        form_new,
        form_del,
        form_save,
    )
}

/// 库源列表展示文案：本地=文件名，网络=URL（隐藏令牌）
fn turso_db_display(db: &TursoDb) -> String {
    if db.kind == 1 {
        db.url.clone()
    } else {
        std::path::Path::new(&db.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| db.path.clone())
    }
}
