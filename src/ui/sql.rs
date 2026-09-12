//! SQL 查询面板（windui 版，UI 层）
//!
//! 多行输入（`SyntaxInput`，SQL 语法高亮、等宽字体）+ 执行/清空按钮 + 状态栏。
//! SQL 在后台线程执行（`core::db::spawn_execute_sql`），结果经 channel 回 UI 线程。

use windui::prelude::*;

use super::icons;
use super::sink;
use super::syntax_input::{LexerKind, SyntaxInput};
use crate::core;
use crate::core::turso::TursoSource;
use crate::lang;

/// 渲染 SQL 查询面板（执行按钮 + 多行输入框 + 状态栏）
pub fn render_sql_panel(
    sql_query: Signal<String>,
    sql_status: Signal<String>,
    connected: Signal<bool>,
    make_source: impl Fn() -> TursoSource + 'static,
    tx: Sender<core::db::DbMsg>,
) -> Element {
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
                    LexerKind::Sql,
                ))
                .font_family("Consolas")
                .font_size(13.0)
                .width_match()
                .height(96),
        )
}
