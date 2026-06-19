use crate::{password, totp, turso_viewer, datatable, strings::lang, sql_editor};
use egui::{CentralPanel, FontId, Panel, RichText, ScrollArea, TextEdit};
use std::sync::Arc;

#[derive(Default, PartialEq, Clone)]
enum AppTab {
    #[default]
    Totp,
    Bcrypt,
    TursoData,
}

pub struct QiToolboxApp {
    selected_tab: AppTab,
    input_text: String,
    output_text: String,
    sql_output_text: String,
    qr_code_image: Option<Arc<egui::ColorImage>>,
    turso_viewer: turso_viewer::TursoViewer,
    account_name: String,
    issuer: String,
    hash_algorithm: password::HashAlgorithm,
    platform_preset: password::PlatformPreset,
    key_loaded: bool,
    show_about: bool,
    // SQL 查询
    show_sql_panel: bool,
    sql_query: String,
    sql_status: String,
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
            account_name: String::from("user"),
            issuer: String::from("Qi"),
            hash_algorithm: password::HashAlgorithm::Argon2id,
            platform_preset: password::PlatformPreset::None,
            key_loaded: false,
            show_about: false,
            show_sql_panel: false,
            sql_query: String::new(),
            sql_status: String::new(),
        }
    }

    fn load_saved_key(&mut self) {
        if let Ok(key) = std::fs::read_to_string("qi_key.txt") {
            self.input_text = key.trim().to_string();
        }
    }

    fn generate_random_password(&mut self, length: usize) {
        let all_chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz\
            ABCDEFGHIJKLMNOPQRSTUVWXYZ\
            0123456789\
            !@#$%^&*"
            .chars()
            .collect();
        let lower: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
        let upper: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().collect();
        let digits: Vec<char> = "0123456789".chars().collect();
        let special: Vec<char> = "!@#$%^&*".chars().collect();

        let len = length.max(8);
        let mut bytes = vec![0u8; len];
        getrandom::getrandom(&mut bytes).expect("rng failed");

        let mut chars: Vec<char> = Vec::with_capacity(len);
        // 保证至少包含四类字符各一个
        chars.push(lower[(bytes[0] as usize) % lower.len()]);
        chars.push(upper[(bytes[1] as usize) % upper.len()]);
        chars.push(digits[(bytes[2] as usize) % digits.len()]);
        chars.push(special[(bytes[3] as usize) % special.len()]);
        for &b in &bytes[4..] {
            chars.push(all_chars[(b as usize) % all_chars.len()]);
        }
        // Fisher-Yates 洗牌
        for i in (1..chars.len()).rev() {
            let j = (bytes[i % bytes.len()] as usize) % (i + 1);
            chars.swap(i, j);
        }

        self.input_text = chars.into_iter().collect();
    }

    fn update_qr_code(&mut self) {
        if self.selected_tab == AppTab::Totp && !self.input_text.is_empty() {
            let secret_key = self.input_text.trim();

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                totp::generate_qr_code_data(secret_key, &self.account_name, &self.issuer)
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

                ui.selectable_value(&mut self.selected_tab, AppTab::Totp, lang::TAB_2FA);
                ui.selectable_value(&mut self.selected_tab, AppTab::Bcrypt, lang::TAB_PASSWORD);
                ui.selectable_value(&mut self.selected_tab, AppTab::TursoData, lang::TAB_TURSO);

                if self.selected_tab != prev_tab {
                    self.input_text.clear();
                    self.output_text.clear();
                    self.show_sql_panel = false;
                    self.sql_status.clear();
                    self.show_about = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new(lang::ABOUT_BTN).size(13.0)).clicked() {
                        self.show_about = !self.show_about;
                    }
                });
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
                        self.render_password_tab(ui);
                    }
                }

                if self.show_about {
                    egui::Window::new(lang::ABOUT_TITLE)
                        .open(&mut self.show_about)
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ui.ctx(), |ui| {
                            ui.vertical(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new("Qi Toolbox").size(20.0).strong());
                                ui.add_space(4.0);
                                ui.label(RichText::new("v0.1.0").size(13.0).weak());
                                ui.add_space(16.0);
                                ui.label(RichText::new(lang::ABOUT_DESC).size(14.0));
                                ui.add_space(10.0);
                                ui.label(RichText::new(lang::ABOUT_2FA).size(13.0));
                                ui.add_space(4.0);
                                ui.label(RichText::new(lang::ABOUT_PWD).size(13.0));
                                ui.add_space(4.0);
                                ui.label(RichText::new(lang::ABOUT_TURSO).size(13.0));
                                ui.add_space(20.0);
                                ui.hyperlink_to(
                                    RichText::new("GitHub: https://github.com/wujianqi/qi-toolbox")
                                        .size(12.0)
                                        .color(egui::Color32::from_rgb(88, 166, 255)),
                                    "https://github.com/wujianqi/qi-toolbox",
                                );
                                ui.add_space(8.0);
                                ui.label(RichText::new(lang::ABOUT_BUILT).size(12.0).weak());
                            });
                        });
                }
            });
        });
    }
}

