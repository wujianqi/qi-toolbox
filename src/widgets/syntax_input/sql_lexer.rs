//! SQL 词法分析器 — 多方言子集（SQLite/Turso、MySQL、PostgreSQL），UTF-8 安全。

/// SQL 方言：影响关键字集与引号/注释规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// SQLite / Turso（默认）
    Sqlite,
    /// MySQL：反引号标识符、# 单行注释、双引号也是字符串
    MySql,
    /// PostgreSQL：双引号标识符、:: 强转、更多关键字
    Postgres,
}

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
    "select",
    "from",
    "where",
    "and",
    "or",
    "not",
    "in",
    "is",
    "null",
    "as",
    "on",
    "join",
    "left",
    "right",
    "inner",
    "outer",
    "cross",
    "full",
    "group",
    "by",
    "order",
    "having",
    "limit",
    "offset",
    "union",
    "all",
    "insert",
    "into",
    "values",
    "update",
    "set",
    "delete",
    // DDL
    "create",
    "table",
    "index",
    "view",
    "database",
    "drop",
    "alter",
    "add",
    "column",
    "primary",
    "key",
    "foreign",
    "references",
    "unique",
    "default",
    "constraint",
    "check",
    "if",
    "not",
    "exists",
    "replace",
    // 表达式
    "case",
    "when",
    "then",
    "else",
    "end",
    "between",
    "like",
    "glob",
    "escape",
    "limit",
    "collate",
    "using",
    "natural",
    // 聚合 / 窗口
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "distinct",
    "asc",
    "desc",
    "nulls",
    "first",
    "last",
    "filter",
    "over",
    "partition",
    "rows",
    "range",
    "groups",
    "current",
    "row",
    "unbounded",
    "preceding",
    "following",
    "exclude",
    "no",
    "others",
    "ties",
    // 类型
    "integer",
    "text",
    "real",
    "blob",
    "numeric",
    "boolean",
    "varchar",
    "char",
    "int",
    "bigint",
    "smallint",
    "float",
    "double",
    "date",
    "datetime",
    "timestamp",
    "time",
    "json",
    // 事务
    "begin",
    "commit",
    "rollback",
    "transaction",
    "deferred",
    "immediate",
    "exclusive",
    // 其他
    "explain",
    "analyze",
    "pragma",
    "with",
    "recursive",
    "true",
    "false",
    "auto_increment",
    "unsigned",
    "signed",
    "vacuum",
    "reindex",
    "conflict",
    "abort",
    "rollback",
    "fail",
    "ignore",
    // Turso / libSQL 扩展
    "returning",
    "strict",
    "without",
    "rowid",
    "always",
    "generated",
    "stored",
];

/// MySQL 特有关键字（在 SQLITE_KEYWORDS 之外）。
const MYSQL_KEYWORDS: &[&str] = &[
    "show",
    "databases",
    "use",
    "describe",
    "desc",
    "truncate",
    "engine",
    "charset",
    "collate",
    "auto_increment",
    "unsigned",
    "zerofill",
    "lock",
    "unlock",
    "duplicate",
    "ignore",
    "force",
    "straight_join",
    "sql_small_result",
    "sql_big_result",
    "sql_buffer_result",
    "sql_no_cache",
    "sql_cache",
    "procedure",
    "trigger",
    "delimiter",
    "utf8mb4",
    "mediumint",
    "tinyint",
    "enum",
    "binary",
    "autoinc",
    "last_insert_id",
    "now",
    "ifnull",
    "concat",
    "group_concat",
    "instr",
    "lpad",
    "rpad",
    "substr",
    "substring",
];

