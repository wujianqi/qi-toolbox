//! SFTP 页 UI：连接表单 + 工具栏（上级/刷新/新建文件夹/上传/下载/删除）、
//! 文件列表与新建文件夹/删除确认弹窗。SSH 命令工具已拆至独立子窗口模块
//! [`super::ssh_cmd`]，本文件仅保留其状态结构 [`SftpUi`]。
//!
//! 页面状态封装在 [`SftpUi`]（run() 中创建，主题重建不丢状态）：
//! 含全部信号 + 工作线程命令发送端，后台消息统一由 [`SftpUi::on_msg`] 消费。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use windui::prelude::*;

use super::{card, icons, input_dialog, ssh_cmd};
use crate::core::sftp;
use crate::lang;

/// SFTP 页状态信号集合 + 工作线程命令发送端
#[derive(Clone)]
pub struct SftpUi {
    pub host: Signal<String>,
    pub port: Signal<String>,
    pub user: Signal<String>,
    pub pass: Signal<String>,
    pub connected: Signal<bool>,
    pub cwd: Signal<String>,
    pub entries: Signal<Vec<sftp::SftpEntry>>,
    pub status: Signal<String>,
    pub error: Signal<String>,
    pub selected: Signal<Option<String>>,
    pub mkdir_show: Signal<bool>,
    pub mkdir_name: Signal<String>,
    pub delete_show: Signal<bool>,
    /// 删除确认弹窗的提示文案（打开时拼好，如「确认删除 abc.txt？」）
    pub delete_msg: Signal<String>,
    /// 待删除文件的完整远程路径（确认时用）
    pub delete_path: Signal<String>,
    pub delete_is_dir: Signal<bool>,
    /// SSH 命令工具：命令输入 / 输出内容（供独立子窗口 [`cmd_window`] 使用）
    pub cmd_input: Signal<String>,
    pub cmd_output: Signal<String>,
    /// 命令模板区展开/收起（默认收起，不常驻占空间；子窗内点标题行切换）
    pub show_tpl: Signal<bool>,
    /// 工作线程命令发送端（`spawn_worker` 后回填）
    cmd: Rc<RefCell<Option<mpsc::Sender<sftp::SftpCmd>>>>,
}

impl SftpUi {
    pub fn new() -> Self {
        Self {
            host: signal(String::from("127.0.0.1")),
            port: signal(String::from("22")),
            user: signal(String::from("root")),
            pass: signal(String::new()),
            connected: signal(false),
            cwd: signal(String::new()),
            entries: signal(Vec::new()),
            status: signal(String::new()),
            error: signal(String::new()),
            selected: signal(None),
            mkdir_show: signal(false),
            mkdir_name: signal(String::new()),
            delete_show: signal(false),
            delete_msg: signal(String::new()),
            delete_path: signal(String::new()),
            delete_is_dir: signal(false),
            cmd_input: signal(String::new()),
            cmd_output: signal(String::new()),
            show_tpl: signal(false),
            cmd: Rc::new(RefCell::new(None)),
        }
    }

    /// 回填工作线程命令发送端（`spawn_worker` 返回后调用一次）
    pub fn set_cmd(&self, cmd: mpsc::Sender<sftp::SftpCmd>) {
        *self.cmd.borrow_mut() = Some(cmd);
    }

    /// 取工作线程命令发送端（UI 构建前已回填）
    pub fn cmd(&self) -> mpsc::Sender<sftp::SftpCmd> {
        self.cmd
            .borrow()
            .as_ref()
            .expect("sftp cmd 已在 run() 中回填")
            .clone()
    }

