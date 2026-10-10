//! 横向可滚动的数据表格（应用侧自定义 Widget，不改 windui）。
//!
//! windui 的 `Element::scroll` 只支持纵向滚动（`Layout::Scroll` 仅 `scroll_y`），
//! 官方表格列宽按视口权重分配、超长单元格裁成省略号，宽表看不全。本组件把整张表
//! （表头 + 正文 + 查看按钮）画在一个叶子 Widget 里：列宽按内容固定、横向偏移
//! `scroll_x` 自管，纵向滚动仍交给外层 `Element::scroll`（行数 ≤ 100/页，无需虚拟化）。
//!
//! 交互：
//! - 滚轮直滚交给外层（返回 false 冒泡）；**Shift+滚轮** 横向滚动；
//! - 底部横向滚动条可拖动（几何与核心层纵向滚动条同款规格）；
//! - 行首「查看」按钮点击进入行详情（写 `meta` 的 `selected_row`，与原表格共用状态）。

use windui::core::{EventCtx, Widget};
use windui::event::{Event, PointerKind};
use windui::geometry::{Rect, Size};
use windui::prelude::*;
use windui::render::{Canvas, Paint};
use windui::style::Style;
use windui::text::TextEngine;
use windui::theme;

use crate::core::db::TablePage;

/// 行高（逻辑 px），与 table.rs 的 `ROW_H` 一致，外层滚动条尺寸才能对上。
const ROW_H: i32 = 32;
/// 「查看」按钮列宽。
const VIEW_W: i32 = 26;
/// 列内边距（左右各一半）。
const CELL_PAD: i32 = 16;
/// 列宽上下限：窄列不至于读不了，宽列不至于一列霸屏。
const COL_W_MIN: i32 = 64;
const COL_W_MAX: i32 = 520;
/// 单元格展示上限（字符），与 table.rs 的 CELL_DISPLAY_MAX 对齐。
const CELL_MAX: usize = 200;
const FONT: f32 = 13.0;

/// 估算字符串像素宽：ASCII 窄、CJK 宽。确定性估算，不做逐字排版测量
/// （measure 阶段拿不到 Canvas::measure_text，精度对表格列宽足够）。
fn text_w(s: &str) -> i32 {
    s.chars()
        .take(CELL_MAX)
        .map(|c| if c.is_ascii() { 8 } else { 16 })
        .sum()
}

/// 裁剪展示文本：超上限截断（列宽估算同源，不会再撑宽）。
fn clip_cell(cell: &str) -> String {
    if cell.chars().count() <= CELL_MAX {
        cell.to_string()
    } else {
        cell.chars().take(CELL_MAX).collect()
    }
}

/// 按内容估宽：列名与该列所有行（已裁剪展示文本）取最大。
fn col_widths(page: &TablePage, rows: &[Vec<String>]) -> Vec<i32> {
    let mut ws = Vec::with_capacity(page.columns.len());
    for (i, name) in page.columns.iter().enumerate() {
        let mut w = text_w(name) + CELL_PAD;
        for row in rows.iter().take(200) {
            if let Some(c) = row.get(i) {
                w = w.max(text_w(&clip_cell(c)) + CELL_PAD);
            }
        }
        ws.push(w.clamp(COL_W_MIN, COL_W_MAX));
    }
    ws
}

/// 横向滚动表格 widget。数据走信号（与 table.rs 的 meta/rows 同一对），
/// `scroll_x` 与拖动态挂在 widget 内（Cell：paint 是 &self）。
struct HTable {
    meta: Signal<Vec<TablePage>>,
    rows: Signal<Vec<Vec<String>>>,
    scroll_x: std::cell::Cell<i32>,
    content_w: std::cell::Cell<i32>,
    /// 拖动横向滚动条：(按下 x，按下时 scroll_x)。
    drag: std::cell::Cell<Option<(i32, i32)>>,
}

impl HTable {
    fn max_scroll(&self, view_w: i32) -> i32 {
        (self.content_w.get() - view_w).max(0)
    }
}

