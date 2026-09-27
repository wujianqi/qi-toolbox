//! S3 浏览页 UI：站点管理（多站点）+ 前缀目录浏览 + 上传/下载/删除/新建目录
//!
//! 页面状态封装在 [`S3Ui`]；阻塞 HTTP 在 core::s3 后台线程执行，
//! 结果经 S3Msg 回 UI 线程（ui::run 中注册 channel）。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use windui::prelude::*;

use windui::core::EventCtx;

use super::{icons, input_dialog};
use crate::core::fmt::format_size;
use crate::core::s3;
use crate::lang;
use crate::widgets;

/// S3 页状态信号集合
#[derive(Clone)]
pub struct S3Ui {
    /// 已保存站点（store.db）
    pub sites: Signal<Vec<crate::core::store::S3Site>>,
    /// 站点下拉选中索引
    pub site_sel: Signal<usize>,
    /// 站点管理弹窗（列表：选中即回填下拉，可编辑/删除）
    pub site_mgr_show: Signal<bool>,
    /// 站点编辑弹窗（site_edit_id=0 新建）
    pub site_edit_show: Signal<bool>,
    pub site_edit_id: Signal<i64>,
    pub site_name: Signal<String>,
    pub site_endpoint: Signal<String>,
    pub site_region: Signal<String>,
    pub site_bucket: Signal<String>,
    pub site_access: Signal<String>,
    pub site_secret: Signal<String>,
    pub site_path_style: Signal<usize>,
    /// 当前前缀（"" = 根）
    pub prefix: Signal<String>,
    /// 当前目录条目
    pub entries: Signal<Vec<s3::S3Entry>>,
    /// 多选的条目名
    pub selected: Signal<Vec<String>>,
    pub status: Signal<String>,
    pub error: Signal<String>,
    pub connected: Signal<bool>,
    /// 新建目录弹窗
    pub mkdir_show: Signal<bool>,
    pub mkdir_name: Signal<String>,
    /// 删除确认弹窗（待删项：名称/完整键/是否目录）
    pub delete_show: Signal<bool>,
    pub delete_msg: Signal<String>,
    pub delete_items: Signal<Vec<(String, String, bool)>>,
    /// 恢复确认弹窗（待恢复的备份对象名）
    pub restore_show: Signal<bool>,
    pub restore_msg: Signal<String>,
    /// 正在从 S3 下载备份（on_msg 收到 Done 时按恢复流程落库）
    pub restoring: Signal<bool>,
    /// 工作线程命令发送端（run() 中回填）
    cmd: Rc<RefCell<Option<mpsc::Sender<s3::S3Cmd>>>>,
}

impl S3Ui {
    pub fn new() -> Self {
        Self {
            sites: signal(crate::core::store::s3_list().unwrap_or_default()),
            site_sel: signal(0usize),
            site_mgr_show: signal(false),
            site_edit_show: signal(false),
            site_edit_id: signal(0i64),
            site_name: signal(String::new()),
            site_endpoint: signal(String::new()),
            site_region: signal(String::new()),
            site_bucket: signal(String::new()),
            site_access: signal(String::new()),
            site_secret: signal(String::new()),
            site_path_style: signal(0usize),
            prefix: signal(String::new()),
            entries: signal(Vec::new()),
            selected: signal(Vec::new()),
            status: signal(String::new()),
            error: signal(String::new()),
            connected: signal(false),
            mkdir_show: signal(false),
            mkdir_name: signal(String::new()),
            delete_show: signal(false),
            delete_msg: signal(String::new()),
            delete_items: signal(Vec::new()),
            restore_show: signal(false),
            restore_msg: signal(String::new()),
            restoring: signal(false),
            cmd: Rc::new(RefCell::new(None)),
        }
    }

    pub fn set_cmd(&self, cmd: mpsc::Sender<s3::S3Cmd>) {
        *self.cmd.borrow_mut() = Some(cmd);
    }

    pub fn cmd(&self) -> mpsc::Sender<s3::S3Cmd> {
        self.cmd
            .borrow()
            .as_ref()
            .expect("s3 cmd 已在 run() 中回填")
            .clone()
    }

