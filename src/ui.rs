use crate::{bcrypt_tool, totp_tool, turso_viewer, datatable};
use egui::{CentralPanel, FontId, Panel, RichText, ScrollArea, TextEdit};
use std::sync::Arc;

#[derive(Default, PartialEq, Clone)]
enum AppTab {
    #[default]
    Totp,
    Bcrypt,
    TursoData,
    About,
}

pub struct QiToolboxApp {
    selected_tab: AppTab,
    input_text: String,
    output_text: String,
    sql_output_text: String,
    qr_code_image: Option<Arc<egui::ColorImage>>,
    turso_viewer: turso_viewer::TursoViewer,
}

impl QiToolboxApp {
    pub fn new() -> Self {
        Self {
            selected_tab: AppTab::default(),
            input_text: String::new(),
            output_text: String::new(),
            sql_output_text: String::new(),
            qr_code_image: None,
            turso_viewer: turso_viewer::TursoViewer::default(),
        }
    }

    fn load_saved_key(&mut self) {
        if let Ok(key) = std::fs::read_to_string("qi_key.txt") {
            self.input_text = key.trim().to_string();
        }
    }

    fn generate_random_password(&mut self, length: usize) {
        let lowercase = "abcdefghijklmnopqrstuvwxyz";
        let uppercase = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let digits = "0123456789";
        let special = "!@#$%^&*";

        let mut random_bytes = vec![0u8; length.max(8)];
        getrandom::getrandom(&mut random_bytes).expect("Failed to generate random bytes");

        let mut password = String::new();
        password.push(
            lowercase
                .chars()
                .nth((random_bytes[0] as usize) % lowercase.len())
                .unwrap(),
        );
        password.push(
            uppercase
                .chars()
                .nth((random_bytes[1] as usize) % uppercase.len())
                .unwrap(),
        );
        password.push(
            digits
                .chars()
                .nth((random_bytes[2] as usize) % digits.len())
                .unwrap(),
        );
        password.push(
            special
                .chars()
                .nth((random_bytes[3] as usize) % special.len())
                .unwrap(),
        );

        let all_chars = format!("{}{}{}{}", lowercase, uppercase, digits, special);
        for i in 4..length {
            let idx = (random_bytes[i] as usize) % all_chars.len();
            password.push(all_chars.chars().nth(idx).unwrap());
        }

        let mut chars: Vec<char> = password.chars().collect();
        for i in (1..chars.len()).rev() {
            let j = ((random_bytes[i % random_bytes.len()] as usize) % (i + 1)) as usize;
            chars.swap(i, j);
        }

        self.input_text = chars.into_iter().collect();
    }

    fn update_qr_code(&mut self) {
        if self.selected_tab == AppTab::Totp && !self.input_text.is_empty() {
            let secret_key = self.input_text.trim();

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                totp_tool::generate_qr_code_data(secret_key, "QiUser", "Qi")
            }));

            match result {
                Ok(Ok((rgba, width, height))) => {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        &rgba,
                    );
                    self.qr_code_image = Some(Arc::new(color_image));
                }
                _ => {
                    self.qr_code_image = None;
                }
            }
        } else {
            self.qr_code_image = None;
        }
    }

}

impl eframe::App for QiToolboxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        Panel::top("top_panel").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let prev_tab = self.selected_tab.clone();

                ui.selectable_value(&mut self.selected_tab, AppTab::Totp, "2FA验证码生成");
                ui.selectable_value(&mut self.selected_tab, AppTab::Bcrypt, "Bcrypt密码生成");
                ui.selectable_value(&mut self.selected_tab, AppTab::TursoData, "Turso数据库浏览");
                ui.selectable_value(&mut self.selected_tab, AppTab::About, "关于软件");

                if self.selected_tab != prev_tab {
                    self.input_text.clear();
                    self.output_text.clear();
                }
            });
        });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical(|ui| {
                match self.selected_tab {
                    AppTab::TursoData => {
                        self.render_turso_tab(ui);
                        return;
                    }
                    AppTab::Totp => {
                        ui.add_space(8.0);
                        self.render_totp_tab(ui);
                    }
                    AppTab::Bcrypt => {
                        ui.add_space(8.0);
                        self.render_bcrypt_tab(ui);
                    }
                    AppTab::About => {
                        ui.add_space(8.0);
                        self.render_about_tab(ui);
                    }
                }
            });
        });
    }
}

