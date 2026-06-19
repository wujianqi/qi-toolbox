//! 数据表列表与数据表格渲染

use crate::turso_viewer;
use egui::{RichText, ScrollArea};
use egui::scroll_area::ScrollBarVisibility;
use std::rc::Rc;

/// 左侧表列表
pub fn render_table_list(viewer: &mut turso_viewer::TursoViewer, ui: &mut egui::Ui, height: f32) {
    ui.vertical(|ui| {
        ui.set_min_width(180.0);
        ui.set_max_width(180.0);

        ui.label(
            RichText::new(format!("表列表 ({})", viewer.tables.len()))
                .size(13.0)
                .strong(),
        );
        ui.add_space(4.0);

        ScrollArea::vertical()
            .id_salt("table_list")
            .max_height(height - 30.0)
            .show(ui, |ui| {
                let selected = viewer.selected_table.clone();
                let tables: Vec<String> = viewer.tables.clone();

                for table_name in &tables {
                    let is_selected = selected.as_deref() == Some(table_name.as_str());
                    let text = if is_selected {
                        RichText::new(format!("\u{25B6} {}", table_name)).strong()
                    } else {
                        RichText::new(format!("  {}", table_name))
                    };

                    if ui
                        .add(egui::Button::new(text).frame(false))
                        .clicked()
                        && !is_selected
                    {
                        match viewer.set_selected_table(table_name.clone()) {
                            Ok(()) => {}
                            Err(e) => {
                                viewer.error_message = Some(e);
                            }
                        }
                    }
                }
            });
    });
}