impl QiToolboxApp {
    fn render_password_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(lang::PWD_TITLE).size(16.0).strong());
        ui.add_space(10.0);

        // 平台预设
        ui.horizontal(|ui| {
            ui.label(RichText::new(lang::PWD_PLATFORM).size(13.0));
            let prev_preset = self.platform_preset;
            egui::ComboBox::from_id_salt("platform_preset")
                .selected_text(self.platform_preset.label())
                .show_ui(ui, |ui| {
                    for &preset in password::PlatformPreset::all() {
                        ui.selectable_value(&mut self.platform_preset, preset, preset.label());
                    }
                });
            // 平台切换时自动匹配算法
            if self.platform_preset != prev_preset {
                self.hash_algorithm = self.platform_preset.default_algorithm();
            }
            ui.label(RichText::new(self.platform_preset.note()).size(11.0).weak());
        });

        // 算法选择
        ui.horizontal(|ui| {
            ui.label(RichText::new(lang::PWD_ALGO).size(13.0));
            egui::ComboBox::from_id_salt("hash_algo")
                .selected_text(self.hash_algorithm.label())
                .show_ui(ui, |ui| {
                    for &algo in password::HashAlgorithm::all() {
                        ui.selectable_value(&mut self.hash_algorithm, algo, algo.label());
                    }
                });
            ui.label(self.hash_algorithm.description());
        });

        ui.add_space(10.0);

        // 密码生成按钮
        ui.horizontal(|ui| {
            if ui.button(RichText::new(lang::PWD_GEN_8)).clicked() {
                self.generate_random_password(8);
                self.output_text.clear();
            }
            if ui.button(RichText::new(lang::PWD_GEN_12)).clicked() {
                self.generate_random_password(12);
                self.output_text.clear();
            }
            if ui.button(RichText::new(lang::PWD_GEN_16)).clicked() {
                self.generate_random_password(16);
                self.output_text.clear();
            }
        });

        ui.add_space(15.0);
        ui.label(RichText::new(lang::PWD_INPUT_LABEL).size(14.0));
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.input_text)
                    .desired_width(ui.available_width() - 200.0)
                    .hint_text(lang::PWD_INPUT_HINT)
                    .font(FontId::monospace(14.0)),
            );

            if ui.button(RichText::new(lang::PWD_ENCRYPT)).clicked() && !self.input_text.is_empty() {
                let input = self.input_text.trim();
                match password::hash_password(input, self.hash_algorithm) {
                    Ok(hash) => {
                        self.output_text = hash.clone();
                        self.sql_output_text = format!(
                            "UPDATE admins SET password = '{}' WHERE username = 'your_username';",
                            hash
                        );
                    }
                    Err(e) => {
                        self.output_text = e;
                        self.sql_output_text.clear();
                    }
                }
            }
        });

        ui.add_space(15.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(lang::PWD_OUTPUT_LABEL).size(14.0));
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
                ui.label(RichText::new(lang::PWD_SQL_LABEL).size(14.0));
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
                                        .layouter(&mut sql_editor::sql_layouter)
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
        ui.label(RichText::new(lang::TOTP_TITLE).size(16.0).strong());
        ui.add_space(10.0);

        if self.input_text.is_empty() && !self.key_loaded {
            self.load_saved_key();
            self.key_loaded = true;
        }

        ui.horizontal(|ui| {
            if ui.button(RichText::new(lang::TOTP_GEN_KEY)).clicked() {
                self.input_text = totp::generate_secret_key();
                self.output_text.clear();
            }
            ui.label(
                RichText::new(lang::TOTP_GEN_HINT)
                    .size(12.0)
                    .weak(),
            );
        });

        ui.add_space(15.0);
        ui.label(RichText::new(lang::TOTP_KEY_LABEL).size(14.0));
        ui.label(RichText::new(lang::TOTP_KEY_WARN).size(12.0).color(egui::Color32::RED));
        ui.add_space(5.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new(lang::TOTP_ACCOUNT).size(13.0));
            ui.add(
                TextEdit::singleline(&mut self.account_name)
                    .desired_width(150.0)
                    .hint_text(lang::TOTP_ACCOUNT_HINT)
                    .font(FontId::monospace(13.0)),
            );
            ui.add_space(10.0);
            ui.label(RichText::new(lang::TOTP_ISSUER).size(13.0));
            ui.add(
                TextEdit::singleline(&mut self.issuer)
                    .desired_width(150.0)
                    .hint_text(lang::TOTP_ISSUER_HINT)
                    .font(FontId::monospace(13.0)),
            );
        });

        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.input_text)
                    .desired_width(ui.available_width() - 300.0)
                    .hint_text(lang::TOTP_KEY_INPUT_HINT)
                    .font(FontId::monospace(14.0)),
            );

            if ui.button(RichText::new(lang::TOTP_GENERATE)).clicked() && !self.input_text.is_empty() {
                self.output_text = totp::run(self.input_text.trim());
            }
            if ui.button(RichText::new(lang::TOTP_SAVE)).clicked() && !self.input_text.is_empty() {
                let key = self.input_text.trim();
                match std::fs::write("qi_key.txt", key) {
                    Ok(_) => {
                        self.output_text = format!(
                            "{}",
                            lang::TOTP_SAVED.replace("{}", key)
                        );
                    }
                    Err(e) => {
                        self.output_text = format!("{}: {}", lang::TOTP_SAVE_FAIL.trim_end_matches(": {}"), e);
                    }
                }
            }
            if ui.button(RichText::new(lang::TOTP_QR)).clicked() && !self.input_text.is_empty() {
                self.update_qr_code();
            }
        });

        ui.add_space(15.0);
        ui.label(RichText::new(lang::TOTP_CODE_LABEL).size(14.0));
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
                                        RichText::new(lang::TOTP_SCAN_HINT)
                                            .size(11.0)
                                            .weak(),
                                    );
                                });
                            });
                        } else if !self.input_text.is_empty() {
                            ui.group(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(lang::TOTP_CLICK_QR)
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
            if ui.button(RichText::new(lang::TURSO_OPEN)).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(lang::TURSO_FILE_TITLE)
                    .add_filter(lang::TURSO_FILE_FILTER1, &["db", "sqlite", "libsql"])
                    .add_filter(lang::TURSO_FILE_FILTER2, &["*"])
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

            if ui.button(RichText::new(lang::TURSO_CONNECT)).clicked() {
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
                        let fname = std::path::Path::new(&db_path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or(db_path);
                        let count = self.turso_viewer.tables.len();
                        self.output_text = lang::TURSO_CONNECTED
                            .replace("{}", &fname)
                            .replacen("{}", &count.to_string(), 1);
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
                            lang::TURSO_UNKNOWN_PANIC.to_string()
                        };
                        let _ = std::fs::write("qi-toolbox_panic.txt", &msg);
                        self.output_text = format!("\u{1F4A5} {}", msg);
                    }
                }
            }

            let connected = !self.turso_viewer.tables.is_empty();

            if ui.add_enabled(connected, egui::Button::new(RichText::new(lang::TURSO_REFRESH))).clicked() {
                self.show_sql_panel = false;
                self.sql_status.clear();
                if let Some(table_name) = self.turso_viewer.selected_table.clone() {
                    match self.turso_viewer.set_selected_table(table_name) {
                        Ok(()) => {
                            self.output_text = lang::TURSO_REFRESHED.to_string();
                        }
                        Err(e) => {
                            self.output_text = format!("\u{274C} {}", e);
                        }
                    }
                }
            }

            let sql_label = if self.show_sql_panel {
                lang::TURSO_CLOSE_SQL
            } else {
                lang::TURSO_OPEN_SQL
            };
            if ui.add_enabled(connected, egui::Button::new(RichText::new(sql_label))).clicked() {
                self.show_sql_panel = !self.show_sql_panel;
                self.sql_status.clear();
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

        // SQL 查询面板
        if self.show_sql_panel {
            sql_editor::render_sql_panel(&mut self.turso_viewer, ui, &mut self.sql_query, &mut self.sql_status);
        }

        // 左侧表列表 + 右侧数据表格，自适应剩余高度
        let remaining = ui.available_height();
        if self.show_sql_panel {
            // SQL 查询模式：隐藏左侧表列表，右侧直接显示查询结果
            datatable::render_data_table(&mut self.turso_viewer, ui);
        } else {
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
    }

}