impl Widget for HTable {
    fn measure(&self, _avail: Size, _s: &Style, _t: &mut dyn TextEngine) -> Size {
        // 固有内容尺寸：宽度给足内容（供外层判断），高度 = 表头 + 行数。
        // 宽度在 measure 阶段按信号现算（信号读取无 &mut 需求）。
        let meta = self.meta.get();
        let rows = self.rows.get();
        let Some(page) = meta.first() else {
            return Size::ZERO;
        };
        let w: i32 = VIEW_W + col_widths(page, &rows).iter().sum::<i32>();
        let h = ROW_H * (rows.len() as i32 + 1);
        self.content_w.set(w);
        Size::new(w.max(1), h)
    }

    fn paint(
        &self,
        _bounds: Rect,
        content: Rect,
        _f: bool,
        _e: bool,
        canvas: &mut dyn Canvas,
        _s: &Style,
    ) {
        let th = theme::current();
        let p = &th.palette;
        let meta = self.meta.get();
        let rows = self.rows.get();
        let Some(page) = meta.first() else { return };

        let widths = col_widths(page, &rows);
        let scroll_x = self
            .scroll_x
            .get()
            .clamp(0, (self.content_w.get() - content.w).max(0));
        self.scroll_x.set(scroll_x);

        let ts = windui::text::TextStyle::new(FONT);
        let mut x0 = content.x as f32 - scroll_x as f32;

        // ── 表头 ──
        canvas.fill_rect(
            content.x as f32,
            content.y as f32,
            content.w as f32,
            ROW_H as f32,
            &Paint::fill(p.surface_alt),
        );
        // 查看按钮列占位（表头留白与正文按钮同宽）
        x0 += VIEW_W as f32;
        for (i, name) in page.columns.iter().enumerate() {
            let Some(page) = meta.first() else { break };
            if !page.visible.get(i).copied().unwrap_or(true) {
                continue;
            }
            let w = widths[i] as f32;
            let rect = Rect::new(x0 as i32 + 8, content.y + 6, w as i32 - 16, ROW_H - 12);
            canvas.draw_text(name, rect, p.text_muted, windui::spec::Align::Start, &ts);
            x0 += w;
        }
        // 表头列分隔线
        let mut xl = content.x as f32 + VIEW_W as f32 - scroll_x as f32;
        for (i, _) in page.columns.iter().enumerate() {
            if !page.visible.get(i).copied().unwrap_or(true) {
                continue;
            }
            xl += widths[i] as f32;
            if xl > content.x as f32 && xl < content.right() as f32 {
                canvas.draw_line(
                    xl,
                    content.y as f32,
                    xl,
                    (content.y + content.h) as f32,
                    1.0,
                    &Paint::fill(p.divider),
                );
            }
        }
        canvas.draw_line(
            content.x as f32,
            (content.y + ROW_H) as f32,
            content.right() as f32,
            (content.y + ROW_H) as f32,
            1.0,
            &Paint::fill(p.divider),
        );

        // ── 正文行 ──
        canvas.save();
        canvas.clip_rect(Rect::new(
            content.x,
            content.y + ROW_H,
            content.w,
            (content.h - ROW_H).max(0),
        ));
        for (ri, row) in rows.iter().enumerate() {
            let y = content.y + ROW_H + (ri as i32) * ROW_H;
            if ri % 2 == 1 {
                canvas.fill_rect(
                    content.x as f32,
                    y as f32,
                    content.w as f32,
                    ROW_H as f32,
                    &Paint::fill(p.surface_alt),
                );
            }
            let mut xc = content.x as f32 + VIEW_W as f32 - scroll_x as f32;
            for (i, _) in page.columns.iter().enumerate() {
                let visible = page.visible.get(i).copied().unwrap_or(true);
                let w = widths[i] as f32;
                if visible {
                    let cell = row.get(i).map(String::as_str).unwrap_or("");
                    let is_null = cell == "NULL";
                    let rect = Rect::new(xc as i32 + 8, y + 6, w as i32 - 16, ROW_H - 12);
                    canvas.draw_text(
                        &clip_cell(cell),
                        rect,
                        if is_null { p.text_muted } else { p.text },
                        windui::spec::Align::Start,
                        &ts,
                    );
                }
                xc += w;
            }
        }
        canvas.restore();

        // ── 底部横向滚动条 ──
        let max_scroll = (self.content_w.get() - content.w).max(0);
        if max_scroll > 0 {
            let track_w = 7.0f32;
            let ty = content.bottom() as f32 - track_w - 3.0;
            let tw = content.w as f32;
            let ratio = content.w as f32 / self.content_w.get().max(1) as f32;
            let thumb_w = (tw * ratio).max(24.0);
            let travel = tw - thumb_w;
            let thumb_x = content.x as f32 + travel * (scroll_x as f32 / max_scroll as f32);
            canvas.fill_round_rect(
                content.x as f32,
                ty,
                tw,
                track_w,
                track_w / 2.0,
                &Paint::fill(p.divider.scale_alpha(0.5)),
            );
            canvas.fill_round_rect(
                thumb_x,
                ty,
                thumb_w,
                track_w,
                track_w / 2.0,
                &Paint::fill(p.border),
            );
        }
    }

