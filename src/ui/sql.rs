//! SQL 查询面板（windui 版，UI 层）
//!
//! 多行输入（`SyntaxInput`，SQL 语法高亮、等宽字体）+ 执行/清空按钮 + 状态栏。
//! SQL 在后台线程执行（`core::db::spawn_execute_sql`），结果经 channel 回 UI 线程。

use std::cell::RefCell;
use std::rc::Rc;

use windui::prelude::*;

use super::icons;
use super::sink;
use crate::core;
use crate::core::db::DbSource;
use crate::lang;
use crate::widgets::syntax_input::{LexerKind, SyntaxInput};

/// 执行历史下拉选中索引的信号池：按 `hist_key` 复用 thread_local 槽位，永不随
/// 重建回收。不能在 `render_sql_panel` 里 `signal(0)` 现建——主题切换整树重建会
/// dispose 构建期信号，下拉持有的句柄即成死句柄，再读即 panic（signal.rs:545）。
/// 同一连接源每次构建取池中信号并复位为 0；池按需扩容，缩容不回收。
fn hist_sel_sig(hist_key: &'static str) -> Signal<usize> {
    use std::cell::RefCell;
    thread_local! {
        static POOL: RefCell<Vec<(&'static str, Signal<usize>)>> = RefCell::new(Vec::new());
    }
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        // 自愈：首建若发生在整树重建的 SignalScope 内会被收走（is_alive=false），
        // 此时换新槽位重建，避免残留死句柄（读取即 panic）。
        if let Some(pos) = p.iter().position(|(k, _)| *k == hist_key) {
            if !p[pos].1.is_alive() {
                p[pos].1 = signal(0usize);
            }
            return p[pos].1;
        }
        let s = signal(0usize);
        p.push((hist_key, s));
        s
    })
}

/// 渲染 SQL 查询面板（执行按钮 + 多行输入框 + 状态栏 + 执行历史下拉）
/// `make_source` 返回统一连接源（Turso/MySQL/PG 三后端由 core::db 分发）。
/// `lexer` 指定 SQL 方言高亮（Turso=Sql、MySQL=SqlMySql、PG=SqlPg）。
/// `hist_key` 为历史存储键（按连接源区分，如 "turso" / "mysql" / "pg"）。
pub fn render_sql_panel(
    sql_query: Signal<String>,
    sql_status: Signal<String>,
    connected: Signal<bool>,
    make_source: impl Fn() -> DbSource + 'static,
    tx: Sender<core::db::DbMsg>,
    lexer: LexerKind,
    hist_key: &'static str,
) -> Element {
    // 执行历史（store.db sql_history 表，按连接源区分；新→旧）
    let history: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(
        core::store::sql_history_list(hist_key).unwrap_or_default(),
    ));
    let hist_sel = hist_sel_sig(hist_key);
    let hist_len = history.borrow().len();
    let hist_opts = hist_sel.map({
        let history = history.clone();
        move |idx: &usize| {
            let list = history.borrow();
            if list.is_empty() {
                vec![lang::SQL_HISTORY_EMPTY()]
            } else {
                vec![list
                    .get((*idx).min(list.len().saturating_sub(1)))
                    .cloned()
                    .unwrap_or_default()]
            }
        }
    });
    let _ = hist_len; // 下拉显示当前选中项；列表在每次执行后刷新

    // 历史下拉选中即回填输入框（方便重跑旧查询）
    let hist_pick = {
        let history = history.clone();
        let sql_query = sql_query.clone();
        move |_: &mut windui::core::EventCtx| {
            let list = history.borrow();
            if let Some(sql) = list.get(hist_sel.get()) {
                sql_query.set(sql.clone());
            }
        }
    };
    let hist_dropdown = Element::dropdown_signal(hist_opts, hist_sel)
        .width(120)
        .enabled_when({
            let history = history.clone();
            move || !history.borrow().is_empty()
        })
        .on_click(hist_pick);
    let hist_clear = Element::button(lang::SQL_HISTORY_CLEAR_BTN())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::TRASH, Some(14)))
        .enabled_when({
            let history = history.clone();
            move || !history.borrow().is_empty()
        })
        .on_click({
            let history = history.clone();
            let sql_status = sql_status.clone();
            move |_| {
                let _ = core::store::sql_history_clear(hist_key);
                history.borrow_mut().clear();
                hist_sel.set(0);
                sql_status.set(lang::SQL_HISTORY_CLEARED());
            }
        });

    let exec = Element::button(lang::TURSO_SQL_EXEC())
        .neutral()
        .icon_content(icons::stateful_icon(icons::PLAY, Some(16)))
        .enabled_when(move || connected.get() && !sql_query.get().trim().is_empty())
        .on_click(move |_| {
            let sql = sql_query.get().trim().to_string();
            if sql.is_empty() {
                sql_status.set(lang::ERR_EMPTY_SQL().to_string());
                return;
            }
            // 记入执行历史（去重提到最新 + 裁剪旧条目，尽力而为）
            if core::store::sql_history_add(hist_key, &sql).is_ok() {
                *history.borrow_mut() = core::store::sql_history_list(hist_key).unwrap_or_default();
                hist_sel.set(0);
            }
            // 后台线程执行 SQL，结果经 channel 回 UI 线程
            core::db::spawn_execute_sql(sink(tx.clone()), make_source(), sql);
        });

    let clear = Element::button(lang::TURSO_SQL_CLEAR())
        .neutral()
        .icon_content(icons::stateful_icon(icons::TRASH, Some(16)))
        .on_click(move |_| {
            sql_query.set(String::new());
            sql_status.set(String::new());
        });

    Element::col()
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
                    Element::label(lang::SQL_HISTORY())
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                )
                .child(hist_dropdown)
                .child(hist_clear)
                .child(
                    Element::label_signal(sql_status)
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
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
        )
}
