//! SQL 查询面板（windui 版，UI 层）
//!
//! 多行输入（`SyntaxInput`，SQL 语法高亮、等宽字体）+ 执行/清空按钮 + 状态栏。
//! SQL 在后台线程执行（`core::db::spawn_execute_sql`），结果经 channel 回 UI 线程。
//! 执行历史与保存查询收进「查询管理」弹窗（随面板返回，由调用方挂根层级），
//! 工具栏只留一枚入口按钮，避免一排下拉+小按钮挤占一行。

use std::cell::RefCell;

use windui::prelude::*;

use super::{icons, sink_opt};
use crate::core;
use crate::core::db::DbSource;
use crate::lang;
use crate::widgets::syntax_input::{LexerKind, SyntaxInput};

/// 「查询管理」弹窗显隐 + 内容刷新信号的信号池：按 `hist_key` 复用 thread_local
/// 槽位，永不随重建回收。不能在 `render_sql_panel` 里 `signal(…)` 现建——主题切换
/// 整树重建会 dispose 构建期信号，弹窗/列表持有的句柄即成死句柄，再读即 panic
/// （signal.rs:545）。同一连接源每次构建取池中信号并复位显隐为 false。
/// 刷新信号用 `Signal<Vec<usize>>`（恒 1 元素计数器）：`host_signal` 要求 Vec，
/// 计数值变化触发弹窗内列表整体重读 store。
/// 信号池条目：`hist_key` + 弹窗显隐信号 + 刷新计数信号
type SqlMgrSig = (String, Signal<bool>, Signal<Vec<usize>>);

fn sql_mgr_sigs(hist_key: &str) -> (Signal<bool>, Signal<Vec<usize>>) {
    thread_local! {
        static POOL: RefCell<Vec<SqlMgrSig>> = const { RefCell::new(Vec::new()) };
    }
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        if let Some(pos) = p.iter().position(|(k, _, _)| k == hist_key) {
            // 自愈：整树重建的 SignalScope 会收走构建期信号，死句柄换新重建
            if !p[pos].1.is_alive() {
                p[pos].1 = signal(false);
            }
            if !p[pos].2.is_alive() {
                p[pos].2 = signal(vec![0usize]);
            }
            p[pos].1.set(false);
            return (p[pos].1, p[pos].2);
        }
        let (show, refresh) = (signal(false), signal(vec![0usize]));
        p.push((hist_key.to_string(), show, refresh));
        (show, refresh)
    })
}

/// 只读模式开关信号的信号池：与 [`sql_mgr_sigs`] 同理，thread_local 槽位
/// 跨整树重建存活，key 为 settings 键（sql.readonly.{hist_key}）。
fn ro_sig(key: &str) -> Signal<bool> {
    thread_local! {
        static POOL: RefCell<Vec<(String, Signal<bool>)>> = const { RefCell::new(Vec::new()) };
    }
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        if let Some(pos) = p.iter().position(|(k, _)| k == key) {
            if !p[pos].1.is_alive() {
                p[pos].1 = signal(false);
            }
            return p[pos].1;
        }
        // 初始值读 settings（"1" = 只读开；默认关）
        let v = crate::core::settings::load()
            .get(key)
            .map(|s| s == "1")
            .unwrap_or(false);
        let s = signal(v);
        p.push((key.to_string(), s));
        s
    })
}

