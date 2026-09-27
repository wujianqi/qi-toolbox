//! SFTP 文件列表行自绘控件（从 `sftp.rs` 拆出）。
//!
//! [`FileRow`]：背景/悬停/勾选标记/图标/文件名/大小全部自绘（行高 30）。
//!
//! 为什么不用 `label + clickable + 勾选框覆盖层`：常驻勾选框让列表显得表单化，
//! 且勾选后 `entries.set` 整列表重建会丢掉所有行的悬停态（光标下的勾选框闪没）。
//! 本控件把悬停做进实例状态、选中态每帧读信号绘制，切换选中只整窗标脏——
//! 勾选标记仅在悬停/选中时浮现，列表平时完全干净，且无重建闪烁。

use std::cell::Cell;
use std::sync::mpsc;

use windui::core::{EventCtx, Widget};
use windui::event::{CursorShape, Event, MouseButton, PointerKind};
use windui::geometry::Rect;
use windui::prelude::*;
use windui::render::{Canvas, Paint};
use windui::spec::Align;
use windui::style::Style;
use windui::text::{TextEngine, TextStyle};

use super::icons;
use crate::core::sftp;

/// 行图标素材：SVG 一次解析（彩色素材自带配色，不参与主题染色），
/// 按扩展名/选中态挑选（目录/压缩包/图片/代码/配置/日志/通用文件，
/// 选中行用 `_CHECKED` 变体）。`Image` 内部 Rc 共享，克隆廉价——整列表共享一份。
#[derive(Clone)]
pub(super) struct RowArt {
    folder: Image,
    folder_checked: Image,
    file: Image,
    file_checked: Image,
    archive: Image,
    archive_checked: Image,
    image: Image,
    image_checked: Image,
    config: Image,
    config_checked: Image,
    log: Image,
    log_checked: Image,
}

impl RowArt {
    pub(super) fn new() -> Self {
        let mk =
            |bytes: &[u8]| Image::from_svg_bytes(bytes, Some(15)).expect("内置 SVG 必然可解析");
        Self {
            folder: mk(icons::FOLDER),
            folder_checked: mk(icons::FOLDER_SELECTED),
            file: mk(icons::GENERIC_FILE),
            file_checked: mk(icons::GENERIC_FILE_SELECTED),
            archive: mk(icons::ARCHIVE_FILE),
            archive_checked: mk(icons::ARCHIVE_FILE_SELECTED),
            image: mk(icons::IMAGE_FILE),
            image_checked: mk(icons::IMAGE_FILE_SELECTED),
            config: mk(icons::CONFIG_FILE),
            config_checked: mk(icons::CONFIG_FILE_SELECTED),
            log: mk(icons::LOG_FILE),
            log_checked: mk(icons::LOG_FILE_SELECTED),
        }
    }

    /// 按文件扩展名 + 选中态挑图标（目录除外）：压缩包 / 图片 / 配置 / 日志
    /// 用彩色素材（选中 = checked 变体），代码 / 其余回落到通用文件描边图标。
    fn pick(&self, name: &str, is_dir: bool, selected: bool) -> &Image {
        if is_dir {
            return if selected {
                &self.folder_checked
            } else {
                &self.folder
            };
        }
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e)
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            // 压缩包
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" => {
                if selected {
                    &self.archive_checked
                } else {
                    &self.archive
                }
            }
            // 图片
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" => {
                if selected {
                    &self.image_checked
                } else {
                    &self.image
                }
            }
            // 配置
            "json" | "yaml" | "yml" | "toml" | "ini" | "conf" | "cfg" | "env" | "xml" => {
                if selected {
                    &self.config_checked
                } else {
                    &self.config
                }
            }
            // 日志
            "log" => {
                if selected {
                    &self.log_checked
                } else {
                    &self.log
                }
            }
            // 其余暂未设图标的类型一律视为通用文件
            _ => {
                if selected {
                    &self.file_checked
                } else {
                    &self.file
                }
            }
        }
    }
}

/// 文件列表行：背景/悬停/勾选标记/图标/文件名/大小全部自绘（行高 30）。
pub(super) struct FileRow {
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
    pub(super) fn new(
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
        Size::new(avail.w.max(0), 26)
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

        // 行首槽位（16×16）：选中行画 checked 变体图标（替代勾选框，风格统一）；
        // 其余时刻（含悬停）画原图标。
        let slot_x = x + SLOT_X;
        let slot_y = y + (h - CHECK_SIZE) / 2.0;
        let img = self.art.pick(&self.entry.name, self.entry.is_dir, sel);
        canvas.draw_image(
            img,
            Rect::new(
                slot_x as i32,
                slot_y as i32,
                CHECK_SIZE as i32,
                CHECK_SIZE as i32,
            ),
            Fit::Contain,
            0.0,
            1.0,
        );

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
                        // 点行首图标 = 选中/取消目录（checked 变体常驻显示，图标即
                        // 选中指示，不依赖悬停态）；点行其余区域 = 进入目录。
                        if rel < CHECK_HIT {
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