impl QiToolboxApp {
    fn render_bcrypt_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Bcrypt密码加密工具").size(16.0).strong());
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            if ui.button(RichText::new("\u{1F3B2} 8位")).clicked() {
                self.generate_random_password(8);
                self.output_text.clear();
            }
            if ui.button(RichText::new("\u{1F3B2} 12位")).clicked() {
                self.generate_random_password(12);
                self.output_text.clear();
            }
            if ui.button(RichText::new("\u{1F3B2} 16位")).clicked() {
                self.generate_random_password(16);
                self.output_text.clear();
            }
        });

        ui.add_space(15.0);
        ui.label(RichText::new("待加密密码：").size(14.0));
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.input_text)
                    .desired_width(ui.available_width() - 200.0)
                    .hint_text("在此输入或点击上方按钮生成密码...")
                    .font(FontId::monospace(14.0)),
            );

            if ui.button(RichText::new("\u{1F512} 加密")).clicked() && !self.input_text.is_empty() {
                let input = self.input_text.trim();
                let hash = bcrypt_tool::run(Some(input.to_string()));
                self.output_text = hash.clone();
                self.sql_output_text = format!(
                    "UPDATE admins SET password = '{}' WHERE username = 'your_username';",
                    hash
                );
            }
        });

        ui.add_space(15.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("加密的密码：").size(14.0));
                ui.add_space(5.0);
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(ui.available_width(), 400.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ScrollArea::vertical()
                            .id_salt("bcrypt_hash_output")
                            .show(ui, |ui| {
                                ui.add(
                                    TextEdit::multiline(&mut self.output_text)
                                        .desired_width(300.0)
                                        .desired_rows(10)
                                        .interactive(true)
                                        .font(FontId::monospace(13.0)),
                                );
                            });
                    },
                );
            });

            ui.vertical(|ui| {
                ui.label(RichText::new("SQL更新语句：").size(14.0));
                ui.add_space(5.0);
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(ui.available_width(), 400.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ScrollArea::vertical()
                            .id_salt("bcrypt_sql_output")
                            .show(ui, |ui| {
                                ui.add(
                                    TextEdit::multiline(&mut self.sql_output_text)
                                        .desired_width(300.0)
                                        .desired_rows(10)
                                        .interactive(true)
                                        .font(FontId::monospace(13.0)),
                                );
                            });
                    },
                );
            });
        });
    }

    fn render_totp_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("2FA TOTP验证码生成器").size(16.0).strong());
        ui.add_space(10.0);

        if self.input_text.is_empty() {
            self.load_saved_key();
        }

        ui.horizontal(|ui| {
            if ui.button(RichText::new("\u{1F511} 生成密钥")).clicked() {
                self.input_text = totp_tool::generate_secret_key();
                self.output_text.clear();
            }
            ui.label(
                RichText::new("(生成兼容主流平台的安全密钥)")
                    .size(12.0)
                    .weak(),
            );
        });

        ui.add_space(15.0);
        ui.label(RichText::new("Base32密钥：").size(14.0));
        ui.label(RichText::new("已实际应用的密钥请妥善保存，泄漏将严重影响安全。").size(12.0).color(egui::Color32::RED));
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.input_text)
                    .desired_width(ui.available_width() - 300.0)
                    .hint_text("在此输入或点击上方按钮生成Base32密钥...")
                    .font(FontId::monospace(14.0)),
            );

            if ui.button(RichText::new("\u{26A1} 生成")).clicked() && !self.input_text.is_empty() {
                self.output_text = totp_tool::run(self.input_text.trim());
            }
            if ui.button(RichText::new("\u{1F4BE} 保存")).clicked() && !self.input_text.is_empty() {
                let key = self.input_text.trim();
                match std::fs::write("qi_key.txt", key) {
                    Ok(_) => {
                        self.output_text = format!(
                            "密钥已保存\n当前密钥: {}",
                            key
                        );
                    }
                    Err(e) => {
                        self.output_text = format!("保存失败: {}", e);
                    }
                }
            }
            if ui.button(RichText::new("\u{1F4F1} 二维码")).clicked() && !self.input_text.is_empty() {
                self.update_qr_code();
            }
        });

        ui.add_space(15.0);
        ui.label(RichText::new("TOTP验证码：").size(14.0));
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(ui.available_width(), 120.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ScrollArea::vertical()
                            .id_salt("totp_output")
                            .show(ui, |ui| {
                                ui.add(
                                    TextEdit::multiline(&mut self.output_text)
                                        .desired_width(300.0)
                                        .desired_rows(3)
                                        .interactive(true)
                                        .font(FontId::monospace(26.0)),
                                );
                            });
                    },
                );
            });

            ui.vertical_centered(|ui| {
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(ui.available_width(), 400.0),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        if let Some(qr_image) = &self.qr_code_image {
                            ui.group(|ui| {
                                ui.vertical_centered(|ui| {
                                    let texture_handle = ui.ctx().load_texture(
                                        "qr_code",
                                        qr_image.as_ref().clone(),
                                        egui::TextureOptions::LINEAR,
                                    );
                                    ui.add(
                                        egui::Image::new(&texture_handle)
                                            .fit_to_original_size(1.0),
                                    );
                                    ui.label(
                                        RichText::new("扫描此二维码配置Google Authenticator或Authy")
                                            .size(11.0)
                                            .weak(),
                                    );
                                });
                            });
                        } else if !self.input_text.is_empty() {
                            ui.group(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("点击上方按钮生成二维码")
                                            .size(12.0)
                                            .weak(),
                                    );
                                });
                            });
                        }
                    },
                );
            });
        });
    }

    fn render_turso_tab(&mut self, ui: &mut egui::Ui) {
        // 顶部操作栏
        ui.horizontal(|ui| {
            if ui.button(RichText::new("\u{1F4C2} 打开")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("选择Turso数据库文件")
                    .add_filter("SQLite/Turso数据库", &["db", "sqlite", "libsql"])
                    .add_filter("所有文件", &["*"])
                    .pick_file()
                {
                    self.turso_viewer.db_path = path.to_string_lossy().to_string();
                    self.output_text = format!(
                        "\u{2705} {}",
                        std::path::Path::new(&self.turso_viewer.db_path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| self.turso_viewer.db_path.clone())
                    );
                }
            }

            if ui.button(RichText::new("\u{1F50C} 连接")).clicked() {
                ui.ctx().request_repaint();

                let db_path = self.turso_viewer.db_path.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut viewer = turso_viewer::TursoViewer::default();
                    viewer.db_path = db_path.clone();
                    viewer.connect()?;
                    Ok::<_, String>(viewer)
                }));
                match result {
                    Ok(Ok(viewer)) => {
                        self.turso_viewer = viewer;
                        self.output_text = format!(
                            "\u{2705} {} | {} 个表",
                            std::path::Path::new(&db_path)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or(db_path),
                            self.turso_viewer.tables.len()
                        );
                    }
                    Ok(Err(e)) => {
                        self.output_text = format!("\u{274C} {}", e);
                    }
                    Err(panic) => {
                        let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "未知panic".to_string()
                        };
                        let _ = std::fs::write("qi-toolbox_panic.txt", &msg);
                        self.output_text = format!("\u{1F4A5} {}", msg);
                    }
                }
            }

            if ui.button(RichText::new("\u{1F504} 刷新")).clicked() {
                if let Some(ref table_name) = self.turso_viewer.selected_table.clone() {
                    match self.turso_viewer.set_selected_table(table_name.clone()) {
                        Ok(()) => {
                            self.output_text = "\u{2705} 已刷新".to_string();
                        }
                        Err(e) => {
                            self.output_text = format!("\u{274C} {}", e);
                        }
                    }
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(ref err) = self.turso_viewer.error_message {
                    ui.label(
                        RichText::new(err.as_str())
                            .size(11.0)
                            .color(egui::Color32::RED),
                    );
                }
                if !self.output_text.is_empty() {
                    ui.label(RichText::new(&self.output_text).size(11.0).weak());
                }
            });
        });

        ui.add_space(2.0);
        ui.separator();
        ui.add_space(2.0);

        // 左侧表列表 + 右侧数据表格，自适应剩余高度
        let remaining = ui.available_height();
        ui.allocate_ui_with_layout(
            egui::Vec2::new(ui.available_width(), remaining),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                datatable::render_table_list(&mut self.turso_viewer, ui, remaining);
                ui.separator();
                datatable::render_data_table(&mut self.turso_viewer, ui);
            },
        );
    }

    fn render_about_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("Qi Toolbox").size(22.0).strong());
            ui.add_space(8.0);
            ui.label(RichText::new("v0.1.0").size(13.0).weak());
            ui.add_space(20.0);
            ui.label(
                RichText::new("开源桌面工具集，提供以下功能：")
                    .size(14.0),
            );
            ui.add_space(12.0);
            ui.label(RichText::new("2FA 验证码生成  -  生成 TOTP 密钥与二维码，兼容 Google Authenticator / Authy").size(13.0));
            ui.add_space(4.0);
            ui.label(RichText::new("Bcrypt 密码加密  -  生成随机密码并计算 Bcrypt 哈希，附带 SQL 更新语句").size(13.0));
            ui.add_space(4.0);
            ui.label(RichText::new("Turso 数据库浏览  -  连接本地 Turso/libSQL 数据库，查看表结构与数据").size(13.0));
            ui.add_space(30.0);
            ui.label(
                RichText::new("基于 Rust + egui 构建")
                    .size(12.0)
                    .weak(),
            );
        });
    }
}
