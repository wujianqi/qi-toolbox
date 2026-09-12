//! SFTP 页 UI：连接表单 + 工具栏（上级/刷新/新建文件夹/上传/下载/删除）、
//! 文件列表与新建文件夹/删除确认弹窗。SSH 命令工具已拆至独立子窗口模块
//! [`super::ssh_cmd`]，本文件仅保留其状态结构 [`SftpUi`]。
//!
//! 页面状态封装在 [`SftpUi`]（run() 中创建，主题重建不丢状态）：
//! 含全部信号 + 工作线程命令发送端，后台消息统一由 [`SftpUi::on_msg`] 消费。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use windui::prelude::*;

use windui::core::{EventCtx, Widget};
use windui::event::{Event, MouseButton, PointerKind};
use windui::render::{Canvas, Paint};
use windui::text::{TextEngine, TextStyle};

use super::{icons, input_dialog, ssh_cmd};
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
    /// 多选的文件名列表（仅文件；目录点击是导航不参与选择），按点击顺序
    pub selected: Signal<Vec<String>>,
    pub mkdir_show: Signal<bool>,
    pub mkdir_name: Signal<String>,
    pub delete_show: Signal<bool>,
    /// 删除确认弹窗的提示文案（打开时拼好，如「确认删除以下 3 项？…」）
    pub delete_msg: Signal<String>,
    /// 待删除项（确认时用）：（文件名, 完整远程路径, 是否目录）
    pub delete_items: Signal<Vec<(String, String, bool)>>,
    /// SSH 命令工具：命令输入 / 输出内容（供独立子窗口 [`cmd_window`] 使用）
    pub cmd_input: Signal<String>,
    pub cmd_output: Signal<String>,
    /// 命令模板区展开/收起（默认收起，不常驻占空间；子窗内点标题行切换）
    pub show_tpl: Signal<bool>,
    /// SSH 命令是否执行中（「执行」禁用、「停止」可用、输入区状态提示联动）
    pub cmd_running: Signal<bool>,
    /// 用户自定义命令模板（AppData 持久化，「我的命令」组渲染；顺序即显示顺序）
    pub custom_cmds: Signal<Vec<String>>,
    /// 添加自定义命令弹窗显隐
    pub cmd_add_show: Signal<bool>,
    /// 添加弹窗内的命令输入
    pub cmd_add_input: Signal<String>,
    /// 工作线程命令发送端（`spawn_worker` 后回填）
    cmd: Rc<RefCell<Option<mpsc::Sender<sftp::SftpCmd>>>>,
    /// Exec 中断标志句柄（`spawn_worker` 返回的共享 `Arc<AtomicBool>`，回填后由「停止」置位）
    cancel: Rc<RefCell<Option<Arc<AtomicBool>>>>,
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
            selected: signal(Vec::new()),
            mkdir_show: signal(false),
            mkdir_name: signal(String::new()),
            delete_show: signal(false),
            delete_msg: signal(String::new()),
            delete_items: signal(Vec::new()),
            cmd_input: signal(String::new()),
            cmd_output: signal(String::new()),
            show_tpl: signal(false),
            cmd_running: signal(false),
            custom_cmds: signal(load_custom_cmds()),
            cmd_add_show: signal(false),
            cmd_add_input: signal(String::new()),
            cmd: Rc::new(RefCell::new(None)),
            cancel: Rc::new(RefCell::new(None)),
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

    /// 回填 Exec 中断标志（`spawn_worker` 返回后调用一次）
    pub fn set_cancel(&self, cancel: Arc<AtomicBool>) {
        *self.cancel.borrow_mut() = Some(cancel);
    }

    /// 取 Exec 中断标志句柄（UI 构建前已回填，命令窗口「停止」时置位）
    pub fn cancel(&self) -> Arc<AtomicBool> {
        self.cancel
            .borrow()
            .as_ref()
            .expect("sftp cancel 已在 run() 中回填")
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
                self.selected.set(Vec::new());
                self.cwd.set(String::new());
                self.error.set(e);
            }
            sftp::SftpMsg::Listed(Ok((cwd, list))) => {
                self.cwd.set(cwd);
                // 选中项按新列表过滤：被删除/改名而消失的自动落选；切到别的目录
                // 后旧目录的选中名不匹配，自然整体清空。
                let names: Vec<String> = list.iter().map(|e| e.name.clone()).collect();
                self.entries.set(list);
                self.selected
                    .update(move |sel| sel.retain(|n| names.iter().any(|x| x == n)));
                self.error.set(String::new());
            }
            sftp::SftpMsg::Listed(Err(e)) => self.error.set(e),
            sftp::SftpMsg::Done(Ok(msg)) => {
                if msg.is_empty() {
                    // 断开连接：清空列表与状态
                    self.connected.set(false);
                    self.entries.set(Vec::new());
                    self.selected.set(Vec::new());
                    self.cwd.set(String::new());
                    self.status.set(String::new());
                } else {
                    self.status.set(msg);
                }
            }
            sftp::SftpMsg::Done(Err(e)) => self.error.set(e),
            sftp::SftpMsg::ExecDone(Ok(out)) => {
                // 执行结束（正常/中断都清运行态），输出整段覆盖
                self.cmd_running.set(false);
                self.cmd_output.set(out);
            }
            sftp::SftpMsg::ExecDone(Err(e)) => {
                self.cmd_running.set(false);
                self.cmd_output.set(e);
            }
        }
    }
}