    /// 当前选中站点的连接凭据（未选/表空返回 None）
    fn cred(&self) -> Option<s3::S3Cred> {
        let s = self.sites.get().get(self.site_sel.get())?.clone();
        Some(s3::S3Cred {
            endpoint: s.endpoint,
            region: s.region,
            bucket: s.bucket,
            access_key: s.access_key,
            secret: s.secret,
            path_style: s.path_style,
        })
    }

    /// 消费后台 S3 消息（App::channel 回 UI 线程时调用）
    pub fn on_msg(&self, msg: s3::S3Msg) {
        match msg {
            s3::S3Msg::Listed(Ok((prefix, list))) => {
                self.prefix.set(prefix);
                self.entries.set(list);
                self.selected.set(Vec::new());
                self.connected.set(true);
                self.error.set(String::new());
            }
            s3::S3Msg::Listed(Err(e)) => {
                self.connected.set(false);
                self.entries.set(Vec::new());
                self.error.set(e);
            }
            s3::S3Msg::Done(Ok(s)) => {
                // 恢复流程：下载完成的产物是 store.db 备份，覆盖本地库而非普通下载提示
                if self.restoring.get() {
                    self.restoring.set(false);
                    // Done 消息带本地化前缀（“已下载: <path>”），取最后一个 ": " 之后的真实路径
                    // （Windows 盘符 "C:" 后无空格，不会误截）
                    let path = match s.rsplit_once(": ") {
                        Some((_, p)) if std::path::Path::new(p).exists() => p.to_string(),
                        _ => s.clone(),
                    };
                    let db = crate::core::store::db_path();
                    match std::fs::copy(&path, &db) {
                        Ok(_) => self.status.set(lang::S3_RESTORE_OK(&path)),
                        Err(e) => self.error.set(format!("restore: {}", e)),
                    }
                    return;
                }
                self.status.set(s);
                self.error.set(String::new());
                // 变更类操作成功后刷新当前目录
                self.refresh();
            }
            s3::S3Msg::Done(Err(e)) => self.error.set(e),
        }
    }

    /// 请求列出当前前缀
    pub fn refresh(&self) {
        if let Some(cred) = self.cred() {
            let _ = self.cmd().send(s3::S3Cmd::List {
                cred,
                prefix: self.prefix.get(),
            });
        }
    }
}

impl Default for S3Ui {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_s3_tab(ui: &S3Ui) -> (Element, Element) {
    let cmd = ui.cmd();
    let sites = ui.sites;
    let site_sel = ui.site_sel;
    let prefix = ui.prefix;
    let entries = ui.entries;
    let selected = ui.selected;
    let connected = ui.connected;
    let status = ui.status;
    let error = ui.error;