    fn on_event(&mut self, ctx: &mut EventCtx, ev: &Event) -> bool {
        let Event::Pointer(p) = ev else { return false };
        let b = ctx.bounds();
        let content = b; // 叶子节点无 padding
        match p.kind {
            PointerKind::Wheel(d) if p.mods.shift => {
                // Shift+滚轮：横向滚动。直滚（无 Shift）不消费 → 冒泡给外层纵向滚动。
                // （windui 未提供横向滚轮事件，Shift+滚轮是桌面通行的横向滚动手势。）
                let view_w = content.w;
                let max = self.max_scroll(view_w);
                if max == 0 {
                    return false;
                }
                let next = (self.scroll_x.get() - d * 48 / 120).clamp(0, max);
                if next != self.scroll_x.get() {
                    self.scroll_x.set(next);
                    ctx.mark_dirty();
                }
                true
            }
            PointerKind::Down => {
                // 底部滚动条拖动
                let max = self.max_scroll(content.w);
                if max > 0 && p.pos.y >= content.bottom() - 16 {
                    self.drag.set(Some((p.pos.x, self.scroll_x.get())));
                    ctx.capture();
                    return true;
                }
                // 「查看」按钮：行首 26px 列
                let rows = self.rows.get();
                let rel_y = p.pos.y - content.y - ROW_H;
                if rel_y >= 0 && p.pos.x - content.x < VIEW_W {
                    let ri = (rel_y / ROW_H) as usize;
                    if ri < rows.len() {
                        self.meta.update(|v| {
                            if let Some(pg) = v.first_mut() {
                                pg.selected_row = Some(ri);
                            }
                        });
                        return true;
                    }
                }
                false
            }
            PointerKind::Move if self.drag.get().is_some() => {
                let max = self.max_scroll(content.w);
                let (start_x, start_scroll) = self.drag.get().unwrap();
                let ratio = content.w as f32 / self.content_w.get().max(1) as f32;
                let thumb_w = (content.w as f32 * ratio).max(24.0);
                let travel = (content.w as f32 - thumb_w).max(1.0);
                let dx = p.pos.x - start_x;
                let next = (start_scroll as f32 + dx as f32 * max as f32 / travel) as i32;
                self.scroll_x.set(next.clamp(0, max));
                ctx.mark_dirty();
                true
            }
            PointerKind::Up if self.drag.get().is_some() => {
                self.drag.set(None);
                ctx.release_capture();
                true
            }
            _ => false,
        }
    }
}

/// 横向可滚动表格：数据与 table.rs 同一对信号（meta 承载列/可见性/选中行）。
/// 放进 `Element::scroll` 使用（纵向滚动由外层负责）。
pub fn htable(meta: Signal<Vec<TablePage>>, rows: Signal<Vec<Vec<String>>>) -> Element {
    Element::leaf().widget(HTable {
        meta,
        rows,
        scroll_x: std::cell::Cell::new(0),
        content_w: std::cell::Cell::new(0),
        drag: std::cell::Cell::new(None),
    })
}