impl Default for SftpUi {
    fn default() -> Self {
        Self::new()
    }
}

/// 读取持久化的自定义命令模板（尽力而为：AppData 文件缺失/损坏/被清空一律返回空表）
fn load_custom_cmds() -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = crate::core::settings::load().get("ssh.custom_cmds") {
        for line in v.split('\n') {
            let c = line.trim();
            if !c.is_empty() && !out.iter().any(|s| s == c) {
                out.push(c.to_string());
            }
        }
    }
    out
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
        delete_items,
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
                // 记忆本次连接输入（尽力而为：主机等明文、密码 DPAPI 加密后落盘）
                let host_s = host.get();
                let port_s = port.get();
                let user_s = user.get();
                let pass_s = pass.get();
                crate::core::settings::commit(&[
                    ("sftp.host", Some(host_s.trim())),
                    ("sftp.port", Some(port_s.trim())),
                    ("sftp.user", Some(user_s.trim())),
                    ("sftp.pass", Some(pass_s.as_str())),
                ]);
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
                // 密码输入：掩码圆点显示，禁止复制/剪切明文
                .child(Element::text_input(pass, "").password().width(90))
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
                    force: false, // 上级导航：15s 内命中缓存即可
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
                // 刷新：跳过目录缓存，强制向服务器重拉当前目录最新列表
                let _ = cmd.send(sftp::SftpCmd::List {
                    path: cwd.get(),
                    force: true,
                });
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
                let names = selected.get();
                if names.is_empty() {
                    error.set(lang::SFTP_NO_SELECT().to_string());
                    return;
                }
                // 只下载文件；目录（勾选用于删除）无法下载，全选目录时给出提示
                let all = entries.get();
                let cwd_s = cwd.get();
                let remotes: Vec<String> = names
                    .iter()
                    .filter_map(|n| {
                        all.iter()
                            .find(|e| &e.name == n)
                            .filter(|e| !e.is_dir)
                            .map(|e| sftp::join_path(&cwd_s, &e.name))
                    })
                    .collect();
                if remotes.is_empty() {
                    error.set(lang::SFTP_NO_FILE_SELECT().to_string());
                    return;
                }
                // worker 按命令队列顺序逐个下载
                let cmd = cmd.clone();
                ctx.request_pick_folder(
                    PickDialog::new().title(lang::SFTP_PICK_DOWNLOAD()),
                    move |dir: Option<std::path::PathBuf>| {
                        if let Some(d) = dir {
                            let local_dir = d.to_string_lossy().to_string();
                            for remote in remotes {
                                let _ = cmd.send(sftp::SftpCmd::Download {
                                    remote: remote.clone(),
                                    local_dir: local_dir.clone(),
                                });
                            }
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
            let names = selected.get();
            if names.is_empty() {
                error.set(lang::SFTP_NO_SELECT().to_string());
                return;
            }
            // 组装待删项（文件名 + 完整远程路径 + 是否目录），按列表顺序展示
            let all = entries.get();
            let cwd_s = cwd.get();
            let items: Vec<(String, String, bool)> = names
                .iter()
                .filter_map(|n| {
                    all.iter()
                        .find(|e| &e.name == n)
                        .map(|e| (n.clone(), sftp::join_path(&cwd_s, n), e.is_dir))
                })
                .collect();
            delete_items.set(items.clone());
            delete_msg.set(lang::SFTP_DELETE_CONFIRM_MULTI().replace("{}", &items.len().to_string()));
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

    // ── 文件列表：自绘行控件 [`FileRow`]——平时无勾选框（列表干净），悬停该行
    // 浮现细线圆角方框，已选行常驻主题色对勾 + 浅蓝底；文件整行点击切换选中，
    // 目录点行首方框选中、点其余区域进入（目录勾选是唯一选中途径）。
    // 行图标 SVG 只解析一次（Image 内部 Rc 共享，克隆廉价），整列表共享一份素材。
    let folder_icon = icons::StatefulIcon::from_svg(icons::FOLDER, Some(14));
    // 删除确认弹窗清单用的目录图标克隆
    let dialog_folder_icon = folder_icon.clone();
    let cmd_rows = cmd.clone();
    let art = RowArt::new();
    let row_builder = move |e: sftp::SftpEntry| {
        Element::leaf()
            .widget(FileRow::new(e, selected, cwd, cmd_rows.clone(), art.clone()))
            .width_match()
            .height(30)
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

    // ── 删除确认弹窗：提示条数（含不可恢复警示）+ 待删项清单，确认后逐个删除 ──
    let item_icon = icons::StatefulIcon::from_svg(icons::FILE, Some(14));
    let items_list = Element::host_signal(delete_items, move |(name, path, is_dir)| {
        let icon = if is_dir {
            dialog_folder_icon.as_ref().map(|s| s.content())
        } else {
            item_icon.as_ref().map(|s| s.content())
        }
        .unwrap_or_else(|| ImageContent::new(None));
        Element::row()
            .width_match()
            .height(26)
            .cross(Align::Center)
            .spacing(8)
            .padding_xy(4, 0)
            .child(Element::image_content(icon))
            .child(
                Element::label(name)
                    .font_size(13.0)
                    .fg_role(Role::Text)
                    .weight(1.0)
                    .max_lines(1),
            )
            .child(
                // 目录标注（多选列表里目录也会被删除，需要一眼看出来）
                Element::label(if is_dir {
                    lang::SFTP_DELETE_DIR().to_string()
                } else {
                    path
                })
                .font_size(11.0)
                .fg_role(Role::TextMuted)
                .max_lines(1),
            )
    });
    let delete_dialog = Element::dialog_panel(
        delete_show,
        lang::SFTP_DELETE(),
        420,
        move |_| delete_show.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(
                Element::label_signal(delete_msg)
                    .font_size(14.0)
                    .fg_role(Role::Danger),
            )
            .child(
                Element::scroll()
                    .width_match()
                    .height(200)
                    .bg_role(Role::SurfaceAlt)
                    .corner(6.0)
                    .child(
                        Element::col()
                            .width_match()
                            .padding_xy(6, 6)
                            .child(items_list),
                    ),
            ),
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
                            let items = delete_items.get();
                            delete_show.set(false);
                            // worker 按命令队列顺序逐个删除；每删完一个自动重列当前目录
                            for (_, path, is_dir) in items {
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
                .child(conn_form)
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

// ───────────────────────────────────────────────────────────────────────
// 文件行自绘控件：悬停浮现勾选标记
// ───────────────────────────────────────────────────────────────────────

/// 行图标素材：SVG 一次解析、两张染色图（目录/文件）。`Image` 内部 Rc 共享，
/// 克隆廉价——整列表共享一份。（选中态不再给图标换色：槽位由勾选标记顶替。）
#[derive(Clone)]
struct RowArt {
    folder: Image,
    file: Image,
}

impl RowArt {
    fn new() -> Self {
        let muted = windui::theme::current().palette.text_muted;
        let mk = |bytes: &[u8]| {
            Image::from_svg_bytes(bytes, Some(15))
                .map(|img| img.tinted(muted))
                .expect("内置 SVG 必然可解析")
        };
        Self {
            folder: mk(icons::FOLDER),
            file: mk(icons::FILE),
        }
    }
}

/// 文件列表行：背景/悬停/勾选标记/图标/文件名/大小全部自绘（行高 30）。
///
/// 为什么不用 `label + clickable + 勾选框覆盖层`：常驻勾选框让列表显得表单化，
/// 且勾选后 `entries.set` 整列表重建会丢掉所有行的悬停态（光标下的勾选框闪没）。
/// 本控件把悬停做进实例状态、选中态每帧读信号绘制，切换选中只整窗标脏——
/// 勾选标记仅在悬停/选中时浮现，列表平时完全干净，且无重建闪烁。
struct FileRow {
    entry: sftp::SftpEntry,
    selected: Signal<Vec<String>>,
    cwd: Signal<String>,
    cmd: mpsc::Sender<sftp::SftpCmd>,
    art: RowArt,
    hover: Cell<bool>,
}

/// 勾选标记边长（逻辑 px）。悬停未选中时只画框，选中后框内补对勾。
const CHECK_SIZE: f32 = 16.0;
/// 行首槽位（图标/勾选标记共用）左缘（相对行左）：不占独立空间，悬停/选中时
/// 勾选标记**替换**图标，行内容与无多选时的紧凑布局完全一致。
const SLOT_X: f32 = 10.0;
/// 勾选命中区右缘（相对行左）：目录"点方框选中、点其余进入"的分界。
const CHECK_HIT: f32 = 30.0;
/// 大小列宽（行右缘再内收 [`SIZE_RIGHT_PAD`]）。
const SIZE_W: f32 = 90.0;
/// 大小列右缘内收量：避开水印在内容之上的滚动条。
const SIZE_RIGHT_PAD: f32 = 12.0;

impl FileRow {
    fn new(
        entry: sftp::SftpEntry,
        selected: Signal<Vec<String>>,
        cwd: Signal<String>,
        cmd: mpsc::Sender<sftp::SftpCmd>,
        art: RowArt,
    ) -> Self {
        Self {
            entry,
            selected,
            cwd,
            cmd,
            art,
            hover: Cell::new(false),
        }
    }

    fn is_sel(&self) -> bool {
        self.selected.get().contains(&self.entry.name)
    }

    /// 在多选集合中切换本行（文件/目录通用）。
    fn toggle_select(&self, ctx: &mut EventCtx) {
        let name = self.entry.name.clone();
        self.selected.update(move |v| {
            if let Some(i) = v.iter().position(|s| *s == name) {
                v.remove(i);
            } else {
                v.push(name);
            }
        });
        // 其它行的选中底色同帧随信号刷新
        ctx.mark_dirty_all();
    }
}

impl Widget for FileRow {
    fn measure(&self, avail: Size, _style: &Style, _text: &mut dyn TextEngine) -> Size {
        Size::new(avail.w.max(0), 30)
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        style: &Style,
    ) {
        let t = windui::theme::current();
        let p = &t.palette;
        let sel = self.is_sel();
        let (x, y, w, h) = (
            bounds.x as f32,
            bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
        );

        // 行底：选中 = 强调色浅底；悬停未选中 = 极淡中性层
        if sel {
            canvas.fill_round_rect(x, y, w, h, 6.0, &Paint::fill(p.accent.scale_alpha(0.12)));
        } else if self.hover.get() {
            canvas.fill_round_rect(x, y, w, h, 6.0, &Paint::fill(p.text.scale_alpha(0.05)));
        }

        // 行首槽位（16×16）：悬停/选中时画勾选标记**替换**文件图标（不占额外
        // 空间，列表平时与紧凑布局一致）；其余时刻画原图标。
        let slot_x = x + SLOT_X;
        let slot_y = y + (h - CHECK_SIZE) / 2.0;
        let show_check = sel || self.hover.get();
        if show_check {
            let color = if sel { p.accent } else { p.text_muted };
            canvas.stroke_round_rect(
                slot_x,
                slot_y,
                CHECK_SIZE,
                CHECK_SIZE,
                4.5,
                1.5,
                &Paint::fill(color),
            );
            if sel {
                let (px, py) = (slot_x + 3.5, slot_y + 8.0);
                canvas.draw_polyline(
                    &[(px, py), (px + 3.0, py + 3.0), (px + 8.5, py - 3.0)],
                    1.8,
                    &Paint::fill(p.accent),
                );
            }
        } else {
            let img = if self.entry.is_dir { &self.art.folder } else { &self.art.file };
            canvas.draw_image(
                img,
                Rect::new(slot_x as i32, slot_y as i32, CHECK_SIZE as i32, CHECK_SIZE as i32),
                Fit::Contain,
                0.0,
                1.0,
            );
        }

        // 文件名：超宽省略号截断（label 的 truncate 等效，逐字符回退 + "…"）
        let ts = TextStyle::of(style);
        let name_x = x + SLOT_X + CHECK_SIZE + 8.0;
        let name_w = (w - (name_x - x) - SIZE_W - 8.0).max(0.0);
        let mut disp: String = self.entry.name.clone();
        if (canvas.measure_text(&disp, &ts).w as f32) > name_w {
            while !disp.is_empty()
                && (canvas.measure_text(&format!("{}\u{2026}", disp), &ts).w as f32) > name_w
            {
                disp.pop();
            }
            disp.push('\u{2026}');
        }
        canvas.draw_text(
            &disp,
            Rect::new(name_x as i32, bounds.y, name_w as i32, bounds.h),
            style.resolved_fg(&t),
            Align::Start,
            &ts,
        );

        // 大小列（目录无大小）：右缘内收 12px，避开水印在内容之上的滚动条
        if !self.entry.is_dir {
            let ts_small = TextStyle { size: 12.0, ..ts };
            canvas.draw_text(
                &sftp::human_size(self.entry.size),
                Rect::new(
                    (x + w - SIZE_W - SIZE_RIGHT_PAD) as i32,
                    bounds.y,
                    SIZE_W as i32,
                    bounds.h,
                ),
                p.text_muted,
                Align::End,
                &ts_small,
            );
        }
    }

    fn on_event(&mut self, ctx: &mut EventCtx, ev: &Event) -> bool {
        match ev {
            Event::Pointer(p) => match p.kind {
                PointerKind::Enter => {
                    self.hover.set(true);
                    ctx.mark_dirty();
                    false
                }
                PointerKind::Leave => {
                    self.hover.set(false);
                    ctx.mark_dirty();
                    false
                }
                PointerKind::Down if p.button == MouseButton::Left => {
                    let b = ctx.bounds();
                    if !b.contains(p.pos) {
                        return false;
                    }
                    let rel = (p.pos.x - b.x) as f32;
                    if self.entry.is_dir {
                        // 点行首勾选区（勾选标记可见时）= 选中/取消目录；
                        // 不可见（未悬停）时点图标等同点行，进入目录。
                        if rel < CHECK_HIT && (self.hover.get() || self.is_sel()) {
                            self.toggle_select(ctx);
                        } else {
                            // 进入目录（15s 内命中缓存即秒开）
                            let path = sftp::join_path(&self.cwd.get(), &self.entry.name);
                            let _ = self.cmd.send(sftp::SftpCmd::List { path, force: false });
                        }
                    } else {
                        // 文件：整行点击切换选中（再点一次取消）
                        self.toggle_select(ctx);
                    }
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn cursor(&self) -> CursorShape {
        CursorShape::Hand
    }
}