    /// 消费后台 SFTP 消息（`App::channel` 回 UI 线程时调用）
    pub fn on_msg(&self, msg: sftp::SftpMsg) {
        match msg {
            sftp::SftpMsg::Connected(Ok(cwd)) => {
                self.connected.set(true);
                self.status.set(lang::SFTP_CONNECTED().to_string());
                self.error.set(String::new());
                self.cwd.set(cwd);
            }
            sftp::SftpMsg::Connected(Err(e)) => {
                self.connected.set(false);
                self.entries.set(Vec::new());
                self.selected.set(None);
                self.cwd.set(String::new());
                self.error.set(e);
            }
            sftp::SftpMsg::Listed(Ok((cwd, list))) => {
                self.cwd.set(cwd);
                self.entries.set(list);
                self.error.set(String::new());
            }
            sftp::SftpMsg::Listed(Err(e)) => self.error.set(e),
            sftp::SftpMsg::Done(Ok(msg)) => {
                if msg.is_empty() {
                    // 断开连接：清空列表与状态
                    self.connected.set(false);
                    self.entries.set(Vec::new());
                    self.selected.set(None);
                    self.cwd.set(String::new());
                    self.status.set(String::new());
                } else {
                    self.status.set(msg);
                }
            }
            sftp::SftpMsg::Done(Err(e)) => self.error.set(e),
            sftp::SftpMsg::ExecDone(Ok(out)) => self.cmd_output.set(out),
            sftp::SftpMsg::ExecDone(Err(e)) => self.cmd_output.set(e),
        }
    }
}

impl Default for SftpUi {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_sftp_tab(ui: &SftpUi) -> Element {
    let SftpUi {
        host,
        port,
        user,
        pass,
        connected,
        cwd,
        entries,
        status,
        error,
        selected,
        mkdir_show,
        mkdir_name,
        delete_show,
        delete_msg,
        delete_path,
        delete_is_dir,
        ..
    } = ui.clone();
    let cmd = ui.cmd();

    // ── 连接 / 断开（按连接状态互斥显示）──
    let connect = Element::button(lang::SFTP_CONNECT())
        .small()
        .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
        .visible_when(move || !connected.get())
        .on_click({
            let cmd = cmd.clone();
            move |_| {
                status.set(String::new());
                error.set(String::new());
                let p = port.get().trim().parse::<u16>().unwrap_or(22);
                let _ = cmd.send(sftp::SftpCmd::Connect {
                    host: host.get().trim().to_string(),
                    port: p,
                    user: user.get().trim().to_string(),
                    pass: pass.get(),
                });
            }
        });

    let disconnect = Element::button(lang::SFTP_DISCONNECT())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
        .visible_when(move || connected.get())
        .on_click({
            let cmd = cmd.clone();
            move |_| {
                let _ = cmd.send(sftp::SftpCmd::Disconnect);
            }
        });

    // ── 连接表单 ──
    let conn_form = Element::col()
        .spacing(10)
        .child(
            Element::row()
                .spacing(8)
                .cross(Align::Center)
                .child(Element::label(lang::SFTP_HOST()).font_size(13.0))
                .child(Element::text_input(host, "").width(120))
                .child(Element::label(lang::SFTP_PORT()).font_size(13.0))
                .child(Element::text_input(port, "22").width(56))
                .child(Element::label(lang::SFTP_USER()).font_size(13.0))
                .child(Element::text_input(user, "").width(100))
                .child(Element::label(lang::SFTP_PASS()).font_size(13.0))
                .child(Element::text_input(pass, "").width(90))
                .child(connect)
                .child(disconnect),
        );