    // ── 站点管理：下拉 + 新建/编辑/删除 ──
    let site_opts = site_sel.map(move |idx| {
        let list = sites.get();
        if list.is_empty() {
            vec![lang::S3_SITE_NONE()]
        } else {
            vec![list.get(*idx).map(|s| s.name.clone()).unwrap_or_default()]
        }
    });
    let open_edit = |ui: &S3Ui, s: Option<crate::core::store::S3Site>| {
        match s {
            Some(s) => {
                ui.site_edit_id.set(s.id);
                ui.site_name.set(s.name);
                ui.site_endpoint.set(s.endpoint);
                ui.site_region.set(s.region);
                ui.site_bucket.set(s.bucket);
                ui.site_access.set(s.access_key);
                ui.site_secret.set(s.secret);
                ui.site_path_style.set(if s.path_style { 1 } else { 0 });
            }
            None => {
                ui.site_edit_id.set(0);
                ui.site_name.set(String::new());
                ui.site_endpoint.set(String::new());
                ui.site_region.set(String::new());
                ui.site_bucket.set(String::new());
                ui.site_access.set(String::new());
                ui.site_secret.set(String::new());
                ui.site_path_style.set(0);
            }
        }
        ui.site_edit_show.set(true);
    };
    let _btn_new = Element::button(lang::S3_SITE_NEW())
        .small()
        .neutral()
        .on_click({
            let ui = ui.clone();
            move |_| open_edit(&ui, None)
        });
    // 「管理站点」：弹窗内集中管理（页面保持简洁，主界面只留下拉/打开/管理）
    let btn_manage = Element::button(lang::S3_SITE_MGR())
        .small()
        .neutral()
        .on_click({
            let ui = ui.clone();
            move |_| ui.site_mgr_show.set(true)
        });
    // 打开站点：先验证连通（列根目录），成功才视为已连接
    let btn_open = Element::button(lang::S3_OPEN())
        .small()
        .icon_content(icons::stateful_icon(icons::PLUG, Some(16)))
        .on_click({
            let ui = ui.clone();
            let cmd = cmd.clone();
            move |_| {
                status.set(String::new());
                error.set(String::new());
                if let Some(cred) = ui.cred() {
                    let _ = cmd.send(s3::S3Cmd::List {
                        cred,
                        prefix: String::new(),
                    });
                }
            }
        });
    // ── 备份/恢复：把本地 store.db 快照上传当前目录 / 选中备份覆盖本地库 ──
    //（挂在站点行「管理站点」之后：备份是站点级操作，与打开/管理同区更顺）
    let bak_btn = Element::button(lang::S3_BAK())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::SAVE, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let ui = ui.clone();
            move |_| match crate::core::store::snapshot_db_path() {
                Ok(snap) => {
                    if let Some(cred) = ui.cred() {
                        let name = format!("store-{}.db", chrono_now_stamp());
                        let key = s3::join_key(&ui.prefix.get(), &name, false);
                        let _ = ui.cmd().send(s3::S3Cmd::Upload {
                            cred,
                            key,
                            local: snap.to_string_lossy().into_owned(),
                        });
                        ui.status.set(lang::S3_UPLOADING().to_string());
                    }
                }
                Err(e) => ui.error.set(e),
            }
        });
    let restore_btn = Element::button(lang::S3_RESTORE())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::REFRESH, Some(16)))
        .enabled_when(move || connected.get() && !selected.get().is_empty())
        .on_click({
            let ui = ui.clone();
            move |_| {
                // 仅允许恢复单个 .db 文件（本系统备份）
                let sel = ui.selected.get();
                let entries = ui.entries.get();
                let Some(name) = sel.iter().find(|n| {
                    entries
                        .iter()
                        .find(|e| &e.name == *n)
                        .map(|e| !e.is_dir && e.name.ends_with(".db"))
                        .unwrap_or(false)
                }) else {
                    ui.error.set(lang::S3_DIR_TAG()); // 占位不可达：enabled_when 已保证选中
                    return;
                };
                ui.restore_msg.set(lang::S3_RESTORE_CONFIRM(name));
                ui.restore_show.set(true);
            }
        });

    let site_row = Element::row()
        .spacing(6)
        .cross(Align::Center)
        .child(Element::dropdown_signal(site_opts, site_sel).width(180))
        .child(btn_open)
        .child(btn_manage)
        .child(bak_btn)
        .child(restore_btn);

    // ── 工具栏：上级 / 刷新 / 上传 / 下载 / 新建目录 / 删除 ──
    let up = Element::button(lang::S3_UP())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::ARROW_LEFT, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let ui = ui.clone();
            let cmd = cmd.clone();
            move |_| {
                let p = s3::parent_prefix(&ui.prefix.get());
                if let Some(cred) = ui.cred() {
                    let _ = cmd.send(s3::S3Cmd::List { cred, prefix: p });
                }
            }
        });
    let refresh = Element::button(lang::S3_REFRESH())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::REFRESH, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let ui = ui.clone();
            move |_| ui.refresh()
        });
    let upload_btn = Element::button(lang::S3_UPLOAD())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::UPLOAD, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let ui = ui.clone();
            move |_| {
                if let Some(file) = widgets::pick_file() {
                    if let (Some(cred), Some(name)) = (
                        ui.cred(),
                        file.split(['/', '\\']).next_back().map(str::to_string),
                    ) {
                        let key = s3::join_key(&ui.prefix.get(), &name, false);
                        let _ = ui.cmd().send(s3::S3Cmd::Upload {
                            cred,
                            key,
                            local: file,
                        });
                        ui.status.set(lang::S3_UPLOADING().to_string());
                    }
                }
            }
        });
    let download_btn = Element::button(lang::S3_DOWNLOAD())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::DOWNLOAD, Some(16)))
        .enabled_when(move || connected.get() && !selected.get().is_empty())
        .on_click({
            let ui = ui.clone();
            move |_| {
                let Some(dir) = widgets::pick_dir() else {
                    return;
                };
                let sel = ui.selected.get();
                let entries = ui.entries.get();
                for name in sel {
                    if let Some(e) = entries.iter().find(|e| e.name == name) {
                        if e.is_dir {
                            continue; // 目录不支持整体下载（逐键下载属后续增强）
                        }
                        if let Some(cred) = ui.cred() {
                            let key = s3::join_key(&ui.prefix.get(), &e.name, false);
                            let _ = ui.cmd().send(s3::S3Cmd::Download {
                                cred,
                                key,
                                local_dir: dir.clone(),
                            });
                        }
                    }
                }
                ui.status.set(lang::S3_DOWNLOADING().to_string());
            }
        });
    let mkdir_btn = Element::button(lang::S3_MKDIR())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::FOLDER, Some(16)))
        .enabled_signal(connected)
        .on_click({
            let ui = ui.clone();
            move |_| {
                ui.mkdir_name.set(String::new());
                ui.mkdir_show.set(true);
            }
        });
    let del_btn = Element::button(lang::S3_DELETE())
        .small()
        .neutral()
        .icon_content(icons::stateful_icon(icons::TRASH, Some(16)))
        .enabled_when(move || connected.get() && !selected.get().is_empty())
        .on_click({
            let ui = ui.clone();
            move |_| {
                let sel = ui.selected.get();
                let entries = ui.entries.get();
                let mut items = Vec::new();
                for name in sel {
                    if let Some(e) = entries.iter().find(|e| e.name == name) {
                        let key = s3::join_key(&ui.prefix.get(), &e.name, e.is_dir);
                        items.push((e.name.clone(), key, e.is_dir));
                    }
                }
                if items.is_empty() {
                    return;
                }
                let n = items.len();
                ui.delete_items.set(items);
                ui.delete_msg.set(lang::S3_DELETE_CONFIRM(n));
                ui.delete_show.set(true);
            }
        });

    let toolbar = Element::row()
        .spacing(6)
        .cross(Align::Center)
        .child(up)
        .child(refresh)
        .child(upload_btn)
        .child(download_btn)
        .child(mkdir_btn)
        .child(del_btn);

    // ── 路径行：bucket + 当前前缀 ──
    let ui_label = ui.clone();
    let prefix_label = prefix.map(move |p| {
        let b = ui_label
            .sites
            .get()
            .get(ui_label.site_sel.get())
            .map(|s| s.bucket.clone())
            .unwrap_or_default();
        if p.is_empty() {
            format!("{}/", b)
        } else {
            format!("{}/{}", b, p)
        }
    });
    let path_row = Element::row()
        .width_match()
        .cross(Align::Center)
        .spacing(6)
        .child(
            Element::label_signal(prefix_label)
                .font_size(12.0)
                .fg_role(Role::TextMuted),
        )
        .child(Element::flex_spacer())
        .child(
            Element::label_signal(status)
                .font_size(11.0)
                .fg_role(Role::Accent),
        )
        .child(
            Element::label_signal(error)
                .font_size(11.0)
                .fg_role(Role::Danger),
        );

    // ── 文件列表：单行条目（目录点击进入；文件点击多选）──
    // ui/cmd 先克隆出列表专用副本，避免函数参数引用逃逸进闭包
    let ui_rows = ui.clone();
    let cmd_rows = cmd.clone();
    let list_area = Element::scroll()
        .width_match()
        .weight(1.0)
        .corner(8.0)
        .bg_role(Role::SurfaceAlt)
        .child(
            Element::list_signal(
                entries,
                |e: &s3::S3Entry| e.name.clone(),
                move |e: s3::S3Entry| {
                    let name = e.name.clone();
                    let is_dir = e.is_dir;
                    let size_txt = if is_dir {
                        String::new()
                    } else {
                        format_size(e.size)
                    };
                    let icon = if is_dir {
                        icons::FOLDER
                    } else {
                        icons::GENERIC_FILE
                    };
                    let (row_prefix, row_ui, row_cmd) = (prefix, ui_rows.clone(), cmd_rows.clone());
                    let (sel2, name2, label_name) = (selected, name.clone(), name.clone());
                    Element::row()
                        .width_match()
                        .height(28)
                        .corner(4.0)
                        .cross(Align::Center)
                        .padding_xy(8, 0)
                        .spacing(8)
                        .clickable()
                        .on_click(move |_| {
                            if is_dir {
                                // 目录导航：进入子前缀（S3 目录 = 前缀）
                                let p = s3::join_key(&row_prefix.get(), &name2, true);
                                if let Some(cred) = row_ui.cred() {
                                    let _ = row_cmd.send(s3::S3Cmd::List { cred, prefix: p });
                                }
                            } else {
                                // 文件多选（再点取消）
                                let name3 = name2.clone();
                                sel2.update(move |v| {
                                    if let Some(pos) = v.iter().position(|s| s == &name3) {
                                        v.remove(pos);
                                    } else {
                                        v.push(name3.clone());
                                    }
                                });
                            }
                        })
                        // 彩色素材自带配色，直接解析，不参与主题染色
                        .child(Element::image_content(ImageContent::from_svg_bytes(
                            icon,
                            Some(14),
                        )))
                        .child(
                            Element::label(label_name)
                                .font_size(13.0)
                                .fg_role(Role::Text)
                                .weight(1.0)
                                .max_lines(1)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(size_txt)
                                .font_size(11.0)
                                .fg_role(Role::TextMuted),
                        )
                },
            )
            .padding_xy(6, 6),
        );

    // ── 新建目录弹窗 ──
    let mkdir_ok = {
        let ui = ui.clone();
        move |_: &mut EventCtx| {
            let name = ui.mkdir_name.get().trim().to_string();
            ui.mkdir_show.set(false);
            ui.mkdir_name.set(String::new());
            if name.is_empty() {
                return;
            }
            if let Some(cred) = ui.cred() {
                let _ = ui.cmd().send(s3::S3Cmd::Mkdir {
                    cred,
                    prefix: ui.prefix.get(),
                    name,
                });
            }
        }
    };
    let mkdir_dialog = input_dialog(
        ui.mkdir_show,
        lang::S3_MKDIR(),
        620,
        {
            let ui = ui.clone();
            move |_| ui.mkdir_show.set(false)
        },
        Element::col().width_match().child(
            Element::text_input(ui.mkdir_name, lang::S3_MKDIR_HINT())
                .autofocus()
                .width_match(),
        ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click({
                        let ui = ui.clone();
                        move |_| ui.mkdir_show.set(false)
                    }),
            )
            .child(Element::button(lang::SFTP_OK()).small().on_click(mkdir_ok)),
    );

    // ── 删除确认弹窗：目录逐键删除警示 ──
    let del_close = ui.delete_show;
    let del_items_sig = ui.delete_items;
    let item_rows = Element::list_signal(
        del_items_sig,
        |(name, _, _): &(String, String, bool)| name.clone(),
        |(name, key, is_dir): (String, String, bool)| {
            Element::row()
                .width_match()
                .height(26)
                .cross(Align::Center)
                .spacing(8)
                .child(
                    Element::label(if is_dir {
                        format!("{}/（{}）", name, lang::S3_DIR_TAG())
                    } else {
                        name
                    })
                    .font_size(13.0)
                    .fg_role(Role::Text)
                    .weight(1.0)
                    .max_lines(1)
                    .truncate(Truncate::End),
                )
                .child(
                    Element::label(key)
                        .font_size(11.0)
                        .fg_role(Role::TextMuted)
                        .max_lines(1)
                        .truncate(Truncate::End),
                )
        },
    );
    let (del_ui, del_cmd) = (ui.clone(), cmd.clone());
    let delete_dialog = input_dialog(
        ui.delete_show,
        lang::S3_DELETE(),
        620,
        move |_| del_close.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(
                Element::label_signal(ui.delete_msg)
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
                            .child(item_rows),
                    ),
            ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click({
                        let ui = ui.clone();
                        move |_| ui.delete_show.set(false)
                    }),
            )
            .child(
                Element::button(lang::S3_DELETE())
                    .small()
                    .danger()
                    .on_click(move |_| {
                        let items = del_ui.delete_items.get();
                        del_ui.delete_show.set(false);
                        del_ui.selected.set(Vec::new());
                        if let Some(cred) = del_ui.cred() {
                            for (_, key, is_dir) in items {
                                if is_dir {
                                    let _ = del_cmd.send(s3::S3Cmd::DeleteDir {
                                        cred: cred.clone(),
                                        prefix: key,
                                    });
                                } else {
                                    let _ = del_cmd.send(s3::S3Cmd::Delete {
                                        cred: cred.clone(),
                                        key,
                                    });
                                }
                            }
                        }
                    }),
            ),
    );

    // ── 站点编辑弹窗 ──
    let edit_close = ui.site_edit_show;
    let save_btn = Element::button(lang::SFTP_SITE_SAVE()).small().on_click({
        let ui = ui.clone();
        move |_| {
            let name = ui.site_name.get().trim().to_string();
            if name.is_empty() {
                return;
            }
            let site = crate::core::store::S3Site {
                id: ui.site_edit_id.get(),
                name,
                endpoint: ui
                    .site_endpoint
                    .get()
                    .trim()
                    .trim_end_matches('/')
                    .to_string(),
                region: ui.site_region.get().trim().to_string(),
                bucket: ui.site_bucket.get().trim().to_string(),
                access_key: ui.site_access.get().trim().to_string(),
                secret: ui.site_secret.get(),
                path_style: ui.site_path_style.get() == 1,
            };
            if crate::core::store::s3_upsert(&site).is_ok() {
                ui.sites
                    .set(crate::core::store::s3_list().unwrap_or_default());
            }
            ui.site_edit_show.set(false);
        }
    });
    let edit_dialog = input_dialog(
        ui.site_edit_show,
        lang::S3_SITE_TITLE(),
        620,
        move |_| edit_close.set(false),
        Element::col()
            .width_match()
            .spacing(10)
            .child(
                Element::text_input(ui.site_name, lang::SFTP_SITE_NAME())
                    .autofocus()
                    .width_match(),
            )
            .child(Element::text_input(ui.site_endpoint, lang::S3_ENDPOINT()).width_match())
            .child(Element::text_input(ui.site_region, lang::S3_REGION()).width_match())
            .child(Element::text_input(ui.site_bucket, lang::S3_BUCKET()).width_match())
            .child(Element::text_input(ui.site_access, lang::S3_ACCESS()).width_match())
            .child(
                Element::text_input(ui.site_secret, lang::S3_SECRET())
                    .password()
                    .width_match(),
            )
            .child(
                Element::row()
                    .spacing(8)
                    .cross(Align::Center)
                    .child(Element::label(lang::S3_PATH_STYLE()).font_size(13.0))
                    .child(
                        Element::dropdown(
                            vec![lang::S3_VHOST_STYLE(), lang::S3_PATH_STYLE_OPT()],
                            ui.site_path_style,
                        )
                        .width(180),
                    ),
            ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click({
                        let ui = ui.clone();
                        move |_| ui.site_edit_show.set(false)
                    }),
            )
            .child(save_btn),
    );

    // ── 站点管理弹窗：单层双栏（左列表 + 右表单），点行即填右侧表单，
    // 保存/删除都在同一层完成，不再弹二级编辑窗 ──
    let mgr_close = ui.site_mgr_show;
    let _ui_mgr_close = ui.site_mgr_show;
    let mgr_form_ui = ui.clone();
    let mgr_list = Element::list_signal(
        ui.sites,
        |s: &crate::core::store::S3Site| s.id,
        move |s: crate::core::store::S3Site| {
            let (sel, sites) = (mgr_form_ui.site_sel, mgr_form_ui.sites);
            let form_ui = mgr_form_ui.clone();
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
                        form_ui.site_endpoint.set(x.endpoint.clone());
                        form_ui.site_region.set(x.region.clone());
                        form_ui.site_bucket.set(x.bucket.clone());
                        form_ui.site_access.set(x.access_key.clone());
                        form_ui.site_secret.set(x.secret.clone());
                        form_ui
                            .site_path_style
                            .set(if x.path_style { 1 } else { 0 });
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
                    Element::label(s.bucket.clone())
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                )
        },
    );
    // 双栏容器：左侧列表（可滚动）+ 新建，右侧表单 + 底部操作
    let form_save = Element::button(lang::SFTP_SITE_SAVE()).small().on_click({
        let (_show, id) = (ui.site_edit_show, ui.site_edit_id);
        let (name, endpoint, region, bucket) = (
            ui.site_name,
            ui.site_endpoint,
            ui.site_region,
            ui.site_bucket,
        );
        let (access, secret, path_style) = (ui.site_access, ui.site_secret, ui.site_path_style);
        let sites = ui.sites;
        move |_| {
            let n = name.get().trim().to_string();
            if n.is_empty() {
                return;
            }
            let site = crate::core::store::S3Site {
                id: id.get(),
                name: n,
                endpoint: endpoint.get().trim().trim_end_matches('/').to_string(),
                region: region.get().trim().to_string(),
                bucket: bucket.get().trim().to_string(),
                access_key: access.get().trim().to_string(),
                secret: secret.get(),
                path_style: path_style.get() == 1,
            };
            if crate::core::store::s3_upsert(&site).is_ok() {
                sites.set(crate::core::store::s3_list().unwrap_or_default());
            }
            // 保存后不关弹窗：继续编辑/新建（关闭仅靠 X / Esc / 点遮罩）
        }
    });
    let form_del = Element::button(lang::S3_SITE_DEL())
        .small()
        .neutral()
        .danger()
        .on_click({
            let (id, show) = (ui.site_edit_id, ui.site_edit_show);
            let sites = ui.sites;
            move |_| {
                let idv = id.get();
                if idv > 0 && crate::core::store::s3_del(idv).is_ok() {
                    sites.set(crate::core::store::s3_list().unwrap_or_default());
                    // 删除后也不关窗，表单重置为新建态
                    id.set(0);
                    let _ = show;
                }
            }
        });
    let mgr_body = Element::row()
        .width_match()
        .height(320)
        .spacing(12)
        // 左栏：站点列表（新建等操作统一放弹窗底部按钮排）
        .child(crate::widgets::mgr_list_col(
            mgr_list,
            ui.sites.map(|s| s.is_empty()),
        ))
        // 右栏：表单卡片（点列表行回填；新建按钮清空）
        .child(crate::widgets::mgr_form_col(
            Element::col()
                .width_match()
                .height_match()
                .spacing(8)
                .child(
                    Element::text_input(ui.site_name, lang::SFTP_SITE_NAME())
                        .autofocus()
                        .width_match(),
                )
                .child(Element::text_input(ui.site_endpoint, lang::S3_ENDPOINT()).width_match())
                .child(
                    Element::row()
                        .spacing(8)
                        .child(Element::text_input(ui.site_region, lang::S3_REGION()).weight(1.0))
                        .child(Element::text_input(ui.site_bucket, lang::S3_BUCKET()).weight(1.0)),
                )
                .child(
                    Element::row()
                        .spacing(8)
                        .child(Element::text_input(ui.site_access, lang::S3_ACCESS()).weight(1.0))
                        .child(
                            Element::text_input(ui.site_secret, lang::S3_SECRET())
                                .password()
                                .weight(1.0),
                        ),
                )
                .child(
                    Element::row()
                        .spacing(8)
                        .cross(Align::Center)
                        .child(Element::label(lang::S3_PATH_STYLE()).font_size(13.0))
                        .child(
                            Element::dropdown(
                                vec![lang::S3_VHOST_STYLE(), lang::S3_PATH_STYLE_OPT()],
                                ui.site_path_style,
                            )
                            .width(180),
                        ),
                )
                .child(Element::flex_spacer()),
        ));
    // 底部按钮排：新建（清空表单）/ 删除（编辑态可用）/ 保存 / 确定
    let mgr_new = Element::button(lang::S3_SITE_NEW())
        .small()
        .neutral()
        .on_click({
            let (id, name, endpoint, region) = (
                ui.site_edit_id,
                ui.site_name,
                ui.site_endpoint,
                ui.site_region,
            );
            let (bucket, access, secret, path_style) = (
                ui.site_bucket,
                ui.site_access,
                ui.site_secret,
                ui.site_path_style,
            );
            move |_| {
                id.set(0);
                // 新建态默认填示例数据，可直接改后保存
                name.set("示例站点".to_string());
                endpoint.set("https://s3.example.com".to_string());
                region.set(String::new());
                bucket.set("my-bucket".to_string());
                access.set(String::new());
                secret.set(String::new());
                path_style.set(0);
            }
        });
    let site_mgr_dialog = crate::widgets::mgr_dialog(
        ui.site_mgr_show,
        lang::S3_SITE_TITLE(),
        620,
        move |_| mgr_close.set(false),
        mgr_body,
        mgr_new,
        form_del,
        form_save,
    );

    // ── 恢复确认弹窗：确认后先下载备份到临时目录，on_msg 的 Done 分支完成覆盖 ──
    let (res_ui, res_cmd) = (ui.clone(), cmd.clone());
    let restore_dialog = input_dialog(
        ui.restore_show,
        lang::S3_RESTORE(),
        620,
        move |_| res_ui.restore_show.set(false),
        Element::col().width_match().child(
            Element::label_signal(ui.restore_msg)
                .font_size(14.0)
                .fg_role(Role::Danger),
        ),
        Element::row()
            .width_match()
            .child(Element::flex_spacer())
            .child(
                Element::button(lang::SFTP_CANCEL())
                    .small()
                    .neutral()
                    .on_click({
                        let ui = ui.clone();
                        move |_| ui.restore_show.set(false)
                    }),
            )
            .child(
                Element::button(lang::SFTP_OK())
                    .small()
                    .danger()
                    .on_click(move |_| {
                        res_ui.restore_show.set(false);
                        let sel = res_ui.selected.get();
                        if let (Some(cred), Some(name)) = (res_ui.cred(), sel.first().cloned()) {
                            let key = s3::join_key(&res_ui.prefix.get(), &name, false);
                            let _ = res_cmd.send(s3::S3Cmd::Download {
                                cred,
                                key,
                                local_dir: std::env::temp_dir().to_string_lossy().into_owned(),
                            });
                            res_ui.restoring.set(true);
                            res_ui.status.set(lang::S3_DOWNLOADING().to_string());
                        }
                    }),
            ),
    );

    // ── 页面；弹窗由调用方挂根层级（遮罩铺满全窗）──
    let page = Element::stack().fill().child(
        Element::col()
            .padding(12)
            .spacing(8)
            .child(site_row)
            .child(toolbar)
            .child(path_row)
            .child(list_area),
    );
    let dialogs = Element::stack()
        .fill()
        .child(mkdir_dialog)
        .child(delete_dialog)
        .child(edit_dialog)
        .child(site_mgr_dialog)
        .child(restore_dialog);

    // 弹窗统一由根层级挂载：ModalScrim 遮罩铺满根节点，模态覆盖整窗（含侧栏）
    (page, dialogs)
}

/// 备份文件名时间戳：YYYYMMDD-HHMMSS（本地时间）
fn chrono_now_stamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // 按东八区近似折算（备份名只求可读排序，不要求精确时区）
    let days = now.div_euclid(86_400);
    let secs = now.rem_euclid(86_400) + 8 * 3600;
    // civ 日期算法（Howard Hinnant）把 Unix 天数转 Y-M-D
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (h, mi, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", y, m, d, h, mi, s)
}

/// PNG 保存对话框复用 [`crate::widgets`]；文件/目录选择同样走 widgets 共享助手。

#[cfg(test)]
mod tests {
    use crate::core::fmt::format_size;

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(204800), "200.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
