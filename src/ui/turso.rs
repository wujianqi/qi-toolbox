//! Turso 页 UI：打开/连接/刷新 + SQL 面板 + 表列表 + 数据表格
//!
//! 页面状态封装在 [`TursoUi`]：含全部信号 + 后台任务通道发送端，
//! 后台消息统一由 [`TursoUi::on_db_msg`] 消费（写信号 + 节流续接排队表）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windui::prelude::*;

use super::{icons, input_dialog, sink, sql, table};
use crate::core;
use crate::core::turso::{self, TursoSource};
use crate::lang;

/// Turso 页状态：全部 UI 信号 + 后台任务通道发送端
#[derive(Clone)]
pub struct TursoUi {
    pub db_path: Signal<String>,
    pub turso_mode: Signal<usize>,
    pub turso_url: Signal<String>,
    pub turso_token: Signal<String>,
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
    pub show_addr: Signal<bool>,
    pub sql_query: Signal<String>,
    pub sql_status: Signal<String>,
    pub table_loading: Signal<bool>,
    pub pending_table: Signal<Option<(String, usize)>>,
    /// 后台任务结果通道（`App::channel` 创建后回填；节流续接加载时经此再发命令）
    tx: Rc<RefCell<Option<Sender<core::db::DbMsg>>>>,
}