/// PostgreSQL 特有关键字（在 SQLITE_KEYWORDS 之外）。
const PG_KEYWORDS: &[&str] = &[
    "schema",
    "schemas",
    "grant",
    "revoke",
    "truncate",
    "ilike",
    "similar",
    "returning",
    "on_conflict",
    "do",
    "nothing",
    "lateral",
    "only",
    "using",
    "cast",
    "extract",
    "overlay",
    "position",
    "substring",
    "trimming",
    "serial",
    "bigserial",
    "smallserial",
    "uuid",
    "jsonb",
    "bytea",
    "inet",
    "cidr",
    "macaddr",
    "interval",
    "timestamptz",
    "timetz",
    "money",
    "xml",
    "array",
    "any",
    "some",
    "all",
    "exists",
    "unknown",
    "vacuum",
    "analyze",
    "comment",
    "on",
    "materialized",
    "view",
    "sequence",
    "extension",
    "coalesce",
    "nullif",
    "greatest",
    "least",
    "now",
    "string_agg",
    "array_agg",
    "row_number",
    "rank",
    "dense_rank",
    "ntile",
    "lag",
    "lead",
];

fn is_keyword_in(set: &[&str], w: &str) -> bool {
    set.iter().any(|k| k.eq_ignore_ascii_case(w))
}

fn is_keyword(w: &str) -> bool {
    // 关键字表全为小写 ASCII：逐项 ASCII 不区分大小写比较即可，避免旧实现
    // 每个词先 to_lowercase 分配一次 String（打字时每词一次，纯白付）。
    // 表里没有非 ASCII 关键字，Unicode 大小写（如土耳其 İ）无需考虑。
    SQLITE_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(w))
}