/// 渲染 SQL 查询面板（执行按钮 + 多行输入框 + 状态栏 + 查询管理入口）
/// `make_source` 返回统一连接源（Turso/MySQL/PG 三后端由 core::db 分发）。
/// `lexer` 指定 SQL 方言高亮（Turso=Sql、MySQL=SqlMySql、PG=SqlPg）。
/// `hist_key` 为历史存储键（按连接源区分，如 "turso" / "mysql" / "pg"）。
///
/// 返回 `(面板, 查询管理弹窗)`：弹窗由调用方挂根层级（ModalScrim 遮罩铺满根节点）。
pub fn render_sql_panel(
    sql_query: Signal<String>,
    connected: Signal<bool>,
    make_source: impl Fn() -> DbSource + 'static,
    tx: Option<Sender<core::db::DbMsg>>,
    lexer: LexerKind,
    hist_key: &'static str,
) -> (Element, Element) {
    let (mgr_show, mgr_refresh) = sql_mgr_sigs(hist_key);

    // ── 只读模式开关（Switch 控件，记忆于 settings；开启后拦截写语句）──
    let ro_key = format!("sql.readonly.{hist_key}");
    let readonly = ro_sig(&ro_key);
    let readonly_sw = Element::switch(readonly).on_switch_change({
        let ro_key = ro_key.clone();
        move |_, on| {
            crate::core::settings::commit(&[(ro_key.as_str(), Some(if on { "1" } else { "0" }))]);
        }
    });

    let exec = Element::button(lang::TURSO_SQL_EXEC())
        .neutral()
        .icon_content(icons::stateful_icon(icons::PLAY))
        .enabled_when(move || connected.get() && !sql_query.get().trim().is_empty())
        .on_click(move |_| {
            let sql = sql_query.get().trim().to_string();
            if sql.is_empty() {
                super::toast::err(lang::ERR_EMPTY_SQL());
                return;
            }
            // 只读模式：拦截写语句（防手滑），SELECT/WITH/EXPLAIN 等照常放行
            if readonly.get() && core::db::is_write_sql(&sql) {
                super::toast::err(lang::SQL_READONLY_BLOCKED());
                return;
            }
            // 记入执行历史（去重提到最新 + 裁剪旧条目，尽力而为）；管理弹窗开着则同步刷新
            if core::store::sql_history_add(hist_key, &sql).is_ok() {
                mgr_refresh.update(|v| v[0] += 1);
            }
            // 后台线程执行 SQL，结果经 channel 回 UI 线程
            core::db::spawn_execute_sql(sink_opt(tx.clone()), make_source(), sql);
        });

    let clear = Element::button(lang::TURSO_SQL_CLEAR())
        .neutral()
        .icon_content(icons::stateful_icon(icons::TRASH))
        .on_click(move |_| {
            sql_query.set(String::new());
        });

    // 工具栏：标题 + 查询管理入口 + 只读开关 + 状态 + 清空/执行
    let panel = Element::col()
        .width_match()
        .spacing(6)
        .bg_role(Role::SurfaceAlt)
        .corner(8.0)
        .padding(10)
        .child(
            Element::row()
                .width_match()
                .spacing(8)
                .cross(Align::Center)
                .child(
                    Element::label(lang::TURSO_SQL_GROUP())
                        .font_size(13.0)
                        .font_weight(700),
                )
                .child(Element::flex_spacer())
                .child(
                    Element::label(lang::SQL_READONLY())
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                )
                .child(readonly_sw)
                .child(
                    Element::button(lang::SQL_MGR_BTN())
                        .small()
                        .neutral()
                        .icon_content(icons::stateful_icon(icons::SEARCH))
                        .on_click(move |_| mgr_show.set(true)),
                )
                .child(clear)
                .child(exec),
        )
        .child(
            // SyntaxInput：SQL 语法高亮、多行（回车换行）、超宽行横向滚动。
            // 字号/字族经 style 传给 widget，与文本输入框同一套外观语义。
            Element::leaf()
                .widget(SyntaxInput::new(
                    sql_query,
                    "SELECT * FROM table_name WHERE ...",
                    lexer,
                ))
                .font_family("Consolas")
                .font_size(13.0)
                .width_match()
                .height(96),
        );

    let dialog = build_sql_mgr_dialog(hist_key, mgr_show, mgr_refresh, sql_query);
    (panel, dialog)
}

