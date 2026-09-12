//! SQL 词法分析器 — 仅支持 SQLite / Turso 语法子集，UTF-8 安全。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlTokenKind {
    Keyword,
    Identifier,
    String,
    Number,
    Comment,
    Operator,
    Paren,
    Semicolon,
    Whitespace,
    Newline,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub kind: SqlTokenKind,
    pub text: &'a str,
}

/// SQLite / Turso 关键字（精简集）。
const SQLITE_KEYWORDS: &[&str] = &[
    // DML
    "select", "from", "where", "and", "or", "not", "in", "is", "null", "as",
    "on", "join", "left", "right", "inner", "outer", "cross", "full",
    "group", "by", "order", "having", "limit", "offset", "union", "all",
    "insert", "into", "values", "update", "set", "delete",
    // DDL
    "create", "table", "index", "view", "database", "drop", "alter", "add",
    "column", "primary", "key", "foreign", "references", "unique",
    "default", "constraint", "check", "if", "not", "exists", "replace",
    // 表达式
    "case", "when", "then", "else", "end", "between", "like", "glob",
    "escape", "limit", "collate", "using", "natural",
    // 聚合 / 窗口
    "count", "sum", "avg", "min", "max", "distinct", "asc", "desc",
    "nulls", "first", "last", "filter", "over", "partition",
    "rows", "range", "groups", "current", "row", "unbounded",
    "preceding", "following", "exclude", "no", "others", "ties",
    // 类型
    "integer", "text", "real", "blob", "numeric", "boolean",
    "varchar", "char", "int", "bigint", "smallint", "float", "double",
    "date", "datetime", "timestamp", "time", "json",
    // 事务
    "begin", "commit", "rollback", "transaction", "deferred", "immediate", "exclusive",
    // 其他
    "explain", "analyze", "pragma", "with", "recursive",
    "true", "false", "auto_increment", "unsigned", "signed",
    "vacuum", "reindex", "conflict", "abort", "rollback", "fail", "ignore",
    // Turso / libSQL 扩展
    "returning", "strict", "without", "rowid", "always", "generated", "stored",
];

fn is_keyword(w: &str) -> bool {
    // 关键字表全为小写 ASCII：逐项 ASCII 不区分大小写比较即可，避免旧实现
    // 每个词先 to_lowercase 分配一次 String（打字时每词一次，纯白付）。
    // 表里没有非 ASCII 关键字，Unicode 大小写（如土耳其 İ）无需考虑。
    SQLITE_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(w))
}

pub fn tokenize(sql: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::with_capacity(sql.len() / 4);
    let mut chars = sql.char_indices().peekable();

    while let Some((byte, ch)) = chars.next() {
        match ch {
            ' ' | '\t' | '\r' => {
                let start = byte;
                while let Some(&(_, c)) = chars.peek() {
                    if c == ' ' || c == '\t' || c == '\r' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Whitespace, text: &sql[start..end] });
            }
            '\n' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Newline, text: &sql[byte..end] });
            }
            // 单引号字符串
            '\'' => {
                let start = byte;
                loop {
                    match chars.next() {
                        Some((_, '\'')) => {
                            if chars.peek().map(|&(_, c)| c) == Some('\'') {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::String, text: &sql[start..end] });
            }
            // 双引号标识符
            '"' => {
                let start = byte;
                for (_, c) in chars.by_ref() {
                    if c == '"' { break; }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Identifier, text: &sql[start..end] });
            }
            // [ ] 方括号标识符（SQLite 兼容）
            '[' => {
                let start = byte;
                for (_, c) in chars.by_ref() {
                    if c == ']' { break; }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Identifier, text: &sql[start..end] });
            }
            // -- 单行注释
            '-' if chars.peek().map(|&(_, c)| c) == Some('-') => {
                let start = byte;
                chars.next();
                for (_, c) in chars.by_ref() {
                    if c == '\n' { break; }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Comment, text: &sql[start..end] });
            }
            // /* 多行注释 */
            '/' if chars.peek().map(|&(_, c)| c) == Some('*') => {
                let start = byte;
                chars.next();
                let mut prev = '\0';
                for (_, c) in chars.by_ref() {
                    if prev == '*' && c == '/' { break; }
                    prev = c;
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Comment, text: &sql[start..end] });
            }
            // 数字
            '0'..='9' => {
                let start = byte;
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' { chars.next(); } else { break; }
                }
                if chars.peek().map(|&(_, c)| c) == Some('e') || chars.peek().map(|&(_, c)| c) == Some('E') {
                    chars.next();
                    if chars.peek().map(|&(_, c)| c) == Some('+') || chars.peek().map(|&(_, c)| c) == Some('-') {
                        chars.next();
                    }
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_ascii_digit() { chars.next(); } else { break; }
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Number, text: &sql[start..end] });
            }
            // 标识符 / 关键字
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = byte;
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' { chars.next(); } else { break; }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                let word = &sql[start..end];
                let kind = if is_keyword(word) { SqlTokenKind::Keyword } else { SqlTokenKind::Identifier };
                tokens.push(Token { kind, text: word });
            }
            // 运算符
            '=' | '<' | '>' | '!' | '+' | '-' | '*' | '%' | '|' | '&' => {
                let start = byte;
                if let Some(&(_, next)) = chars.peek() {
                    match (ch, next) {
                        ('<', '>') | ('=', '=') | ('!', '=') | ('<', '=') | ('>', '=') | ('<', '<') | ('>', '>') | ('-', '-') => {
                            chars.next();
                        }
                        _ => {}
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Operator, text: &sql[start..end] });
            }
            '(' | ')' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Paren, text: &sql[byte..end] });
            }
            ';' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Semicolon, text: &sql[byte..end] });
            }
            _ => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token { kind: SqlTokenKind::Unknown, text: &sql[byte..end] });
            }
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_keywords() {
        let tokens = tokenize("SELECT * FROM users WHERE id = 1 LIMIT 10");
        let keywords: Vec<&str> = tokens.iter().filter(|t| t.kind == SqlTokenKind::Keyword).map(|t| t.text).collect();
        assert_eq!(keywords, vec!["SELECT", "FROM", "WHERE", "LIMIT"]);
    }

    #[test]
    fn turso_returning() {
        let tokens = tokenize("INSERT INTO t VALUES (1) RETURNING *");
        let keywords: Vec<&str> = tokens.iter().filter(|t| t.kind == SqlTokenKind::Keyword).map(|t| t.text).collect();
        assert!(keywords.contains(&"RETURNING"));
    }

    #[test]
    fn string_literal() {
        let tokens = tokenize("name = 'hello''world'");
        let strings: Vec<&str> = tokens.iter().filter(|t| t.kind == SqlTokenKind::String).map(|t| t.text).collect();
        assert_eq!(strings, vec!["'hello''world'"]);
    }

    #[test]
    fn bracket_identifier() {
        let tokens = tokenize("SELECT [column name] FROM [table]");
        let idents: Vec<&str> = tokens.iter().filter(|t| t.kind == SqlTokenKind::Identifier).map(|t| t.text).collect();
        assert_eq!(idents, vec!["[column name]", "[table]"]);
    }

    #[test]
    fn cjk_not_panics() {
        let _ = tokenize("SELECT 1 -- 答案");
    }
}