/// 按方言分词：
/// - MySQL：反引号 `` ` `` 标识符、`#` 单行注释、双引号按字符串处理
/// - PostgreSQL：双引号标识符（与 SQLite 相同）、`::` 强转按运算符
/// - SQLite/Turso：基线行为
pub fn tokenize_dialect(sql: &str, dialect: Dialect) -> Vec<Token<'_>> {
    let keywords: &[&str] = match dialect {
        Dialect::Sqlite => &[],
        Dialect::MySql => MYSQL_KEYWORDS,
        Dialect::Postgres => PG_KEYWORDS,
    };
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
                tokens.push(Token {
                    kind: SqlTokenKind::Whitespace,
                    text: &sql[start..end],
                });
            }
            '\n' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Newline,
                    text: &sql[byte..end],
                });
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
                tokens.push(Token {
                    kind: SqlTokenKind::String,
                    text: &sql[start..end],
                });
            }
            // 双引号标识符
            '"' => {
                let start = byte;
                for (_, c) in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Identifier,
                    text: &sql[start..end],
                });
            }
            // [ ] 方括号标识符（SQLite 兼容）
            '[' => {
                let start = byte;
                for (_, c) in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Identifier,
                    text: &sql[start..end],
                });
            }
            // -- 单行注释
            '-' if chars.peek().map(|&(_, c)| c) == Some('-') => {
                let start = byte;
                chars.next();
                for (_, c) in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Comment,
                    text: &sql[start..end],
                });
            }
            // /* 多行注释 */
            '/' if chars.peek().map(|&(_, c)| c) == Some('*') => {
                let start = byte;
                chars.next();
                let mut prev = '\0';
                for (_, c) in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Comment,
                    text: &sql[start..end],
                });
            }
            // 数字
            '0'..='9' => {
                let start = byte;
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek().map(|&(_, c)| c) == Some('e')
                    || chars.peek().map(|&(_, c)| c) == Some('E')
                {
                    chars.next();
                    if chars.peek().map(|&(_, c)| c) == Some('+')
                        || chars.peek().map(|&(_, c)| c) == Some('-')
                    {
                        chars.next();
                    }
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_ascii_digit() {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Number,
                    text: &sql[start..end],
                });
            }
            // 标识符 / 关键字
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = byte;
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                let word = &sql[start..end];
                let kind = if is_keyword(word) || is_keyword_in(keywords, word) {
                    SqlTokenKind::Keyword
                } else {
                    SqlTokenKind::Identifier
                };
                tokens.push(Token { kind, text: word });
            }
            // MySQL 反引号标识符（方言分支：其它方言按 Unknown 基线处理）
            '`' if dialect == Dialect::MySql => {
                let start = byte;
                for (_, c) in chars.by_ref() {
                    if c == '`' {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Identifier,
                    text: &sql[start..end],
                });
            }
            // MySQL # 单行注释（方言分支）
            '#' if dialect == Dialect::MySql => {
                let start = byte;
                for (_, c) in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Comment,
                    text: &sql[start..end],
                });
            }
            // 运算符
            '=' | '<' | '>' | '!' | '+' | '-' | '*' | '%' | '|' | '&' => {
                let start = byte;
                if let Some(&(_, next)) = chars.peek() {
                    match (ch, next) {
                        ('<', '>')
                        | ('=', '=')
                        | ('!', '=')
                        | ('<', '=')
                        | ('>', '=')
                        | ('<', '<')
                        | ('>', '>')
                        | ('-', '-') => {
                            chars.next();
                        }
                        _ => {}
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Operator,
                    text: &sql[start..end],
                });
            }
            '(' | ')' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Paren,
                    text: &sql[byte..end],
                });
            }
            ';' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Semicolon,
                    text: &sql[byte..end],
                });
            }
            _ => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(sql.len());
                tokens.push(Token {
                    kind: SqlTokenKind::Unknown,
                    text: &sql[byte..end],
                });
            }
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SQLite 基线方言的分词（测试简写）。
    fn tokenize(sql: &str) -> Vec<Token<'_>> {
        tokenize_dialect(sql, Dialect::Sqlite)
    }

    #[test]
    fn sqlite_keywords() {
        let tokens = tokenize("SELECT * FROM users WHERE id = 1 LIMIT 10");
        let keywords: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::Keyword)
            .map(|t| t.text)
            .collect();
        assert_eq!(keywords, vec!["SELECT", "FROM", "WHERE", "LIMIT"]);
    }

    #[test]
    fn turso_returning() {
        let tokens = tokenize("INSERT INTO t VALUES (1) RETURNING *");
        let keywords: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::Keyword)
            .map(|t| t.text)
            .collect();
        assert!(keywords.contains(&"RETURNING"));
    }

    #[test]
    fn string_literal() {
        let tokens = tokenize("name = 'hello''world'");
        let strings: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::String)
            .map(|t| t.text)
            .collect();
        assert_eq!(strings, vec!["'hello''world'"]);
    }

    #[test]
    fn bracket_identifier() {
        let tokens = tokenize("SELECT [column name] FROM [table]");
        let idents: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::Identifier)
            .map(|t| t.text)
            .collect();
        assert_eq!(idents, vec!["[column name]", "[table]"]);
    }

    #[test]
    fn cjk_not_panics() {
        let _ = tokenize("SELECT 1 -- 答案");
    }

    #[test]
    fn mysql_backtick_and_hash_comment() {
        let tokens = tokenize_dialect(
            "SELECT `user name` FROM t # 备注\nWHERE id = 1",
            Dialect::MySql,
        );
        let idents: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::Identifier)
            .map(|t| t.text)
            .collect();
        assert!(idents.contains(&"`user name`"));
        let comments: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::Comment)
            .map(|t| t.text)
            .collect();
        assert_eq!(comments, vec!["# 备注\n"]);
        // MySQL 特有关键字命中
        let kws: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::Keyword)
            .map(|t| t.text)
            .collect();
        assert!(kws.contains(&"SELECT"));
    }

    #[test]
    fn pg_keywords_dialect() {
        let tokens = tokenize_dialect(
            "SELECT jsonb_col::jsonb FROM t WHERE a ILIKE '%x'",
            Dialect::Postgres,
        );
        let kws: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == SqlTokenKind::Keyword)
            .map(|t| t.text)
            .collect();
        assert!(!kws.contains(&"jsonb_col"));
        assert!(kws.contains(&"ILIKE"));
        assert!(kws.contains(&"FROM"));
        // 基线方言下 ILIKE 不是关键字
        let base = tokenize("SELECT a ILIKE '%x' FROM t");
        assert!(!base
            .iter()
            .any(|t| t.kind == SqlTokenKind::Keyword && t.text.eq_ignore_ascii_case("ilike")));
    }
}
