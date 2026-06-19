//! SQL 语法高亮编辑器 — 独立模块，供 Turso 数据库页和密码页共用

use crate::{turso_viewer, strings::lang};
use egui::{Color32, FontId, RichText, TextEdit, TextFormat};
use egui::text::LayoutJob;

/// SQL 语法着色 — 基于 egui LayoutJob，零外部依赖
/// 高亮: 关键字(蓝) / 字符串(红) / 数字(绿) / 注释(绿) / 普通(黑)
fn highlighted_layout(text: &str) -> LayoutJob {
    let font = FontId::new(13.0, egui::FontFamily::Monospace);
    let mut job = LayoutJob::default();

    let color_keyword = Color32::from_rgb(0, 0, 200);
    let color_string  = Color32::from_rgb(163, 21, 21);
    let color_number  = Color32::from_rgb(9, 134, 88);
    let color_comment = Color32::from_rgb(0, 128, 0);
    let color_default = Color32::from_rgb(0, 0, 0);

    let keywords: &[&str] = &[
        "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE",
        "SET", "DELETE", "CREATE", "TABLE", "ALTER", "DROP", "INDEX",
        "AND", "OR", "NOT", "IN", "LIKE", "BETWEEN", "IS", "NULL",
        "AS", "ON", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "FULL",
        "GROUP", "BY", "ORDER", "ASC", "DESC", "HAVING", "LIMIT",
        "OFFSET", "UNION", "ALL", "DISTINCT", "COUNT", "SUM", "AVG",
        "MIN", "MAX", "CASE", "WHEN", "THEN", "ELSE", "END",
        "PRAGMA", "TABLE_INFO", "EXPLAIN", "BEGIN", "COMMIT", "ROLLBACK",
    ];

    let mut chars = text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        match ch {
            '-' if chars.peek().map_or(false, |&(_, c)| c == '-') => {
                let start = i;
                while let Some(&(_, c)) = chars.peek() {
                    if c == '\n' { break; }
                    chars.next();
                }
                let end = chars.peek().map_or(text.len(), |&(ei, _)| ei);
                job.append(&text[start..end], 0.0, TextFormat {
                    font_id: font.clone(), color: color_comment, ..Default::default()
                });
            }
            '\'' => {
                let start = i;
                loop {
                    match chars.next() {
                        Some((_, '\'')) => {
                            if chars.peek().map_or(false, |&(_, c)| c == '\'') {
                                chars.next();
                            } else { break; }
                        }
                        Some((_, '\n')) | None => break,
                        _ => {}
                    }
                }
                let end = chars.peek().map_or(text.len(), |&(ei, _)| ei);
                job.append(&text[start..end], 0.0, TextFormat {
                    font_id: font.clone(), color: color_string, ..Default::default()
                });
            }
            '0'..='9' => {
                let start = i;
                while chars.peek().map_or(false, |&(_, c)| c.is_ascii_digit() || c == '.') {
                    chars.next();
                }
                let end = chars.peek().map_or(text.len(), |&(ei, _)| ei);
                job.append(&text[start..end], 0.0, TextFormat {
                    font_id: font.clone(), color: color_number, ..Default::default()
                });
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = i;
                while chars.peek().map_or(false, |&(_, c)| c.is_alphanumeric() || c == '_') {
                    chars.next();
                }
                let end = chars.peek().map_or(text.len(), |&(ei, _)| ei);
                let word = &text[start..end];
                let upper = word.to_uppercase();
                let color = if keywords.iter().any(|&kw| kw == upper.as_str()) {
                    color_keyword
                } else { color_default };
                job.append(word, 0.0, TextFormat {
                    font_id: font.clone(), color, ..Default::default()
                });
            }
            _ => {
                job.append(&text[i..i + ch.len_utf8()], 0.0, TextFormat {
                    font_id: font.clone(), color: color_default, ..Default::default()
                });
            }
        }
    }
    job
}

/// TextEdit layouter 回调 — 供 egui TextEdit::layouter() 使用
pub fn sql_layouter(ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32) -> std::sync::Arc<egui::text::Galley> {
    let mut job = highlighted_layout(text.as_str());
    job.wrap.max_width = wrap_width;
    ui.painter().layout_job(job)
}

/// 渲染 SQL 查询面板（执行按钮 + 多行输入框 + 状态栏）
pub fn render_sql_panel(
    viewer: &mut turso_viewer::TursoViewer,
    ui: &mut egui::Ui,
    sql_query: &mut String,
    sql_status: &mut String,
) {
    ui.group(|ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(lang::TURSO_SQL_GROUP).size(13.0).strong());
            let sql_connected = !viewer.tables.is_empty();
            if ui.add_enabled(
                sql_connected && !sql_query.trim().is_empty(),
                egui::Button::new(RichText::new(lang::TURSO_SQL_EXEC)),
            ).clicked() {
                ui.ctx().request_repaint();
                match viewer.execute_sql(sql_query.trim()) {
                    Ok(()) => {
                        let rows = viewer.row_count;
                        let cols = viewer.column_names.len();
                        *sql_status = lang::TURSO_SQL_STATUS
                            .replace("{}", &cols.to_string())
                            .replacen("{}", &rows.to_string(), 1);
                    }
                    Err(e) => {
                        *sql_status = format!("\u{274C} {}", e);
                    }
                }
            }
            if ui.button(RichText::new(lang::TURSO_SQL_CLEAR)).clicked() {
                sql_query.clear();
                sql_status.clear();
            }
            if !sql_status.is_empty() {
                ui.label(RichText::new(sql_status.as_str()).size(11.0).weak());
            }
        });
        ui.add_space(4.0);
        let available_width = ui.available_width();
        ui.add(
            TextEdit::multiline(sql_query)
                .layouter(&mut sql_layouter)
                .desired_width(available_width)
                .desired_rows(4)
                .font(FontId::new(13.0, egui::FontFamily::Monospace)),
        );
    });
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);
}
