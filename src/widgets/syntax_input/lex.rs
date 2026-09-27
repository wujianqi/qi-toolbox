//! 语法高亮词法接口（从 `mod.rs` 拆出）：token 模型 + 各词法器的统一转换层。
//!
//! 具体分词实现在 [`super::sql_lexer`] / [`super::shell_lexer`]；本层把它们的
//! token 映射到控件共用的 [`TokenKind`]，并按 [`LexerKind`] 选择 SQL 方言。

use super::{shell_lexer, sql_lexer};

/// 词法分析器：将文本切分为带颜色类型的 token 列表。
#[derive(Debug, Clone)]
pub struct LexToken<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    String,
    Number,
    Comment,
    Operator,
    Flag,
    Command,
    Variable,
    Plain,
}

/// SQL 词法分析 → LexToken 转换（SQLite/Turso 基线方言）。
pub fn lex_sql(text: &str) -> Vec<LexToken<'_>> {
    lex_sql_dialect(text, sql_lexer::Dialect::Sqlite)
}

/// SQL 词法分析 → LexToken 转换（指定方言：MySQL 反引号/# 注释，PG 关键字）。
pub fn lex_sql_dialect(text: &str, dialect: sql_lexer::Dialect) -> Vec<LexToken<'_>> {
    sql_lexer::tokenize_dialect(text, dialect)
        .into_iter()
        .map(|t| LexToken {
            kind: match t.kind {
                sql_lexer::SqlTokenKind::Keyword => TokenKind::Keyword,
                sql_lexer::SqlTokenKind::String => TokenKind::String,
                sql_lexer::SqlTokenKind::Number => TokenKind::Number,
                sql_lexer::SqlTokenKind::Comment => TokenKind::Comment,
                sql_lexer::SqlTokenKind::Operator => TokenKind::Operator,
                sql_lexer::SqlTokenKind::Identifier
                | sql_lexer::SqlTokenKind::Paren
                | sql_lexer::SqlTokenKind::Semicolon => TokenKind::Plain,
                sql_lexer::SqlTokenKind::Whitespace
                | sql_lexer::SqlTokenKind::Newline
                | sql_lexer::SqlTokenKind::Unknown => TokenKind::Plain,
            },
            text: t.text,
        })
        .collect()
}

/// Shell 词法分析 → LexToken 转换。
pub fn lex_shell(text: &str) -> Vec<LexToken<'_>> {
    shell_lexer::tokenize(text)
        .into_iter()
        .map(|t| LexToken {
            kind: match t.kind {
                shell_lexer::ShellTokenKind::Command => TokenKind::Command,
                shell_lexer::ShellTokenKind::Flag => TokenKind::Flag,
                shell_lexer::ShellTokenKind::String => TokenKind::String,
                shell_lexer::ShellTokenKind::Variable => TokenKind::Variable,
                shell_lexer::ShellTokenKind::Operator => TokenKind::Operator,
                shell_lexer::ShellTokenKind::Comment => TokenKind::Comment,
                shell_lexer::ShellTokenKind::Argument => TokenKind::Plain,
                shell_lexer::ShellTokenKind::Whitespace | shell_lexer::ShellTokenKind::Newline => {
                    TokenKind::Plain
                }
            },
            text: t.text,
        })
        .collect()
}

/// 词法分析器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerKind {
    /// SQLite / Turso
    Sql,
    /// MySQL 方言（反引号标识符、# 注释、MySQL 关键字）
    SqlMySql,
    /// PostgreSQL 方言（PG 关键字）
    SqlPg,
    Shell,
}
