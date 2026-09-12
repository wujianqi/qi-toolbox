//! 语法高亮编辑器临时测试入口。
//!
//! 用法：在 main.rs 中临时调用 `test_harness::run_test()` 即可看到效果。
//! 测试完毕后删除此文件和 main.rs 中的调用即可。

use windui::prelude::*;
use windui::signal::signal;

use super::{LexerKind, SyntaxInput};

#[allow(dead_code)] // 临时手动测试入口，仅在 main.rs 中按需调用
pub fn run_test() {
    let sql_text = signal(String::from(
        "SELECT u.id, u.name, u.email\nFROM users u\nINNER JOIN orders o ON o.user_id = u.id\nWHERE u.active = 1 AND o.total > 100\nORDER BY o.created_at DESC\nLIMIT 10;",
    ));

    let shell_text = signal(String::from(
        "docker ps --format '{{.Names}}\\t{{.Status}}' | grep nginx",
    ));

    let sql_input = SyntaxInput::new(sql_text, "输入 SQL 语句...", LexerKind::Sql);
    let shell_input = SyntaxInput::new(shell_text, "输入 shell 命令...", LexerKind::Shell);

    App::new("语法高亮编辑器测试", 680, 400)
        .content(
            Element::col()
                .fill()
                .padding(20)
                .spacing(16)
                .child(
                    Element::label("SyntaxInput — 语法高亮编辑器测试")
                        .font_size(16.0)
                        .font_weight(700),
                )
                .child(
                    Element::card(
                        "SQL 输入",
                        Element::col()
                            .width_match()
                            .spacing(8)
                            .child(
                                Element::label("关键字紫色 / 字符串橙色 / 注释灰色")
                                    .font_size(11.0)
                                    .fg_role(Role::TextMuted),
                            )
                            .child(Element::leaf().widget(sql_input).width_match().height(32)),
                    ),
                )
                .child(
                    Element::card(
                        "Shell 命令输入",
                        Element::col()
                            .width_match()
                            .spacing(8)
                            .child(
                                Element::label("命令名蓝色 / 参数默认色 / 标志紫色 / 变量红色")
                                    .font_size(11.0)
                                    .fg_role(Role::TextMuted),
                            )
                            .child(Element::leaf().widget(shell_input).width_match().height(32)),
                    ),
                ),
        )
        .run();
}