/// 右侧数据表格（含表头、分页、列表/详情视图切换）
pub fn render_data_table(viewer: &mut turso_viewer::TursoViewer, ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        if viewer.column_names.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.label(
                    RichText::new("请先连接数据库并选择一个表")
                        .size(13.0)
                        .weak(),
                );
            });
            return;
        }

        // 表头信息
        ui.horizontal(|ui| {
            let table_name = viewer
                .selected_table
                .as_deref()
                .unwrap_or("");
            ui.label(RichText::new(table_name).size(13.0).strong());
            ui.label(
                RichText::new(format!(
                    "| {} 列 | {} 行",
                    viewer.column_names.len(),
                    viewer.row_count,
                ))
                .size(11.0)
                .weak(),
            );
            // 列过滤 popup
            let visible_count = viewer.visible_columns.iter().filter(|&&v| v).count();
            let total = viewer.column_names.len();
            let label = format!("\u{2699} 列 ({}/{})", visible_count, total);
            ui.menu_button(RichText::new(label).size(11.0), |ui| {
                for (i, col_name) in viewer.column_names.iter().enumerate() {
                    if i < viewer.visible_columns.len() {
                        ui.checkbox(&mut viewer.visible_columns[i], col_name.as_str());
                    }
                }
            });
        });

        // 分页控制 - 仅列表视图显示
        let needs_pagination = viewer.row_count > viewer.page_size && viewer.selected_row.is_none();
        if needs_pagination {
            let offset = viewer.page_offset;
            let page_size = viewer.page_size;
            let total = viewer.row_count;
            let page = offset / page_size + 1;
            let total_pages = (total + page_size - 1) / page_size;

            ui.horizontal(|ui| {
                let can_prev = offset > 0;
                let can_next = offset + page_size < total;

                if ui.add_enabled(can_prev, egui::Button::new(RichText::new("\u{25C0} 上一页").size(11.0))).clicked() {
                    let new_offset = offset.saturating_sub(page_size);
                    let _ = viewer.load_page(new_offset);
                }
                ui.label(RichText::new(format!("第 {} / {} 页", page, total_pages)).size(11.0).weak());
                if ui.add_enabled(can_next, egui::Button::new(RichText::new("下一页 \u{25B6}").size(11.0))).clicked() {
                    let new_offset = offset + page_size;
                    let _ = viewer.load_page(new_offset);
                }
            });
        }

        ui.add_space(2.0);

        // 判断当前是列表视图还是详情视图
        if let Some(sel_row) = viewer.selected_row {
            // 详情视图 - 只需要一行数据，用 Rc 包装避免深拷贝
            if sel_row < viewer.table_data.len() {
                ui.horizontal(|ui| {
                    if ui.button("\u{2190} 返回列表").clicked() {
                        viewer.selected_row = None;
                        return;
                    }
                    ui.label(
                        RichText::new(format!("行 {} 详情", sel_row + 1))
                            .size(13.0)
                            .strong(),
                    );
                });
                ui.add_space(6.0);

                // 只 clone 一行 + 列名（轻量）
                let column_names: Vec<Rc<str>> = viewer.column_names.iter().map(|s| Rc::from(s.as_str())).collect();
                let row_data: Vec<Rc<str>> = viewer.table_data[sel_row].iter().map(|s| Rc::from(s.as_str())).collect();
                ScrollArea::both()
                    .id_salt("row_detail_scroll")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded)
                    .show(ui, |ui| {
                        for (col_i, col_name) in column_names.iter().enumerate() {
                            let cell = &row_data[col_i];
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{}:", col_name))
                                        .size(12.0)
                                        .strong(),
                                );
                                let rt = if cell.as_ref() == "NULL" {
                                    RichText::new(cell.as_ref())
                                        .size(12.0)
                                        .color(egui::Color32::from_rgb(150, 150, 150))
                                        .italics()
                                } else {
                                    RichText::new(cell.as_ref()).size(12.0)
                                };
                                ui.label(rt);
                            });
                        }
                    });
            }
        } else {
            // 列表视图 - 用 Rc 包装 table_data，clone 只是指针复制
            let rc_data: Vec<Vec<Rc<str>>> = viewer.table_data
                .iter()
                .map(|row| row.iter().map(|s| Rc::from(s.as_str())).collect())
                .collect();

            let visible_cols: Vec<(usize, &String)> = viewer.column_names
                .iter()
                .enumerate()
                .filter(|(i, _)| viewer.visible_columns.get(*i).copied().unwrap_or(true))
                .collect();
            let num_visible = visible_cols.len();

            if num_visible == 0 {
                return;
            }

            let mut clicked_row: Option<usize> = None;

            ScrollArea::horizontal()
                .id_salt("data_table_scroll")
                .auto_shrink([false, false])
                .scroll_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    egui_extras::TableBuilder::new(ui)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(egui_extras::Column::auto().at_least(40.0))
                        .columns(egui_extras::Column::auto().at_least(80.0), num_visible)
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.label(RichText::new("").size(11.0).strong());
                            });
                            for (_, col_name) in &visible_cols {
                                header.col(|ui| {
                                    ui.label(RichText::new(col_name.as_str()).size(11.0).strong());
                                });
                            }
                        })
                        .body(|body| {
                            let row_height = 18.0;
                            let total_rows = rc_data.len();
                            body.rows(row_height, total_rows, |mut row| {
                                let row_idx = row.index();
                                let row_data = &rc_data[row_idx];

                                row.col(|ui| {
                                    if ui.small_button("\u{1F50D}").clicked() {
                                        clicked_row = Some(row_idx);
                                    }
                                });

                                for (col_i, _) in &visible_cols {
                                    let cell = &row_data[*col_i];
                                    row.col(|ui| {
                                        let rt = if cell.as_ref() == "NULL" {
                                            RichText::new(cell.as_ref())
                                                .size(11.0)
                                                .color(egui::Color32::from_rgb(150, 150, 150))
                                                .italics()
                                        } else {
                                            RichText::new(cell.as_ref()).size(11.0)
                                        };
                                        ui.label(rt);
                                    });
                                }
                            });
                        });
                });

            if let Some(idx) = clicked_row {
                viewer.selected_row = Some(idx);
            }
        }
    });
}