/// 「查询管理」弹窗：左栏保存的查询（点击回填、逐条删除），右栏执行历史
/// （点击回填、底部清空）。列表数据每次刷新信号变化时从 store 重读，
/// 弹窗体内不持有跨重建状态（规避整树重建死句柄问题）。
fn build_sql_mgr_dialog(
    hist_key: &'static str,
    mgr_show: Signal<bool>,
    mgr_refresh: Signal<Vec<usize>>,
    sql_query: Signal<String>,
) -> Element {
    let mgr_close = mgr_show;

    // 左栏：保存的查询（点击行回填输入框；垃圾桶逐条删除）
    let saved_col = Element::host_signal(mgr_refresh, move |_| {
        let mut rows = Element::col().width_match().spacing(2);
        let list = core::store::saved_sql_list(hist_key).unwrap_or_default();
        let is_empty = list.is_empty();
        for s in list {
            let row_q = sql_query;
            let sql_text = s.sql.clone();
            let (del_id, del_refresh) = (s.id, mgr_refresh);
            rows = rows.child(
                Element::row()
                    .width_match()
                    .height(30)
                    .cross(Align::Center)
                    .spacing(8)
                    .padding_xy(8, 0)
                    .corner(4.0)
                    .clickable()
                    .on_click(move |_| {
                        row_q.set(sql_text.clone());
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
                        Element::button(String::new())
                            .small()
                            .neutral()
                            .icon_content(icons::stateful_icon(icons::TRASH))
                            .on_click(move |_| {
                                if core::store::saved_sql_del(del_id).is_ok() {
                                    del_refresh.update(|v| v[0] += 1);
                                }
                            }),
                    ),
            );
        }
        if is_empty {
            rows = rows.child(
                Element::label(lang::SQL_SAVED_EMPTY())
                    .font_size(12.0)
                    .fg_role(Role::TextMuted),
            );
        }
        Element::scroll().width_match().height_match().child(rows)
    });

    // 右栏：执行历史（点击行回填输入框；清空走底部按钮）
    let hist_col = Element::host_signal(mgr_refresh, move |_| {
        let mut rows = Element::col().width_match().spacing(2);
        let list = core::store::sql_history_list(hist_key).unwrap_or_default();
        for sql in list.iter() {
            let row_q = sql_query;
            let (sql_label, sql_text) = (sql.clone(), sql.clone());
            rows = rows.child(
                Element::row()
                    .width_match()
                    .height(30)
                    .cross(Align::Center)
                    .spacing(8)
                    .padding_xy(8, 0)
                    .corner(4.0)
                    .clickable()
                    .on_click(move |_| {
                        row_q.set(sql_text.clone());
                    })
                    .child(
                        Element::label(sql_label)
                            .font_size(12.0)
                            .fg_role(Role::TextMuted)
                            .weight(1.0)
                            .max_lines(1)
                            .truncate(Truncate::End),
                    ),
            );
        }
        if list.is_empty() {
            rows = rows.child(
                Element::label(lang::SQL_HISTORY_EMPTY())
                    .font_size(12.0)
                    .fg_role(Role::TextMuted),
            );
        }
        Element::scroll().width_match().height_match().child(rows)
    });

    let body = Element::row()
        .width_match()
        .height(360)
        .spacing(12)
        .child(
            Element::col()
                .width(260)
                .height_match()
                .spacing(6)
                .child(
                    Element::label(lang::SQL_SAVED())
                        .font_size(11.0)
                        .font_weight(600)
                        .fg_role(Role::TextMuted),
                )
                .child(saved_col.height_match()),
        )
        .child(
            Element::col()
                .width_match()
                .weight(1.0)
                .height_match()
                .spacing(6)
                .child(
                    Element::label(lang::SQL_HISTORY())
                        .font_size(11.0)
                        .font_weight(600)
                        .fg_role(Role::TextMuted),
                )
                .child(hist_col.height_match()),
        );

    // 底部按钮排：保存当前 SQL / 清空历史 / 弹性 / 关闭
    let footer = Element::row()
        .width_match()
        .spacing(8)
        .child(
            Element::button(lang::SQL_SAVED_ADD())
                .small()
                .enabled_when(move || !sql_query.get().trim().is_empty())
                .on_click({
                    let refresh = mgr_refresh;
                    let query = sql_query;
                    move |_| {
                        let sql = query.get().trim().to_string();
                        if sql.is_empty() {
                            return;
                        }
                        // 名称 = 首行去空白截 24 字符
                        let first = sql.lines().next().unwrap_or("").trim();
                        let mut name: String = first.chars().take(24).collect();
                        if name.is_empty() {
                            name = lang::SQL_SAVED_UNTITLED();
                        }
                        if core::store::saved_sql_add(hist_key, &name, &sql).is_ok() {
                            refresh.update(|v| v[0] += 1);
                            super::toast::ok(lang::SQL_SAVED_DONE(name));
                        }
                    }
                }),
        )
        .child(
            Element::button(lang::SQL_HISTORY_CLEAR_BTN())
                .small()
                .neutral()
                .on_click({
                    let refresh = mgr_refresh;
                    move |_| {
                        let _ = core::store::sql_history_clear(hist_key);
                        refresh.update(|v| v[0] += 1);
                        super::toast::ok(lang::SQL_HISTORY_CLEARED());
                    }
                }),
        )
        .child(Element::flex_spacer())
        .child(
            Element::button(lang::DT_CLOSE())
                .small()
                .on_click(move |_| {
                    mgr_close.set(false);
                }),
        );

    Element::dialog_panel(
        mgr_show,
        lang::SQL_MGR_TITLE(),
        680,
        move |_| mgr_close.set(false),
        body,
        footer,
    )
}