impl TursoUi {
    pub fn new() -> Self {
        Self {
            db_path: signal(String::new()),
            turso_mode: signal(0usize), // 0=本地文件 1=网络连接
            turso_url: signal(String::new()),
            turso_token: signal(String::new()),
            tables: signal(Vec::new()),
            selected: signal(None),
            connected: signal(false),
            table_meta: signal(Vec::new()),
            table_rows: signal(Vec::new()),
            table_title: signal(lang::DT_TABLE_LIST().replace("{}", "0")),
            status: signal(String::new()),
            error: signal(String::new()),
            show_sql: signal(false),
            show_cols: signal(false),
            show_addr: signal(false),
            sql_query: signal(String::new()),
            sql_status: signal(String::new()),
            table_loading: signal(false),
            pending_table: signal(None),
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

    /// 当前连接源：本地文件路径 或 网络 URL+令牌（由模式/输入合成）
    pub fn make_source(&self) -> TursoSource {
        let mode = self.turso_mode.get();
        let path = self.db_path.get();
        let url = self.turso_url.get();
        let token = self.turso_token.get();
        if mode == 1 && !url.trim().is_empty() {
            TursoSource::Remote {
                url: url.trim().to_string(),
                token: token.trim().to_string(),
            }
        } else {
            TursoSource::Local(path)
        }
    }

    /// 消费后台数据库任务消息（`App::channel` 回 UI 线程时调用）
    pub fn on_db_msg(&self, msg: core::db::DbMsg) {
        match msg {
            core::db::DbMsg::Connected(Ok((tables_list, fname))) => {
                // 换库/重连后旧库的浏览缓存可能残留，先整体失效
                core::turso::invalidate();
                // 先取长度再整体移交，避免 set 克隆整张表列表
                let count = tables_list.len();
                self.tables.set(tables_list);
                self.connected.set(true);
                self.table_meta.set(Vec::new());
                self.table_rows.set(Vec::new());
                self.table_title
                    .set(lang::DT_TABLE_LIST().replace("{}", &count.to_string()));
                self.error.set(String::new());
                self.status.set(
                    lang::TURSO_CONNECTED()
                        .replacen("{}", &fname, 1)
                        .replacen("{}", &count.to_string(), 1),
                );
            }
            core::db::DbMsg::Connected(Err(e)) => self.error.set(e),
            core::db::DbMsg::TableLoaded(Ok(page_data)) => {
                self.apply_page(page_data);
                // 节流续接：本次加载完成，若连点期间有排队表则继续加载
                self.table_loading.set(false);
                if let Some(t) = self.pending_table.get() {
                    self.pending_table.set(None);
                    self.table_loading.set(true);
                    core::db::spawn_load_table(
                        sink(self.tx()),
                        self.make_source(),
                        t.0,
                        t.1,
                    );
                }
            }
            core::db::DbMsg::TableLoaded(Err(e)) => {
                self.error.set(e);
                self.table_loading.set(false);
                if let Some(t) = self.pending_table.get() {
                    self.pending_table.set(None);
                    self.table_loading.set(true);
                    core::db::spawn_load_table(
                        sink(self.tx()),
                        self.make_source(),
                        t.0,
                        t.1,
                    );
                }
            }
            core::db::DbMsg::SqlDone(Ok(page_data)) => {
                let cols = page_data.columns.len();
                let rows = page_data.row_count;
                self.apply_page(page_data);
                self.sql_status.set(
                    lang::TURSO_SQL_STATUS()
                        .replacen("{}", &cols.to_string(), 1)
                        .replacen("{}", &rows.to_string(), 1),
                );
                self.error.set(String::new());
            }
            core::db::DbMsg::SqlDone(Err(e)) => {
                self.sql_status.set(format!("\u{274C} {}", e));
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

/// Turso 页 UI：打开/连接/刷新 + SQL 面板 + 表列表 + 数据表格
pub fn build_turso_tab(ui: &TursoUi) -> Element {
    let TursoUi {
        db_path,
        turso_mode,
        turso_url,
        turso_token,
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
        show_addr,
        sql_query,
        sql_status,
        table_loading,
        pending_table,
        ..
    } = ui.clone();

    // 连接源由模式决定：本地文件路径 或 网络 URL+令牌。连接/刷新/翻页/SQL/表列表
    // 多处共用同一构造闭包，避免重复拼装；Signal 为 Copy，闭包可复制给多个回调。
    let make_source = move || {
        let mode = turso_mode.get();
        let path = db_path.get();
        let url = turso_url.get();
        let token = turso_token.get();
        if mode == 1 && !url.trim().is_empty() {
            TursoSource::Remote {
                url: url.trim().to_string(),
                token: token.trim().to_string(),
            }
        } else {
            TursoSource::Local(path)
        }
    };
    // 工作线程命令发送端（Sender 非 Copy，各按钮闭包自行 clone）
    let tx = ui.tx();

    // 打开文件对话框：必须用 ctx.request_pick_file 延迟到事件分发结束后弹出，
    // 框架会自动把当前窗口注入为对话框父窗口（Windows 下主窗口被禁用直到关闭），
    // 避免裸调 rfd::FileDialog 无父窗口导致无法获得焦点、与鼠标捕获冲突。
    let open = Element::button(lang::TURSO_OPEN())
        .neutral()
        .icon_content(icons::stateful_icon(icons::FOLDER, Some(16)))
        .small()
        .on_click({
            let tx_open = tx.clone();
            move |ctx| {
                // 每次打开会话独立克隆发送端，供回调独占（Sender 非 Copy）
                let tx_c = tx_open.clone();
                ctx.request_pick_file(
                    PickDialog::new()
                        .title(lang::TURSO_FILE_TITLE())
                        .filter(lang::TURSO_FILE_FILTER1(), &["db", "sqlite", "libsql"])
                        .filter(lang::TURSO_FILE_FILTER2(), &["*"]),
                    move |path: Option<PathBuf>| {
                        if let Some(path) = path {
                            let p = path.to_string_lossy().to_string();
                            db_path.set(p.clone());
                            let fname = std::path::Path::new(&p)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| p.clone());
                            // 打开成功即自动连接（无需再手动点「连接」）
                            if connected.get() {
                                // 已在连接旧库/网络：先断开并复位页面状态，再连新文件
                                turso::disconnect();
                                connected.set(false);
                                tables.set(Vec::new());
                                selected.set(None);
                                table_meta.set(Vec::new());
                                table_rows.set(Vec::new());
                                table_title.set(lang::DT_TABLE_LIST().replace("{}", "0"));
                                show_sql.set(false);
                                show_cols.set(false);
                                status.set(String::new());
                                error.set(String::new());
                            }
                            status.set(format!("\u{2705} {}", fname));
                            // 记忆打开的本地库（与「连接」按钮的持久化一致）
                            let p_s = p.clone();
                            crate::core::settings::commit(&[
                                ("turso.db_path", Some(p_s.as_str())),
                                ("turso.mode", Some("0")),
                            ]);
                            core::db::spawn_connect(sink(tx_c), TursoSource::Local(p));
                        }
                    },
                );
            }
        });
    let connect_btn = || {
        Element::button(lang::TURSO_CONNECT())
            .neutral()
            .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
            .small()
            .visible_when(move || !connected.get())
            .on_click({
                let tx_connect = tx.clone();
                move |_| {
                    // 记忆连接源（尽力而为：路径/URL 明文、token 加密后落盘）
                    let db_path_s = db_path.get();
                    let mode_s = turso_mode.get().to_string();
                    let url_s = turso_url.get();
                    let token_s = turso_token.get();
                    crate::core::settings::commit(&[
                        ("turso.db_path", Some(db_path_s.as_str())),
                        ("turso.mode", Some(&mode_s)),
                        ("turso.url", Some(url_s.trim())),
                        ("turso.token", Some(token_s.as_str())),
                    ]);
                    core::db::spawn_connect(sink(tx_connect.clone()), make_source());
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
                table_title.set(lang::DT_TABLE_LIST().replace("{}", "0"));
                show_sql.set(false);
                show_cols.set(false);
                status.set(String::new());
                error.set(String::new());
            })
    };

    // 刷新当前表（后台线程执行）
    let tx_refresh = tx.clone();
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
                let offset = table_meta
                    .get()
                    .first()
                    .map(|t| t.page_offset)
                    .unwrap_or(0);
                core::db::spawn_load_table(sink(tx_refresh.clone()), make_source(), table, offset);
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

    // ── 连接 + 操作：单行紧凑条（模式/输入/连接/刷新/SQL/列设置 一条放下，省出空间给数据表）──
    let conn_bar = Element::row()
        .width_match()
        .spacing(6)
        .cross(Align::Center)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding_xy(10, 8)
        .child(Element::segmented(
            vec![lang::TURSO_MODE_LOCAL(), lang::TURSO_MODE_REMOTE()],
            turso_mode,
        )
        .font_size(12.0))
        .child(open.visible_when(move || turso_mode.get() == 0))
        .child(
            Element::button(lang::TURSO_URL_BTN())
                .neutral()
                .small()
                .visible_when(move || turso_mode.get() == 1)
                .on_click(move |_| show_addr.set(true)),
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
        make_source,
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
        Element::scroll()
            .width_match()
            .height(240)
            .child(
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
        300,
        move |_| show_cols.set(false),
        col_body,
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::DT_CLOSE())
                    .small()
                    .on_click(move |_| show_cols.set(false)),
            ),
    );

    // ── 网络地址模态窗：URL + 令牌（远程模式内联输入移入弹窗，节省顶部空间）──
    let addr_dialog = input_dialog(
        show_addr,
        lang::TURSO_URL(),
        380,
        move |_| show_addr.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(
                Element::text_input(turso_url, lang::TURSO_URL_HINT())
                    .width_match(),
            )
            .child(
                Element::text_input(turso_token, lang::TURSO_TOKEN_HINT())
                    .width_match(),
            ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .outline_soft()
                    .neutral()
                    .on_click(move |_| show_addr.set(false)),
            )
            .child(
                Element::button(lang::SFTP_OK())
                    .small()
                    .on_click(move |_| show_addr.set(false)),
            ),
    );

    // ── 分页条：上一页 / 第 x / y 页 / 下一页（每页 PAGE_SIZE 行，节流防连点）──
    // 构建闭包是 move，预克隆 sender（原 tx 留给下方 SQL 面板）
    let page_bar_tx = tx.clone();
    let page_bar = Element::host_signal(table_meta, move |t: core::db::TablePage| {
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
            move |offset: usize| {
                if table_loading.get() {
                    pending_table.set(Some((name.clone(), offset)));
                    return;
                }
                table_loading.set(true);
                core::db::spawn_load_table(sink(tx.clone()), make_source(), name.clone(), offset);
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

        Element::row()
            .width_match()
            .spacing(8)
            .cross(Align::Center)
            .child(prev)
            .child(
                Element::label(
                    lang::DT_PAGE()
                        .replacen("{}", &(cur + 1).to_string(), 1)
                        .replacen("{}", &total_pages.to_string(), 1),
                )
                .font_size(12.0)
                .fg_role(Role::TextMuted),
            )
            .child(next)
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
                                        ImageContent::from_svg_bytes(
                                            icons::TABLE_ICON,
                                            Some(14),
                                        )
                                        .tint(Role::TextMuted.resolve(&windui::theme::current())),
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
                        .child(table_list.weight(1.0)),
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
            sql::render_sql_panel(sql_query, sql_status, connected, make_source, tx)
            .visible_when(move || show_sql.get()),
        );

    // 页面 + 弹窗（弹窗为浮层，叠在页面之上）
    Element::stack()
        .fill()
        .child(page)
        .child(col_dialog)
        .child(addr_dialog)
}
