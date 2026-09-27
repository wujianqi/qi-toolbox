//! SFTP 页 UI：连接表单 + 工具栏（上级/刷新/新建文件夹/上传/下载/删除）、
//! 文件列表与新建文件夹/删除确认弹窗。SSH 命令工具已拆至独立子窗口模块
//! [`super::ssh_cmd`]，本文件仅保留其状态结构 [`SftpUi`]。
//!
//! 页面状态封装在 [`SftpUi`]（run() 中创建，主题重建不丢状态）：
//! 含全部信号 + 工作线程命令发送端，后台消息统一由 [`SftpUi::on_msg`] 消费。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use windui::prelude::*;

use super::sftp_row::{FileRow, RowArt};
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
    /// SSH 命令是否执行中（「执行」禁用、「停止」可用、输入区状态提示联动）
    pub cmd_running: Signal<bool>,
    /// 用户自定义命令模板（AppData 持久化，「我的命令」组渲染；顺序即显示顺序）
    pub custom_cmds: Signal<Vec<String>>,
    /// 添加自定义命令弹窗显隐
    pub cmd_add_show: Signal<bool>,
    /// 添加弹窗内的命令输入
    pub cmd_add_input: Signal<String>,
    /// 常用命令选择器：当前分组索引（对应 cmd_window 的分组表，含「我的命令」末组）
    pub tpl_group: Signal<usize>,
    /// 常用命令选择器：搜索关键字（空串 = 不过滤）
    pub tpl_search: Signal<String>,
    /// 已保存的 SFTP 站点（store.db 多站点管理）
    pub sites: Signal<Vec<crate::core::store::SftpSite>>,
    /// 常用目录书签（store.db sftp_bmks 表，跨会话）
    pub bmks: Signal<Vec<String>>,
    /// 站点下拉当前选中索引
    pub site_sel: Signal<usize>,
    /// 书签下拉当前选中索引（启动期创建的永生信号：不能在 build_sftp_tab 构建
    /// 期现建——主题切换整树重建会回收构建期信号，下拉句柄即成死句柄再读即崩）
    pub bmk_sel: Signal<usize>,
    /// 站点编辑弹窗显隐与表单（site_edit_id=0 表示新建）
    pub site_edit_show: Signal<bool>,
    /// 站点管理弹窗（列表集中管理：编辑/删除）
    pub site_mgr_show: Signal<bool>,
    pub site_edit_id: Signal<i64>,
    pub site_name: Signal<String>,
    pub site_host: Signal<String>,
    pub site_port: Signal<String>,
    pub site_user: Signal<String>,
    pub site_pass: Signal<String>,
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
            cmd_running: signal(false),
            custom_cmds: signal(load_custom_cmds()),
            cmd_add_show: signal(false),
            cmd_add_input: signal(String::new()),
            tpl_group: signal(0usize),
            tpl_search: signal(String::new()),
            sites: signal(crate::core::store::sftp_list().unwrap_or_default()),
            bmks: signal(crate::core::store::sftp_bmk_list().unwrap_or_default()),
            site_sel: signal(0usize),
            bmk_sel: signal(0usize),
            site_edit_show: signal(false),
            site_mgr_show: signal(false),
            site_edit_id: signal(0i64),
            site_name: signal(String::new()),
            site_host: signal(String::new()),
            site_port: signal(String::from("22")),
            site_user: signal(String::new()),
            site_pass: signal(String::new()),
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
                // 记住上次目录：连接成功后自动跳到上次浏览的目录（尽力而为）
                let last = crate::core::settings::load()
                    .get("sftp.last_dir")
                    .cloned()
                    .unwrap_or_default();
                if !last.is_empty() {
                    let _ = self.cmd().send(sftp::SftpCmd::List {
                        path: last,
                        force: false,
                    });
                }
            }
            sftp::SftpMsg::Connected(Err(e)) => {
                self.connected.set(false);
                self.entries.set(Vec::new());
                self.selected.set(Vec::new());
                self.cwd.set(String::new());
                self.error.set(e);
            }
            sftp::SftpMsg::Listed(Ok((cwd, list))) => {
                self.cwd.set(cwd.clone());
                // 记住上次浏览目录（非敏感项明文，尽力而为）
                let remember = cwd.clone();
                std::thread::spawn(move || {
                    crate::core::settings::commit(&[("sftp.last_dir", Some(remember.as_str()))]);
                });
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

/// 读取持久化的自定义命令模板（尽力而为：库读取失败一律返回空表）
fn load_custom_cmds() -> Vec<String> {
    crate::core::store::ssh_list()
        .unwrap_or_default()
        .iter()
        .filter(|c| c.category.is_empty())
        .map(|c| c.command.clone())
        .collect()
}

pub fn build_sftp_tab(ui: &SftpUi) -> (Element, Element) {
    let SftpUi {
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

    // ── 站点信号（连接行与站点管理弹窗共用；store.db 持久化）──
    let sites = ui.sites;
    let site_sel = ui.site_sel;
    let site_opts = site_sel.map(move |idx| {
        let list = sites.get();
        if list.is_empty() {
            vec![lang::SFTP_SITE_NONE()]
        } else {
            vec![list.get(*idx).map(|s| s.name.clone()).unwrap_or_default()]
        }
    });

    // ── 连接 / 断开（按连接状态互斥显示）；凭据取自选中站点（账号密码等
    // 全部收进「管理站点」弹窗，顶部与 S3 页一致保持简洁）──
    let connect = Element::button(lang::SFTP_CONNECT())
        .small()
        .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
        .visible_when(move || !connected.get())
        .on_click({
            let cmd = cmd.clone();
            move |_| {
                status.set(String::new());
                error.set(String::new());
                if let Some(s) = sites.get().get(site_sel.get()) {
                    let _ = cmd.send(sftp::SftpCmd::Connect {
                        host: s.host.clone(),
                        port: s.port,
                        user: s.user.clone(),
                        pass: s.pass.clone(),
                    });
                }
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

    // ── 连接行控件 ──
    fn strip(s: &str) -> &str {
        s.trim_end_matches([':', '：'])
    }

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

    // ── 目录书签：收藏当前目录 + 书签下拉跳转（store.db sftp_bmks 表）──
    let bmk_add = Element::button(lang::SFTP_BMK_ADD())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::SAVE, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let ui = ui.clone();
            move |_| {
                let path = ui.cwd.get();
                if path.is_empty() {
                    return;
                }
                if crate::core::store::sftp_bmk_add(&path).is_ok() {
                    ui.bmks
                        .set(crate::core::store::sftp_bmk_list().unwrap_or_default());
                    ui.status.set(lang::SFTP_BMK_ADDED().to_string());
                }
            }
        });
    let bmk_sel = ui.bmk_sel;
    let bmk_opts = bmk_sel.map({
        let bmks = ui.bmks;
        move |idx: &usize| {
            let list = bmks.get();
            if list.is_empty() {
                vec![lang::SFTP_BMK_NONE()]
            } else {
                vec![list
                    .get((*idx).min(list.len().saturating_sub(1)))
                    .cloned()
                    .unwrap_or_default()]
            }
        }
    });
    let bmk_jump = {
        let bmks = ui.bmks;
        let bmk_sel = bmk_sel;
        let cmd = cmd.clone();
        move |_: &mut windui::core::EventCtx| {
            if let Some(path) = bmks.get().get(bmk_sel.get()) {
                let _ = cmd.send(sftp::SftpCmd::List {
                    path: path.clone(),
                    force: false,
                });
            }
        }
    };
    let bmk_jump = Element::dropdown_signal(bmk_opts, bmk_sel)
        .width(160)
        .enabled_when({
            let bmks = ui.bmks;
            move || !bmks.get().is_empty() && connected.get()
        })
        .on_click(bmk_jump);
    // 删除当前选中的书签（列表为空时隐藏）
    let bmk_del = Element::button(lang::SFTP_BMK_DEL())
        .small()
        .neutral()
        .danger()
        .visible_when({
            let bmks = ui.bmks;
            move || !bmks.get().is_empty()
        })
        .on_click({
            let ui = ui.clone();
            let bmk_sel = bmk_sel;
            move |_| {
                let list = ui.bmks.get();
                if let Some(path) = list.get(bmk_sel.get().min(list.len().saturating_sub(1))) {
                    let path = path.clone();
                    if crate::core::store::sftp_bmk_del(&path).is_ok() {
                        ui.bmks
                            .set(crate::core::store::sftp_bmk_list().unwrap_or_default());
                        bmk_sel.set(0);
                    }
                }
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
            delete_msg.set(lang::SFTP_DELETE_CONFIRM_MULTI(items.len()));
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

    // ── 连接行：站点下拉 + 连接/断开 + 管理站点 + 标签下拉 + SSH 命令 ──
    // （后两项放到连接行，避免顶部文件工具栏过挤）
    let conn_form = Element::row()
        .spacing(6)
        .cross(Align::Center)
        .child(
            Element::dropdown_signal(site_opts, site_sel)
                .width(180)
                .on_click({
                    // 切换站点即自动断开旧连接，避免凭据错配
                    let cmd = cmd.clone();
                    move |_| {
                        let _ = cmd.send(sftp::SftpCmd::Disconnect);
                    }
                }),
        )
        .child(connect)
        .child(disconnect)
        .child(
            // 新建/编辑/删除集中到「管理站点」弹窗
            Element::button(lang::SFTP_SITE_MGR())
                .small()
                .neutral()
                .on_click({
                    let show = ui.site_mgr_show;
                    move |_| show.set(true)
                }),
        )
        .child(bmk_jump)
        .child(cmd_btn);

    let toolbar = Element::row()
        .spacing(6)
        .cross(Align::Center)
        .child(up)
        .child(refresh)
        .child(bmk_add)
        .child(bmk_del)
        .child(mkdir_btn)
        .child(upload_btn)
        .child(download_btn)
        .child(delete_btn)
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
            .widget(FileRow::new(
                e,
                selected,
                cwd,
                cmd_rows.clone(),
                art.clone(),
            ))
            .width_match()
            .height(26)
    };

    let list_area = Element::stack().weight(1.0).child(
        Element::scroll()
            .fill()
            .visible_when(move || connected.get())
            .child(Element::host_signal(entries, row_builder)),
    );

    // ── 新建文件夹弹窗 ──
    let mkdir_dialog = input_dialog(
        mkdir_show,
        lang::SFTP_MKDIR_TITLE(),
        620,
        {
            move |_| {
                mkdir_show.set(false);
                mkdir_name.set(String::new());
            }
        },
        Element::text_input(mkdir_name, lang::SFTP_MKDIR_HINT())
            .autofocus()
            .width_match(),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(Element::button(lang::SFTP_OK()).small().on_click({
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
            })),
    );

    // ── 删除确认弹窗：提示条数（含不可恢复警示）+ 待删项清单，确认后逐个删除 ──
    let item_icon = icons::StatefulIcon::from_svg(icons::GENERIC_FILE, Some(14));
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
        620,
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

    let sites = ui.sites;
    let site_sel = ui.site_sel;
    let _site_opts = site_sel.map(move |idx| {
        let list = sites.get();
        if list.is_empty() {
            vec![lang::SFTP_SITE_NONE()]
        } else {
            vec![list.get(*idx).map(|s| s.name.clone()).unwrap_or_default()]
        }
    });
    let _btn_site_new = Element::button(lang::SFTP_SITE_NEW())
        .small()
        .outline_soft()
        .neutral()
        .on_click({
            let (show, id, name, site_host, site_port, site_user, site_pass) = (
                ui.site_edit_show,
                ui.site_edit_id,
                ui.site_name,
                ui.site_host,
                ui.site_port,
                ui.site_user,
                ui.site_pass,
            );
            move |_| {
                id.set(0);
                // 新建态默认填示例数据，可直接改后保存
                name.set("示例站点".to_string());
                site_host.set("127.0.0.1".to_string());
                site_port.set("22".to_string());
                site_user.set("root".to_string());
                site_pass.set(String::new());
                show.set(true);
            }
        });
    let btn_site_save = Element::button(lang::SFTP_SITE_SAVE()).small().on_click({
        let (_show, id, name, site_host, site_port, site_user, site_pass) = (
            ui.site_edit_show,
            ui.site_edit_id,
            ui.site_name,
            ui.site_host,
            ui.site_port,
            ui.site_user,
            ui.site_pass,
        );
        move |_| {
            let n = name.get().trim().to_string();
            if n.is_empty() {
                return;
            }
            let site = crate::core::store::SftpSite {
                id: id.get(),
                name: n,
                host: site_host.get().trim().to_string(),
                port: site_port.get().trim().parse::<u16>().unwrap_or(22),
                user: site_user.get().trim().to_string(),
                pass: site_pass.get(),
            };
            if crate::core::store::sftp_upsert(&site).is_ok() {
                sites.set(crate::core::store::sftp_list().unwrap_or_default());
            }
            // 保存后不关弹窗：继续编辑/新建（关闭仅靠 X / Esc / 点遮罩）
        }
    });
    let site_close = ui.site_edit_show;
    let site_cancel_close = site_close;
    let site_dialog = input_dialog(
        ui.site_edit_show,
        lang::SFTP_SITE_TITLE(),
        620,
        move |_| site_close.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(
                Element::text_input(ui.site_name, strip(&lang::SFTP_SITE_NAME()))
                    .autofocus()
                    .width_match(),
            )
            .child(Element::text_input(ui.site_host, strip(&lang::SFTP_HOST())).width_match())
            .child(Element::text_input(ui.site_port, strip(&lang::SFTP_PORT())).width_match())
            .child(Element::text_input(ui.site_user, strip(&lang::SFTP_USER())).width_match())
            .child(
                Element::text_input(ui.site_pass, strip(&lang::SFTP_PASS()))
                    .password()
                    .width_match(),
            ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click(move |_| site_cancel_close.set(false)),
            )
            .child(btn_site_save),
    );

    // ── 站点管理弹窗：单层双栏（左列表 + 右表单），点行即填右侧表单，
    // 保存/删除都在同一层完成，不再弹二级编辑窗 ──
    let mgr_close = ui.site_mgr_show;
    let _ui_mgr_close = ui.site_mgr_show;
    let mgr_list_ui = ui.clone();
    let _mgr_del_ui = ui.clone();
    let mgr_list = Element::list_signal(
        ui.sites,
        |s: &crate::core::store::SftpSite| s.id,
        move |s: crate::core::store::SftpSite| {
            let (sel, sites) = (mgr_list_ui.site_sel, mgr_list_ui.sites);
            let form_ui = mgr_list_ui.clone();
            let row_id = s.id;
            Element::row()
                .width_match()
                .height(30)
                .cross(Align::Center)
                .spacing(8)
                .padding_xy(8, 0)
                .corner(4.0)
                .clickable()
                // 点行：同步下拉索引 + 整行字段回填右侧表单（单层内编辑）
                .on_click(move |_| {
                    let list = sites.get();
                    if let Some(idx) = list.iter().position(|x| x.id == row_id) {
                        sel.set(idx);
                    }
                    if let Some(x) = list.iter().find(|x| x.id == row_id) {
                        form_ui.site_edit_id.set(x.id);
                        form_ui.site_name.set(x.name.clone());
                        form_ui.site_host.set(x.host.clone());
                        form_ui.site_port.set(x.port.to_string());
                        form_ui.site_user.set(x.user.clone());
                        form_ui.site_pass.set(x.pass.clone());
                    }
                })
                .child(
                    Element::label(s.name.clone())
                        .font_size(13.0)
                        .fg_role(Role::Text)
                        .weight(1.0)
                        .max_lines(1)
                        .truncate(Truncate::End),
                )
                .child(
                    Element::label(format!("{}@{}", s.user, s.host))
                        .font_size(11.0)
                        .fg_role(Role::TextMuted)
                        .max_lines(1)
                        .truncate(Truncate::End),
                )
        },
    );
    // 双栏容器：左侧列表（可滚动），右侧表单（编辑/新建）+ 底部操作
    let form_save = Element::button(lang::SFTP_SITE_SAVE()).small().on_click({
        let (show, id, name, site_host, site_port, site_user, site_pass) = (
            ui.site_edit_show,
            ui.site_edit_id,
            ui.site_name,
            ui.site_host,
            ui.site_port,
            ui.site_user,
            ui.site_pass,
        );
        move |_| {
            let n = name.get().trim().to_string();
            if n.is_empty() {
                return;
            }
            let site = crate::core::store::SftpSite {
                id: id.get(),
                name: n,
                host: site_host.get().trim().to_string(),
                port: site_port.get().trim().parse::<u16>().unwrap_or(22),
                user: site_user.get().trim().to_string(),
                pass: site_pass.get(),
            };
            if crate::core::store::sftp_upsert(&site).is_ok() {
                sites.set(crate::core::store::sftp_list().unwrap_or_default());
            }
            show.set(false);
        }
    });
    let form_del = Element::button(lang::SFTP_SITE_DEL())
        .small()
        .neutral()
        .danger()
        .on_click({
            let (id, show) = (ui.site_edit_id, ui.site_edit_show);
            move |_| {
                let idv = id.get();
                if idv > 0 && crate::core::store::sftp_del(idv).is_ok() {
                    sites.set(crate::core::store::sftp_list().unwrap_or_default());
                    // 删除后也不关窗，表单重置为新建态
                    id.set(0);
                    let _ = show;
                }
            }
        });
    let mgr_body = Element::row()
        .width_match()
        .height(300)
        .spacing(12)
        // 左栏：站点列表（新建等操作统一放弹窗底部按钮排）
        .child(crate::widgets::mgr_list_col(
            mgr_list,
            sites.map(|s| s.is_empty()),
        ))
        // 右栏：表单卡片（点列表行回填；新建按钮清空）
        .child(crate::widgets::mgr_form_col(
            Element::col()
                .width_match()
                .height_match()
                .spacing(8)
                .child(
                    Element::text_input(ui.site_name, strip(&lang::SFTP_SITE_NAME()))
                        .autofocus()
                        .width_match(),
                )
                .child(Element::text_input(ui.site_host, strip(&lang::SFTP_HOST())).width_match())
                .child(
                    Element::row()
                        .spacing(8)
                        .child(
                            Element::text_input(ui.site_port, strip(&lang::SFTP_PORT())).width(80),
                        )
                        .child(
                            Element::text_input(ui.site_user, strip(&lang::SFTP_USER()))
                                .weight(1.0),
                        ),
                )
                .child(
                    Element::text_input(ui.site_pass, strip(&lang::SFTP_PASS()))
                        .password()
                        .width_match(),
                )
                .child(Element::flex_spacer()),
        ));
    // 底部按钮排：新建（清空表单）/ 删除（编辑态可用）/ 保存 / 确定
    let mgr_new = Element::button(lang::SFTP_SITE_NEW())
        .small()
        .neutral()
        .on_click({
            let (id, name, site_host, site_port, site_user, site_pass) = (
                ui.site_edit_id,
                ui.site_name,
                ui.site_host,
                ui.site_port,
                ui.site_user,
                ui.site_pass,
            );
            move |_| {
                id.set(0);
                // 新建态默认填示例数据，可直接改后保存
                name.set("示例站点".to_string());
                site_host.set("127.0.0.1".to_string());
                site_port.set("22".to_string());
                site_user.set("root".to_string());
                site_pass.set(String::new());
            }
        });
    let site_mgr_dialog = crate::widgets::mgr_dialog(
        ui.site_mgr_show,
        lang::SFTP_SITE_TITLE(),
        620,
        move |_| mgr_close.set(false),
        mgr_body,
        mgr_new,
        form_del,
        form_save,
    );

    // ── 页面：无标题，紧凑布局把空间让给文件区；弹窗由调用方挂根层级（遮罩铺满全窗）──
    let page = Element::stack().fill().child(
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
    );
    let dialogs = Element::stack()
        .fill()
        .child(mkdir_dialog)
        .child(delete_dialog)
        .child(site_dialog)
        .child(site_mgr_dialog);

    // 弹窗统一由根层级挂载：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
    (page, dialogs)
}
