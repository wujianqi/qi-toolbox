//! Shell 命令词法分析器 — UTF-8 安全，按 char 迭代 + 字节偏移追踪。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellTokenKind {
    Operator,
    String,
    Variable,
    Flag,
    Command,
    Argument,
    Comment,
    Whitespace,
    Newline,
}

#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub kind: ShellTokenKind,
    pub text: &'a str,
}

pub fn tokenize(line: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::with_capacity(line.len() / 3);
    let mut chars = line.char_indices().peekable();
    let mut first_token = true;

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
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Whitespace,
                    text: &line[start..end],
                });
            }
            '\n' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Newline,
                    text: &line[byte..end],
                });
                first_token = true;
            }
            '#' => {
                let start = byte;
                for (_, c) in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Comment,
                    text: &line[start..end],
                });
                first_token = false;
            }
            '|' => {
                let start = byte;
                if chars.peek().map(|&(_, c)| c) == Some('|') {
                    chars.next();
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Operator,
                    text: &line[start..end],
                });
                first_token = false;
            }
            '>' | '<' => {
                let start = byte;
                if chars.peek().map(|&(_, c)| c) == Some(ch) {
                    chars.next();
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Operator,
                    text: &line[start..end],
                });
                first_token = false;
            }
            ';' => {
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Operator,
                    text: &line[byte..end],
                });
                first_token = false;
            }
            '&' => {
                let start = byte;
                if chars.peek().map(|&(_, c)| c) == Some('&') {
                    chars.next();
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Operator,
                    text: &line[start..end],
                });
                first_token = false;
            }
            '$' => {
                let start = byte;
                if chars.peek().map(|&(_, c)| c) == Some('{') {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                    }
                } else {
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_alphanumeric() || c == '_' {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Variable,
                    text: &line[start..end],
                });
                first_token = false;
            }
            '\'' | '"' => {
                let quote = ch;
                let start = byte;
                while let Some((_, c)) = chars.next() {
                    if c == quote {
                        break;
                    }
                    if c == '\\' {
                        chars.next();
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::String,
                    text: &line[start..end],
                });
                first_token = false;
            }
            '-' if chars
                .peek()
                .map(|&(_, c)| c)
                .is_some_and(|c| c.is_alphanumeric()) =>
            {
                let start = byte;
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_alphanumeric() || c == '-' || c == '_' || c == '=' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                tokens.push(Token {
                    kind: ShellTokenKind::Flag,
                    text: &line[start..end],
                });
                first_token = false;
            }
            _ => {
                let start = byte;
                while let Some(&(_, c)) = chars.peek() {
                    if c == ' '
                        || c == '\t'
                        || c == '\n'
                        || c == '|'
                        || c == ';'
                        || c == '&'
                        || c == '>'
                        || c == '<'
                        || c == '"'
                        || c == '\''
                        || c == '$'
                    {
                        break;
                    }
                    chars.next();
                }
                let end = chars.peek().map(|&(b, _)| b).unwrap_or(line.len());
                let kind = if first_token {
                    ShellTokenKind::Command
                } else {
                    ShellTokenKind::Argument
                };
                tokens.push(Token {
                    kind,
                    text: &line[start..end],
                });
                first_token = false;
            }
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_command() {
        let tokens = tokenize("ls -la /tmp");
        let kinds: Vec<ShellTokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&ShellTokenKind::Command));
        assert!(kinds.contains(&ShellTokenKind::Flag));
    }

    #[test]
    fn pipe() {
        let tokens = tokenize("ps aux | grep nginx");
        let ops: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == ShellTokenKind::Operator)
            .map(|t| t.text)
            .collect();
        assert_eq!(ops, vec!["|"]);
    }

    #[test]
    fn variable() {
        let tokens = tokenize("echo $HOME ${USER}");
        let vars: Vec<&str> = tokens
            .iter()
            .filter(|t| t.kind == ShellTokenKind::Variable)
            .map(|t| t.text)
            .collect();
        assert_eq!(vars, vec!["$HOME", "${USER}"]);
    }
}