    // ── 工具栏：上级 / 刷新 / 新建文件夹 / 上传 / 下载 / 删除 ──
    let up = Element::button(lang::SFTP_UP())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::ARROW_LEFT, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let cmd = cmd.clone();
            move |_| {
                let _ = cmd.send(sftp::SftpCmd::List {
                    path: sftp::parent_path(&cwd.get()),
                });
            }
        });

    let refresh = Element::button(lang::SFTP_REFRESH())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::REFRESH, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let cmd = cmd.clone();
            move |_| {
                let _ = cmd.send(sftp::SftpCmd::List { path: cwd.get() });
            }
        });

    let mkdir_btn = Element::button(lang::SFTP_MKDIR())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::FOLDER, Some(16)))
        .enabled_signal(connected)
        .on_click(move |_| {
            mkdir_name.set(String::new());
            mkdir_show.set(true);
        });

    let upload_btn = Element::button(lang::SFTP_UPLOAD())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::UPLOAD, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let cmd = cmd.clone();
            move |ctx| {
                // 每次点击克隆一份，供文件对话框回调独占（否则外层闭包变 FnOnce）
                let cmd = cmd.clone();
                ctx.request_pick_file(
                    PickDialog::new().title(lang::SFTP_PICK_UPLOAD()),
                    move |path: Option<std::path::PathBuf>| {
                        if let Some(p) = path {
                            let local = p.to_string_lossy().to_string();
                            let fname = p
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let remote = sftp::join_path(&cwd.get(), &fname);
                            let _ = cmd.send(sftp::SftpCmd::Upload { local, remote });
                        }
                    },
                );
            }
        });

    let download_btn = Element::button(lang::SFTP_DOWNLOAD())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::DOWNLOAD, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let cmd = cmd.clone();
            move |ctx| {
                let Some(name) = selected.get() else {
                    error.set(lang::SFTP_NO_SELECT().to_string());
                    return;
                };
                let remote = sftp::join_path(&cwd.get(), &name);
                // 每次点击克隆一份，供文件夹对话框回调独占（否则外层闭包变 FnOnce）
                let cmd = cmd.clone();
                ctx.request_pick_folder(
                    PickDialog::new().title(lang::SFTP_PICK_DOWNLOAD()),
                    move |dir: Option<std::path::PathBuf>| {
                        if let Some(d) = dir {
                            let _ = cmd.send(sftp::SftpCmd::Download {
                                remote: remote.clone(),
                                local_dir: d.to_string_lossy().to_string(),
                            });
                        }
                    },
                );
            }
        });

    let delete_btn = Element::button(lang::SFTP_DELETE())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::TRASH, Some(16)))
        .enabled_signal(connected)
        .on_click(move |_| {
            let Some(name) = selected.get() else {
                error.set(lang::SFTP_NO_SELECT().to_string());
                return;
            };
            let is_dir = entries
                .get()
                .iter()
                .find(|e| e.name == name)
                .map(|e| e.is_dir)
                .unwrap_or(false);
            delete_path.set(sftp::join_path(&cwd.get(), &name));
            delete_msg.set(lang::SFTP_DELETE_CONFIRM().replace("{}", &name));
            delete_is_dir.set(is_dir);
            delete_show.set(true);
        });

    // SSH 命令工具：在独立子窗口打开（复用同一 SSH 会话执行远程命令）。
    // 子窗共享 SftpUi 的信号句柄（Signal 为 Copy），worker 通道注册在 App 级，
    // 命令输出会实时刷进子窗；`.single("sftp_cmd")` 防重复开窗（再点跳到前台）。
    let cmd_btn = Element::button(lang::SFTP_CMD())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::TERMINAL, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let ui = ui.clone();
            move |ctx| {
                // on_click 是 FnMut（可多次调用），body 内再 clone 供 content 闭包独占
                let ui = ui.clone();
                ctx.open_window(
                    Window::new(lang::SFTP_CMD_WIN_TITLE(), 680, 620)
                        .centered(true)
                        .min_size(520, 420)
                        .single("sftp_cmd")
                        .content(move || ssh_cmd::cmd_window(&ui)),
                );
            }
        });

    let toolbar = Element::row()
        .width_match()
        .spacing(8)
        .cross(Align::Center)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding_xy(12, 10)
        .child(up)
        .child(refresh)
        .child(mkdir_btn)
        .child(upload_btn)
        .child(download_btn)
        .child(delete_btn)
        .child(cmd_btn)
        .child(Element::flex_spacer())
        .child(
            Element::label_signal(status)
                .font_size(11.0)
                .fg_role(Role::TextMuted),
        )
        .child(
            Element::label_signal(error)
                .font_size(11.0)
                .fg_role(Role::Danger),
        );

    // ── 当前路径行 ──
    let path_row = Element::row()
        .width_match()
        .spacing(8)
        .cross(Align::Center)
        .child(
            Element::label(lang::SFTP_PATH())
                .font_size(12.0)
                .fg_role(Role::TextMuted),
        )
        .child(
            Element::label_signal(cwd)
                .font_size(12.0)
                .fg_role(Role::TextMuted)
                .weight(1.0)
                .max_lines(1),
        );

    // ── 文件列表：目录在前，点目录进入、点文件选中/取消选中 ──
    // SVG 图标只解析一次（StatefulIcon 内 Image 为 Rc 共享，克隆廉价），
    // 行内克隆复用，避免列表重建时每行反复解析 SVG 与染色
    let folder_icon = icons::StatefulIcon::from_svg(icons::FOLDER, Some(15));
    let file_icon = icons::StatefulIcon::from_svg(icons::FILE, Some(15));
    let cmd_rows = cmd.clone();
    let row_builder = move |e: sftp::SftpEntry| {
        // 每次调用克隆一份 sender 给行内按钮（行按钮是 move 闭包，会独占一份）
        let cc = cmd_rows.clone();
        let name = e.name.clone();
        let is_dir = e.is_dir;
        let sel = selected.get().as_deref() == Some(name.as_str());
        let icon = if is_dir {
            folder_icon.as_ref().map(|s| s.content())
        } else {
            file_icon.as_ref().map(|s| s.content())
        }
        .unwrap_or_else(|| ImageContent::new(None));
        let size_txt = if is_dir {
            String::new()
        } else {
            sftp::human_size(e.size)
        };
        let mut row = Element::row()
            .width_match()
            .height(30)
            .corner(6.0)
            .cross(Align::Center)
            .spacing(8)
            .padding_xy(10, 0)
            .child(Element::image_content(icon))
            .child(
                Element::label(e.name)
                    .font_size(13.0)
                    .fg_role(Role::Text)
                    .weight(1.0)
                    .max_lines(1),
            )
            .child(
                Element::label(size_txt)
                    .font_size(12.0)
                    .fg_role(Role::TextMuted)
                    .width(90),
            )
            .clickable();
        if sel {
            row = row.bg_role_alpha(Role::Accent, 0.12);
        }
        row.on_click(move |_| {
            if is_dir {
                selected.set(None);
                let p = sftp::join_path(&cwd.get(), &name);
                let _ = cc.send(sftp::SftpCmd::List { path: p });
            } else {
                let prev = selected.get();
                selected.set(if prev.as_deref() == Some(name.as_str()) {
                    None
                } else {
                    Some(name.clone())
                });
                // 重建行，刷新选中高亮
                entries.set(entries.get());
            }
        })
    };

    let list_area = Element::stack()
        .weight(1.0)
        .child(
            Element::label(lang::SFTP_NO_CONN())
                .font_size(13.0)
                .fg_role(Role::TextMuted)
                .align(Align::Center)
                .visible_when(move || !connected.get()),
        )
        .child(
            Element::scroll()
                .fill()
                .visible_when(move || connected.get())
                .child(Element::host_signal(entries, row_builder)),
        );

    // ── 新建文件夹弹窗 ──
    let mkdir_dialog = input_dialog(
        mkdir_show,
        lang::SFTP_MKDIR_TITLE(),
        360,
        {
            move |_| {
                mkdir_show.set(false);
                mkdir_name.set(String::new());
            }
        },
        Element::text_input(mkdir_name, lang::SFTP_MKDIR_HINT()).width_match(),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_OK())
                    .small()
                    .on_click({
                        let cmd = cmd.clone();
                        move |_| {
                            let name = mkdir_name.get().trim().to_string();
                            if !name.is_empty() {
                                let _ = cmd.send(sftp::SftpCmd::Mkdir {
                                    path: sftp::join_path(&cwd.get(), &name),
                                });
                            }
                            mkdir_show.set(false);
                            mkdir_name.set(String::new());
                        }
                    }),
            ),
    );

    // ── 删除确认弹窗 ──
    let delete_dialog = Element::dialog_panel(
        delete_show,
        lang::SFTP_DELETE(),
        360,
        move |_| delete_show.set(false),
        Element::label_signal(delete_msg).font_size(14.0),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click(move |_| delete_show.set(false)),
            )
            .child(
                Element::button(lang::SFTP_DELETE())
                    .small()
                    .danger()
                    .on_click({
                        let cmd = cmd.clone();
                        move |_| {
                            let path = delete_path.get();
                            let is_dir = delete_is_dir.get();
                            delete_show.set(false);
                            if !path.is_empty() {
                                let _ = cmd.send(sftp::SftpCmd::Delete { path, is_dir });
                            }
                        }
                    }),
            ),
    );

    // ── 页面：内容 + 两个弹窗（随页显隐）；无标题，紧凑布局把空间让给文件区 ──
    Element::stack()
        .fill()
        .child(
            Element::col()
                .padding(12)
                .spacing(8)
                .child(card(lang::SFTP_CARD_CONN(), conn_form))
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(8)
                        .child(toolbar)
                        .child(path_row)
                        .child(list_area),
                ),
        )
        .child(mkdir_dialog)
        .child(delete_dialog)
}
