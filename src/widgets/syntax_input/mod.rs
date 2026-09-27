//! 语法高亮文本编辑控件（多行、无自动换行、水平/垂直滚动）。
//!
//! 基于 windui `Widget` trait 自绘的轻量代码输入框：多行文本编辑 + 光标/选区 +
//! 剪贴板 + 撤销/重做 + 输入法定位，并用词法分析器把正文按 token 分色绘制。
//!
//! # 设计要点（与 windui 自带的 `TextInput` 同一套纪律）
//!
//! - **几何一律来自最近一帧 paint/measure 建好的布局缓存**。控件在
//!   [`measure`](Widget::measure)/[`paint`](Widget::paint) 里用 style 的真实字号与行高
//!   重建 `Layout`（每行字符 x 坐标、行高、字号）并存入 `RefCell`；`on_event` 里读同一份
//!   缓存做命中测试与光标换算——**绝不在事件回调里用写死的字号/行高**（那正是"点下去光标
//!   跑到别处"的根因：回调按假行高换算，绘制却按真行高排布，两者对不上）。
//! - **文本一改就在下一帧 paint 重建布局**，故编辑后立刻点/键的几何总是新鲜的。
//!   重建时前缀测宽走**逐行缓存**（键 = 行内容 + 度量指纹）：逐键编辑只改一行，
//!   其余行不再 O(行长²) 重测——这是长文档编辑卡顿的主要来源。
//! - 行模型：正文按 `\n` 切成行，每行 `LayoutRow{ start, x }`，`x[k]` 为第 k 个字符左缘
//!   相对行首的像素偏移（`x[0]=0`，`x.len()=字符数+1`，最后一个是行尾插入点）。
//! - win32 键语义：回车是 `Key::Enter`；可打印字符（含空格）经 WM_CHAR 以 `Key::Char`
//!   送达；Ctrl 组合（A/C/V/X/Z/Y）经 WM_KEYDOWN 以 `Key::Other(虚拟键码)` 送达。

pub mod shell_lexer;
pub mod sql_lexer;
pub mod test_harness;

use std::cell::{Cell, RefCell};

use windui::core::{EventCtx, Widget};
use windui::event::{CursorShape, Event, Key, KeyEvent, MouseButton, PointerKind};
use windui::geometry::{Color, Point, Rect, Size};
use windui::render::{Canvas, Paint};
use windui::signal::Signal;
use windui::spec::Align;
use windui::style::Style;
use windui::text::{TextEngine, TextStyle};
use windui::ui::caret::{CaretOpts, CaretState};

mod geometry;
mod lex;
#[cfg(test)]
mod tests;

use crate::widgets::sel_core::byte_at;
pub(crate) use geometry::MeasureKey;
use geometry::{b_left_offset, row_metrics, row_segs, Layout, Row, NO_WRAP_W, PAD};
pub use lex::{lex_shell, lex_sql, lex_sql_dialect, LexerKind, TokenKind};

/// 高亮主题色（解析自 windui 当前主题）。
struct HighlightColors {
    keyword: Color,
    string: Color,
    number: Color,
    comment: Color,
    operator: Color,
    command: Color,
    variable: Color,
    text: Color,
    cursor_color: Color,
}

impl HighlightColors {
    fn from_theme() -> Self {
        let t = windui::theme::current();
        let p = &t.palette;
        Self {
            keyword: p.accent,
            string: Color::rgb(163, 113, 247),
            number: Color::rgb(181, 137, 0),
            comment: p.text_muted,
            operator: p.text,
            command: Color::rgb(78, 165, 218),
            variable: Color::rgb(204, 102, 102),
            text: p.text,
            cursor_color: p.accent,
        }
    }

    /// 按 `TokenKind` 声明顺序（下标 = 枚举判别值）的 token 色：布局缓存键与
    /// paint 取色共用，保证两处语义一致（Flag/Plain 分别与 keyword/正文同色）。
    fn as_palette(&self) -> [Color; 9] {
        [
            self.keyword,  // Keyword
            self.string,   // String
            self.number,   // Number
            self.comment,  // Comment
            self.operator, // Operator
            self.keyword,  // Flag
            self.command,  // Command
            self.variable, // Variable
            self.text,     // Plain
        ]
    }

    fn color_for(&self, kind: TokenKind) -> Color {
        self.as_palette()[kind as usize]
    }
}

/// 逐行宽度缓存上限（条数）。超过即整体清空：长会话下旧行版本会随编辑不断
/// 累积，按条数兜底防缓存无限膨胀（单条 ≈ 行字节 + 4×字符数 字节）。
const WIDTH_CACHE_MAX_ENTRIES: usize = 8_192;

/// 撤销栈上限：条数 + 快照总字节。快照是全文克隆，只限条数时"反复粘贴
/// 大文本"可堆到上百 MB，按字节兜底防内存膨胀。
const UNDO_MAX_ENTRIES: usize = 200;
const UNDO_MAX_BYTES: usize = 4 * 1024 * 1024;

// ───────────────────────────────────────────────────────────────────────────
// 控件
// ───────────────────────────────────────────────────────────────────────────

/// 语法高亮文本编辑控件。
///
/// 多行（回车插入换行）、无自动换行；超宽行横向滚动、超高竖向滚动，自动跟随光标。
/// 支持：光标/选区、Shift 扩选、方向键/Home/End、双击选词、三击选行、拖拽选择、
/// Ctrl+A/C/V/X/Z/Y、输入法定位（win32 系统 IME 自绘合成串）。
pub struct SyntaxInput {
    text: Signal<String>,
    placeholder: String,
    lexer: LexerKind,

    // ── 编辑态（字符下标，全文含换行符）──
    cursor: Cell<usize>,
    anchor: Cell<Option<usize>>,
    /// ↑/↓ 时保持的目标列（字符列）；水平移动/编辑后清空。
    goal_col: Cell<Option<usize>>,

    // ── 几何（paint/measure 每帧重建）──
    layout: RefCell<Layout>,
    scroll_x: Cell<f32>,
    scroll_y: Cell<f32>,
    /// true 时下一帧 paint 把视口滚到光标处。
    follow: Cell<bool>,
    dragging: Cell<bool>,

    // ── 焦点 / IME ──
    composing: Cell<bool>,
    /// 最近一帧光标在**节点局部**坐标 (x, y_top, height)，供输入法定位。
    caret_local: Cell<Option<(i32, i32, i32)>>,
    /// 插入光标闪烁（对齐官方 TextInput 的 [`CaretState`]：框架托管方波闪烁、
    /// 点击后实心保持、失焦清除，并自报脏区续帧；不再自画一条常亮光标）。
    caret: CaretState,

    // ── 撤销 / 重做 ──
    undo_stack: RefCell<Vec<(String, usize)>>,
    redo_stack: RefCell<Vec<(String, usize)>>,
}

impl SyntaxInput {
    pub fn new(text: Signal<String>, placeholder: impl Into<String>, lexer: LexerKind) -> Self {
        let cursor = text.with(|s| s.chars().count());
        Self {
            text,
            placeholder: placeholder.into(),
            lexer,
            cursor: Cell::new(cursor),
            anchor: Cell::new(None),
            goal_col: Cell::new(None),
            layout: RefCell::new(Layout::empty()),
            scroll_x: Cell::new(0.0),
            scroll_y: Cell::new(0.0),
            follow: Cell::new(true),
            dragging: Cell::new(false),
            composing: Cell::new(false),
            caret_local: Cell::new(None),
            caret: CaretState::new(),
            undo_stack: RefCell::new(Vec::new()),
            redo_stack: RefCell::new(Vec::new()),
        }
    }

    // ── 文本 / 字节换算 ──────────────────────────────────────────────

    fn text_clone(&self) -> String {
        self.text.with(|s| s.clone())
    }

    fn char_count(&self) -> usize {
        // 布局与当前文本一致时直接用缓存的 total（memcmp 级校验，远快于逐字符
        // 计数）——selection/clamp 每帧多处调用，长文档下 O(N) 计数是白付的。
        let lay = self.layout.borrow();
        if lay.valid && self.text.with(|s| lay.text.as_str() == *s) {
            return lay.total;
        }
        drop(lay);
        self.text.with(|s| s.chars().count())
    }

    /// 规范化选区 [start, end)。无选区返回 None。
    fn selection(&self) -> Option<(usize, usize)> {
        let total = self.char_count();
        crate::widgets::sel_core::normalize_selection(self.cursor.get(), self.anchor.get(), total)
    }

    /// 光标/锚点钳制到合法字符下标 [0, 文本长度]（文本被外部改写后可能悬空/越界）。
    /// 仅当本帧**真的修正了越界值**、且修正后锚点塌缩到光标处时才清掉锚点
    /// （空选区无意义）；未越界时绝不碰 anchor——拖选起始帧 `anchor == cursor`
    /// 是正常起始态，若每帧 paint 都清掉，之后 Move 更新 cursor 时 anchor 已丢失，
    /// 整段拖选就没有高亮（表现为"只有双击才有选中效果"）。
    /// 返回是否发生变化。
    fn clamp_cursor(&self) -> bool {
        let total = self.char_count();
        let mut changed = false;
        let c = self.cursor.get();
        if c > total {
            self.cursor.set(total);
            changed = true;
        }
        let nc = self.cursor.get();
        if let Some(a) = self.anchor.get() {
            if a > total {
                self.anchor.set(Some(total));
                changed = true;
            }
        }
        if changed && self.anchor.get() == Some(nc) {
            self.anchor.set(None);
        }
        changed
    }

    // ── 布局重建 ─────────────────────────────────────────────────────

    /// `scale` 为 DPI 缩放因子（measure 用 `TextEngine::scale()`，paint 用
    /// `Canvas::dpi_scale()`——同源，否则跨分辨率拖动后两帧缓存互踢）。
    /// `ts` 供宽度缓存取度量指纹（字族/字重/斜体变化时缓存作废）。
    fn rebuild(
        &self,
        font: f32,
        line_h: f32,
        scale: f32,
        ts: &TextStyle<'_>,
        palette: &[Color],
        mut measure: impl FnMut(&str) -> f32,
    ) {
        let mkey = MeasureKey::of(ts, scale);
        let mut lay = self.layout.borrow_mut();
        // 缓存键用无分配比较（&str 直比 / 色板按位比）：未变则跳过重建，
        // 不克隆文本——measure 与 paint 每帧各调一次本函数。键 = f(文本, 字号,
        // 行高, DPI, 词法器, 色板, 度量指纹)；全未变则沿用上次布局（对齐官方
        // TextInput 的 key 缓存：光标移动/闪烁等无关重绘不触发布局）。
        let text_changed = self.text.with(|s| lay.text.as_str() != *s);
        if lay.rows_valid()
            && !text_changed
            && lay.font == font
            && lay.line_h == line_h
            && lay.scale == scale
            && lay.lexer == self.lexer
            && lay.palette.as_slice() == palette
            && lay.wkey.as_ref() == Some(&mkey)
        {
            return;
        }
        // 度量指纹变了（字族/字号/DPI）→ 旧宽度全部作废；顺带按条数兜底清缓存。
        if lay.wkey.as_ref() != Some(&mkey) {
            lay.widths.clear();
            lay.wkey = Some(mkey);
        }
        if lay.widths.len() > WIDTH_CACHE_MAX_ENTRIES {
            lay.widths.clear();
        }
        let text = self.text_clone();
        // 按 '\n' 切逻辑行重建（与旧逐字符扫描语义一致：'\r' 留在行内、结尾
        // '\n' 补空行、空文本产出一空行）。每行宽度优先命中逐行缓存。
        let mut rows: Vec<Row> = Vec::new();
        let mut gi = 0usize;
        for line in text.split('\n') {
            let lc = line.chars().count();
            let (x, bmap) = row_metrics(line, font, &mut measure, &mut lay.widths);
            rows.push(Row {
                start: gi,
                x,
                text: line.to_owned(),
                bmap,
                runs: Vec::new(),
            });
            gi += lc + 1;
        }
        // 词法只在建布局时跑一次，run 按显示行归位。token 按全文顺序到达：
        // 行指针只前进不回退（原先每 token `rows.iter().position` 全扫，
        // O(token×行数) 在长文档上是平方级）。行尾 '\n' 被单行/多行注释 token
        // 吞进 span 时，run 末列按行长钳制——注释在行内可见，'\n' 本身不入行。
        let tokens = match self.lexer {
            LexerKind::Sql => lex_sql(&text),
            LexerKind::SqlMySql => lex_sql_dialect(&text, sql_lexer::Dialect::MySql),
            LexerKind::SqlPg => lex_sql_dialect(&text, sql_lexer::Dialect::Postgres),
            LexerKind::Shell => lex_shell(&text),
        };
        let mut col = 0usize;
        let mut row_i = 0usize;
        for tok in tokens {
            let span = tok.text.chars().count();
            if span == 0 {
                continue;
            }
            while row_i + 1 < rows.len() && col >= rows[row_i].end_full() {
                row_i += 1;
            }
            let r = &mut rows[row_i];
            if col >= r.start && col < r.end_full() {
                let cs = col - r.start;
                r.runs.push((tok.kind, cs, (cs + span).min(r.len())));
            }
            col += span;
        }
        lay.rows = rows;
        lay.line_h = line_h;
        lay.font = font;
        lay.total = text.chars().count();
        lay.scale = scale;
        lay.text = text;
        lay.lexer = self.lexer;
        lay.palette = palette.to_vec();
        lay.valid = true;
    }

    // ── 几何换算（读最近一次 layout）────────────────────────────────

    /// 视图可用宽/高（bounds 内缩 PAD）。
    fn view_size(b: Rect) -> (f32, f32) {
        let w = (b.w - 2 * PAD).max(0) as f32;
        let h = (b.h - 2 * PAD).max(0) as f32;
        (w, h)
    }

    fn row_top(&self, b: Rect, line_h: f32, row: usize) -> f32 {
        b.y as f32 + PAD as f32 + row as f32 * line_h - self.scroll_y.get()
    }

    /// 绝对画布坐标 → 字符下标。与绘制同源：内容坐标系原点在 bounds 左上 + PAD，
    /// 绘制时再按 scroll 平移，故命中前把坐标减 PAD、加回滚动量。
    fn hit_index(&self, b: Rect, px: f32, py: f32) -> usize {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            // 空布局的 total 是 0，不能拿来钳——用真实文本长度。
            return self.cursor.get().min(self.char_count());
        }
        // 内容坐标（相对"bounds 左上 + PAD"的原点，未滚动）。
        let cx = px - (b.x as f32 + PAD as f32) + self.scroll_x.get();
        let cy = py - (b.y as f32 + PAD as f32) + self.scroll_y.get();
        let row = crate::widgets::sel_core::row_at_y(cy, lay.line_h, lay.rows.len());
        let r = &lay.rows[row];
        // x 单调不减：二分定位 cx 所在列（原为 O(len) 全扫，长行逐帧拖选会卡）。
        // cx 在某字符中部时按半开区间归属——中点之前算左、之后算右。
        let col = crate::widgets::sel_core::col_at_x(&r.x, cx);
        (r.start + col).min(lay.total)
    }

    // ── 编辑操作 ─────────────────────────────────────────────────────

    /// 撤销/重做入栈前的当前状态快照。
    fn snapshot(&self) -> (String, usize) {
        (self.text_clone(), self.cursor.get())
    }

    fn push_undo(&self) {
        let mut u = self.undo_stack.borrow_mut();
        u.push(self.snapshot());
        self.redo_stack.borrow_mut().clear();
        // 双重上限（条数 + 快照总字节）：快照是全文克隆，只限条数时反复粘贴
        // 大文本可把栈堆到上百 MB。始终保底 1 条，保证最近一步可撤销。
        let mut bytes: usize = u.iter().map(|(t, _)| t.len()).sum();
        while u.len() > 1 && (u.len() > UNDO_MAX_ENTRIES || bytes > UNDO_MAX_BYTES) {
            bytes = bytes.saturating_sub(u[0].0.len());
            u.remove(0);
        }
    }

    /// 用 `ins` 替换 [start, end) 的字符区间，并把光标放到 `cursor_after`。
    fn apply_edit(&self, start: usize, end: usize, ins: &str, cursor_after: usize) {
        let total = self.char_count();
        let (s, e) = (start.min(total), end.min(total));
        let (bs, be) = self.text.with(|t| (byte_at(t, s), byte_at(t, e)));
        self.text.update(|t| {
            t.replace_range(bs..be, ins);
        });
        self.cursor.set(cursor_after);
        self.anchor.set(None);
        self.goal_col.set(None);
    }

    fn delete_selection(&self) -> bool {
        match self.selection() {
            Some((s, e)) => {
                self.push_undo();
                self.apply_edit(s, e, "", s);
                true
            }
            None => false,
        }
    }

    /// 打字：替换选区或光标处插入单个字符。`push_undo` 在改动前调用。
    fn type_char(&self, c: char) {
        self.push_undo();
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        let (s0, e0, after) = match self.selection() {
            Some((s0, e0)) => (s0, e0, s0 + 1),
            None => {
                let cur = self.cursor.get();
                (cur, cur, cur + 1)
            }
        };
        self.apply_edit(s0, e0, s, after);
    }

    fn insert_newline(&self) {
        self.push_undo();
        let (s0, e0, after) = match self.selection() {
            Some((s0, e0)) => (s0, e0, s0 + 1),
            None => {
                let cur = self.cursor.get();
                (cur, cur, cur + 1)
            }
        };
        self.apply_edit(s0, e0, "\n", after);
    }

    fn backspace(&self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.cursor.get();
        if cur == 0 {
            return;
        }
        self.push_undo();
        self.apply_edit(cur - 1, cur, "", cur - 1);
    }

    fn delete_forward(&self) {
        if self.delete_selection() {
            return;
        }
        let cur = self.cursor.get();
        if cur >= self.char_count() {
            return;
        }
        self.push_undo();
        self.apply_edit(cur, cur + 1, "", cur);
    }

    fn paste(&self, ins: &str) {
        if ins.is_empty() {
            return;
        }
        // 行模型只认 '\n'：统一换行，并把 \r\n / 孤立 \r 折叠成 \n。
        let mut norm = String::with_capacity(ins.len());
        let mut prev_cr = false;
        for c in ins.chars() {
            match c {
                '\r' => {
                    norm.push('\n');
                    prev_cr = true;
                }
                '\n' => {
                    if !prev_cr {
                        norm.push('\n');
                    }
                    prev_cr = false;
                }
                _ => {
                    norm.push(c);
                    prev_cr = false;
                }
            }
        }
        if norm.is_empty() {
            return;
        }
        self.push_undo();
        let cur = self.cursor.get();
        let (s0, e0) = match self.selection() {
            Some(se) => se,
            None => (cur, cur),
        };
        let n = norm.chars().count();
        self.apply_edit(s0, e0, &norm, s0 + n);
    }

    fn select_all(&self) {
        self.anchor.set(Some(0));
        self.cursor.set(self.char_count());
        self.goal_col.set(None);
    }

    /// 选中下标 gi 处"词"的区间（用于双击）：词 = 字母数字/下划线/非标点非空白，
    /// 向两侧扫到边界；命中分隔符只选该字符。
    fn word_around(&self, gi: usize) -> (usize, usize) {
        let chars: Vec<char> = self.text_clone().chars().collect();
        crate::widgets::sel_core::word_around(&chars, gi, |c| {
            c.is_alphanumeric() || c == '_' || (!c.is_whitespace() && !c.is_ascii_punctuation())
        })
    }

    /// 选中光标行整段（三击）。
    fn select_line(&self) {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        let gi = self.cursor.get().min(lay.total);
        let (row, _) = lay.line_of(gi);
        let (s, e) = lay.row_span(row);
        self.anchor.set(Some(s));
        // 行尾不把换行选进来，但保留最后一个字符之后可停：end = 本行内容末。
        self.cursor.set(e.min(lay.total));
        self.goal_col.set(None);
    }

    // ── 光标移动 ─────────────────────────────────────────────────────

    /// 移动光标到 gi，同时按 shift 扩选 / 非 shift 清除选区。
    fn move_to(&self, gi: usize, shift: bool) {
        if shift && self.anchor.get().is_none() {
            self.anchor.set(Some(self.cursor.get()));
        }
        self.cursor.set(gi.min(self.char_count()));
        if !shift {
            self.anchor.set(None);
        }
        self.goal_col.set(None);
        self.follow.set(true);
    }

    fn move_left(&self, shift: bool) {
        if !shift {
            if let Some((s, _)) = self.selection() {
                self.cursor.set(s);
                self.anchor.set(None);
                self.goal_col.set(None);
                self.follow.set(true);
                return;
            }
        }
        let cur = self.cursor.get();
        if cur > 0 {
            self.move_to(cur - 1, shift);
        }
    }

    fn move_right(&self, shift: bool) {
        if !shift {
            if let Some((_, e)) = self.selection() {
                self.cursor.set(e);
                self.anchor.set(None);
                self.goal_col.set(None);
                self.follow.set(true);
                return;
            }
        }
        let cur = self.cursor.get();
        if cur < self.char_count() {
            self.move_to(cur + 1, shift);
        }
    }

    fn move_home(&self, shift: bool) {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        if shift && self.anchor.get().is_none() {
            self.anchor.set(Some(self.cursor.get()));
        }
        let gi = self.cursor.get().min(lay.total);
        let (row, _) = lay.line_of(gi);
        self.cursor.set(lay.row_span(row).0.min(lay.total));
        if !shift {
            self.anchor.set(None);
        }
        self.goal_col.set(None);
        self.follow.set(true);
    }

    fn move_end(&self, shift: bool) {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        if shift && self.anchor.get().is_none() {
            self.anchor.set(Some(self.cursor.get()));
        }
        let gi = self.cursor.get().min(lay.total);
        let (row, _) = lay.line_of(gi);
        self.cursor.set(lay.row_span(row).1.min(lay.total));
        if !shift {
            self.anchor.set(None);
        }
        self.goal_col.set(None);
        self.follow.set(true);
    }

    fn move_vertical(&self, down: bool, shift: bool) {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        if shift && self.anchor.get().is_none() {
            self.anchor.set(Some(self.cursor.get()));
        }
        let gi = self.cursor.get().min(lay.total);
        let (row, col) = lay.line_of(gi);
        // 目标列：第一次上下移动记住当前列，后续保持一致。
        let goal = match self.goal_col.get() {
            Some(g) => g,
            None => {
                self.goal_col.set(Some(col));
                col
            }
        };
        let target = if down {
            (row + 1).min(lay.rows.len().saturating_sub(1))
        } else {
            row.saturating_sub(1)
        };
        let end_col = lay.row_len(target).min(goal);
        let new_gi = lay.row_span(target).0 + end_col;
        self.cursor.set(new_gi.min(self.char_count()));
        if !shift {
            self.anchor.set(None);
        }
        self.follow.set(true);
    }

    // ── 撤销 / 重做 ─────────────────────────────────────────────────

    fn undo(&self) {
        if let Some((text, cursor)) = self.undo_stack.borrow_mut().pop() {
            let cur = self.snapshot();
            self.redo_stack.borrow_mut().push(cur);
            self.text.set(text);
            self.cursor.set(cursor);
            self.anchor.set(None);
            self.goal_col.set(None);
            self.follow.set(true);
        }
    }

    fn redo(&self) {
        if let Some((text, cursor)) = self.redo_stack.borrow_mut().pop() {
            let cur = self.snapshot();
            self.undo_stack.borrow_mut().push(cur);
            self.text.set(text);
            self.cursor.set(cursor);
            self.anchor.set(None);
            self.goal_col.set(None);
            self.follow.set(true);
        }
    }

    // ── 视口滚动（供 paint 调用，基于最新布局）────────────────────

    /// 若光标越界，把视口滚到让光标可见。
    fn ensure_visible(&self, b: Rect) {
        let (vw, vh) = Self::view_size(b);
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        let (row, col) = lay.line_of(self.cursor.get());
        let r = &lay.rows[row];
        let cx = r.x.get(col).copied().unwrap_or(0.0);
        let cy = row as f32 * lay.line_h;

        let mut sx = self.scroll_x.get();
        if cx - sx < 0.0 {
            sx = cx;
        } else if cx + 20.0 - sx > vw {
            sx = (cx + 20.0 - vw).max(0.0);
        }
        self.scroll_x.set(sx.max(0.0));

        let content_h = lay.rows.len() as f32 * lay.line_h;
        let max_sy = (content_h - vh).max(0.0);
        let mut sy = self.scroll_y.get();
        if cy - sy < 0.0 {
            sy = cy;
        } else if cy + lay.line_h - sy > vh {
            sy = (cy + lay.line_h - vh).max(0.0);
        }
        self.scroll_y.set(sy.clamp(0.0, max_sy));
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Widget
// ───────────────────────────────────────────────────────────────────────────

impl Widget for SyntaxInput {
    fn measure(&self, avail: Size, style: &Style, text: &mut dyn TextEngine) -> Size {
        let font = style.font_size.max(1.0);
        let line_h = style.line_height.unwrap_or(1.4) * font;
        let ts = TextStyle::of(style);
        let palette = HighlightColors::from_theme().as_palette();
        self.rebuild(font, line_h, text.scale(), &ts, &palette, |p: &str| {
            text.measure(p, &ts, None).w.max(0) as f32
        });
        let lay = self.layout.borrow();
        let rows = lay.rows.len() as f32;
        let intrinsic = (rows * lay.line_h) as i32 + 2 * PAD;
        let min = lay.line_h as i32 + 2 * PAD;
        // 视口模型：父级给了真实高度就填满分配区（不随内容缩——否则单行短内容时控件
        // 只是一小条，点下方空白落不到控件上、无法聚焦）；无约束（含 windui 哨兵值
        // i32::MAX/4）时回退固有高度。哨兵判定统一走 layout_hint::viewport_height。
        let h = crate::widgets::layout_hint::viewport_height(avail.h, intrinsic, min);
        Size::new(avail.w, h)
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        style: &Style,
    ) {
        let colors = HighlightColors::from_theme();
        let font = style.font_size.max(1.0);
        let line_h = style.line_height.unwrap_or(1.4) * font;
        let ts = TextStyle::of(style);
        // 每帧用真实字号/画布测量重建布局 → 命中/绘制同源，且文本改动即刷新。
        // 未变则 rebuild 内部直接命中缓存跳过（DPI 取自 canvas，与 measure 同源）。
        let palette = colors.as_palette();
        self.rebuild(
            font,
            line_h,
            canvas.dpi_scale(),
            &ts,
            &palette,
            |p: &str| canvas.measure_text(p, &ts).w.max(0) as f32,
        );

        // 背景 + 焦点描边（按整块 bounds 自绘，与命中坐标一致）。
        let bg = colors.placeholder_bg();
        canvas.fill_round_rect(
            bounds.x as f32,
            bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
            6.0,
            &Paint::fill(bg),
        );
        if focused {
            canvas.stroke_round_rect(
                bounds.x as f32,
                bounds.y as f32,
                bounds.w as f32,
                bounds.h as f32,
                6.0,
                1.5,
                &Paint::fill(colors.cursor_color),
            );
        }

        // 无焦点且有占位 → 画占位文字（无分配读 Signal，不再每帧克隆整串）。
        if self.text.with(|s| s.is_empty()) && !focused && !self.composing.get() {
            let pad = PAD as f32;
            let ty = bounds.y as f32 + (bounds.h as f32 - font) / 2.0;
            canvas.draw_text(
                &self.placeholder,
                Rect::new(
                    (bounds.x as f32 + pad) as i32,
                    ty as i32,
                    (bounds.w - 2 * PAD).max(0),
                    font as i32,
                ),
                colors.placeholder_color(),
                Align::Start,
                &ts,
            );
            return;
        }

        // 若要求跟随光标，先把视口滚到位。
        // 每次 paint 先钳制一次：外部改写 Signal（清空/回填）不会发事件给控件，
        // 悬空的 cursor/anchor 若不及时收口，下一帧的光标/选区绘制就会错位。
        let _ = self.clamp_cursor();
        if self.follow.get() && focused {
            self.ensure_visible(bounds);
            self.follow.set(false);
        }

        // ── 逐行整串着色绘制（对齐官方 TextInput 多行渲染）──
        // 视口剔除：整行落在可视区外的一律跳过、根本不画——长文本下绝大多数行
        // 不可见，逐字符测量 + draw_text 是纯浪费。
        let lay = self.layout.borrow();
        canvas.save();
        canvas.clip_rect(Rect::new(
            bounds.x + PAD,
            bounds.y + PAD,
            (bounds.w - 2 * PAD).max(0),
            (bounds.h - 2 * PAD).max(0),
        ));
        let vtop = bounds.y + PAD;
        let vbot = bounds.y + bounds.h - PAD;
        let line_h_i = lay.line_h as i32;
        let scroll_x = self.scroll_x.get();

        // 选区高亮：必须画在 clip 内（否则滚动后隐藏行的底色会越过控件边界叠到
        // 相邻 UI 上），且铺在文字**之下**（字形压在色块上保持可读）。隐藏行同样剔除。
        if focused {
            if let Some((as_, ae)) = self.selection() {
                let sel_c = colors.cursor_color.scale_alpha(0.20);
                for (row, r) in lay.rows.iter().enumerate() {
                    let ry = self.row_top(bounds, lay.line_h, row);
                    if ry + lay.line_h < vtop as f32 || ry > vbot as f32 {
                        continue;
                    }
                    let end = r.end_full();
                    // pos(gi) 把全文下标钳到本行列位置 [0..=len]。
                    let pos = |gi: usize| -> usize {
                        if gi <= r.start {
                            0
                        } else if gi >= end {
                            r.len()
                        } else {
                            gi - r.start
                        }
                    };
                    let c1 = pos(as_);
                    let c2 = pos(ae);
                    if c1 < c2 {
                        let x1 = b_left_offset() + r.x[c1] - scroll_x;
                        let x2 = b_left_offset() + r.x[c2] - scroll_x;
                        let rx = bounds.x as f32 + x1;
                        canvas.fill_rect(
                            rx,
                            ry,
                            (x2 - x1).max(1.0),
                            lay.line_h,
                            &Paint::fill(sel_c),
                        );
                    }
                }
            }
        }

        for (row, r) in lay.rows.iter().enumerate() {
            if r.text.is_empty() {
                continue; // 空行无字形
            }
            let ry = self.row_top(bounds, lay.line_h, row);
            if ry + lay.line_h < vtop as f32 || ry > vbot as f32 {
                continue;
            }
            let base_x = bounds.x as f32 + b_left_offset() - scroll_x;
            if r.runs.is_empty() {
                // 整行无 token：单次整串正文色绘制。
                canvas.draw_text(
                    &r.text,
                    Rect::new((base_x + r.x[0]) as i32, ry as i32, NO_WRAP_W, line_h_i),
                    colors.text,
                    Align::Start,
                    &ts,
                );
                continue;
            }
            // 按 token run 切段着色（对齐官方 rich.rs「run 整段测量、逐 run 绘制」
            // 的批次方式）：合并相邻同色段、空隙按正文色补齐，每段整串一次成型
            // 绘制。切段必须经 bmap 按字节切——字符列直当字节下标，行内有中文
            // 等多字节字符时会切进字符中间而 panic。
            for (c1, c2, kind) in row_segs(r) {
                let (b1, b2) = (r.bmap[c1] as usize, r.bmap[c2] as usize);
                canvas.draw_text(
                    &r.text[b1..b2],
                    Rect::new((base_x + r.x[c1]) as i32, ry as i32, NO_WRAP_W, line_h_i),
                    colors.color_for(kind),
                    Align::Start,
                    &ts,
                );
            }
        }
        canvas.restore();

        // ── 光标（框架 CaretState：方波闪烁/点击实心/失焦清除全由框架托管，
        //    并自报脏区续帧；不再自画一条常亮光标）──
        // composing 期间隐藏光标（系统合成浮层自带光标），且**不更新
        // caret_local**——IME 上屏文本会经下一次 paint 的 layout 重建反映进几何。
        if focused && !self.composing.get() {
            if lay.rows_valid() {
                let (row, col) = lay.line_of(self.cursor.get());
                let r = &lay.rows[row];
                let cx = b_left_offset() + r.x[col] - scroll_x;
                let cy = self.row_top(bounds, lay.line_h, row);
                let t = windui::theme::current();
                let opts = CaretOpts::from_theme(&t.input);
                let _ = self.caret.paint(
                    canvas,
                    (bounds.x as f32 + cx) as i32,
                    cy as i32,
                    lay.line_h as i32,
                    t.input.cursor(&t.palette),
                    &opts,
                );
                // 记录光标节点局部坐标，供输入法定位候选窗。
                self.caret_local.set(Some((
                    cx as i32,
                    (cy - bounds.y as f32) as i32,
                    lay.line_h as i32,
                )));
            } else {
                self.caret_local.set(None);
            }
        } else {
            self.caret_local.set(None);
        }
        // 失焦：选区只属于焦点控件——清掉锚点，避免屏幕上残留多个控件的高亮
        // （与 SelectText 及系统编辑框失焦即取消选区一致）；同时重置光标滑行
        // 起点与脏区记录，下次出现按"首次"处理（不滑过来）。
        if !focused {
            if self.anchor.get().is_some() {
                self.anchor.set(None);
            }
            self.caret.reset();
        }
    }

    fn on_event(&mut self, ctx: &mut EventCtx, ev: &Event) -> bool {
        let b = ctx.bounds();

        match ev {
            Event::Pointer(p) => match p.kind {
                PointerKind::Down => {
                    if !b.contains(p.pos) {
                        return false;
                    }
                    ctx.request_focus();
                    self.follow.set(true);
                    if p.button != MouseButton::Left {
                        return true;
                    }
                    match p.click_count {
                        2 => {
                            let idx = self.hit_index(b, p.pos.x as f32, p.pos.y as f32);
                            let (s, e) = self.word_around(idx);
                            self.anchor.set(Some(s));
                            self.cursor.set(e);
                            self.dragging.set(false);
                            ctx.mark_dirty();
                            return true;
                        }
                        n if n >= 3 => {
                            let idx = self.hit_index(b, p.pos.x as f32, p.pos.y as f32);
                            // 把命中点所在行置为当前行再整行选中。
                            self.cursor.set(idx);
                            self.select_line();
                            self.dragging.set(false);
                            ctx.mark_dirty();
                            return true;
                        }
                        _ => {}
                    }
                    let idx = self.hit_index(b, p.pos.x as f32, p.pos.y as f32);
                    self.cursor.set(idx);
                    self.anchor.set(Some(idx));
                    self.dragging.set(true);
                    self.goal_col.set(None);
                    ctx.capture();
                    ctx.mark_dirty();
                    true
                }
                PointerKind::Move => {
                    if self.dragging.get() {
                        // 拖出上/下边界时自动滚动。
                        self.auto_scroll_drag(b, p.pos);
                        let idx = self.hit_index(b, p.pos.x as f32, p.pos.y as f32);
                        self.cursor.set(idx);
                        ctx.mark_dirty();
                        return true;
                    }
                    false
                }
                PointerKind::Up => {
                    if self.dragging.get() {
                        self.dragging.set(false);
                        ctx.release_capture();
                        if self.anchor.get() == Some(self.cursor.get()) {
                            self.anchor.set(None);
                        }
                        ctx.mark_dirty();
                        return true;
                    }
                    false
                }
                PointerKind::Leave => {
                    self.follow.set(false);
                    false
                }
                PointerKind::Enter => {
                    self.follow.set(true);
                    false
                }
                PointerKind::Wheel(steps) => {
                    let lay = self.layout.borrow();
                    if !lay.rows_valid() {
                        return false;
                    }
                    let (_, vh) = Self::view_size(b);
                    let content_h = lay.rows.len() as f32 * lay.line_h;
                    if content_h <= vh {
                        return false; // 无纵向溢出，交外层滚动容器
                    }
                    let max_sy = (content_h - vh).max(0.0);
                    let sy = self.scroll_y.get();
                    let dy = -(steps as f32) * lay.line_h;
                    let at_boundary = (dy < 0.0 && sy <= 0.0) || (dy > 0.0 && sy >= max_sy);
                    if at_boundary {
                        return false;
                    }
                    self.scroll_y.set((sy + dy).clamp(0.0, max_sy));
                    self.follow.set(false);
                    ctx.mark_dirty();
                    true
                }
            },
            Event::Key(k) if k.pressed => self.on_key(ctx, *k),
            _ => false,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn cursor(&self) -> CursorShape {
        CursorShape::Text
    }

    fn ime_caret(&self) -> Option<(i32, i32, i32)> {
        self.caret_local.get()
    }

    fn set_composing(&mut self, composing: bool) {
        self.composing.set(composing);
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let c = self.cursor.get();
        Some(self.selection().unwrap_or((c, c)))
    }

    fn ime_text(&self) -> Option<String> {
        Some(self.text_clone())
    }

    fn reset_interaction(&mut self) {
        self.anchor.set(None);
        let c = self.cursor.get().min(self.char_count());
        self.cursor.set(c);
        self.dragging.set(false);
        self.goal_col.set(None);
        self.follow.set(true);
        // 失焦时一并重置光标（闪烁相位/滑行起点/脏区记录），下次出现按"首次"处理。
        self.caret.reset();
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl SyntaxInput {
    /// 键盘处理（win32 语义）。
    fn on_key(&self, ctx: &mut EventCtx, k: KeyEvent) -> bool {
        // IME 合成期间：可打印字符是上屏文本本身（WM_CHAR 在合成结束时送达，
        // 与 set_composing(false) 的先后不保证），必须放行；但方向键/Home/End 等
        // 导航键若在合成中移动光标/选区，会把系统合成浮层的光标带歪。
        // （backspace/delete 在 win32 上走默认窗口过程即可正确删选区，这里不接管。）
        if self.composing.get() {
            return matches!(k.key, Key::Char(_));
        }
        // 任何编辑/导航键都应把视口拉回光标。
        self.follow.set(true);

        // 文档已改则本帧末由 repaint 重建布局，无需在此计算几何。
        match k.key {
            // 可打印字符（含空格经 WM_CHAR 送达）。回车在 win32 是 Key::Enter 而非 Char。
            Key::Char(c) if !k.ctrl => {
                if c == '\n' || c == '\r' {
                    self.insert_newline();
                } else if !c.is_control() {
                    self.type_char(c);
                }
                ctx.mark_dirty();
                true
            }
            Key::Enter => {
                self.insert_newline();
                ctx.mark_dirty();
                true
            }
            Key::Backspace => {
                self.backspace();
                ctx.mark_dirty();
                true
            }
            Key::Delete => {
                self.delete_forward();
                ctx.mark_dirty();
                true
            }
            Key::Left => {
                self.move_left(k.shift);
                ctx.mark_dirty();
                true
            }
            Key::Right => {
                self.move_right(k.shift);
                ctx.mark_dirty();
                true
            }
            Key::Up => {
                self.move_vertical(false, k.shift);
                ctx.mark_dirty();
                true
            }
            Key::Down => {
                self.move_vertical(true, k.shift);
                ctx.mark_dirty();
                true
            }
            Key::Home => {
                self.move_home(k.shift);
                ctx.mark_dirty();
                true
            }
            Key::End => {
                self.move_end(k.shift);
                ctx.mark_dirty();
                true
            }
            // Ctrl 组合：win32 以虚拟键码经 Key::Other 送达（见 text input 参考实现）。
            Key::Other(0x41) if k.ctrl => {
                self.select_all();
                ctx.mark_dirty();
                true
            }
            Key::Other(0x43) if k.ctrl => {
                if let Some(sel) = self.selected_str() {
                    ctx.clipboard_set(&sel);
                }
                true
            }
            Key::Other(0x58) if k.ctrl => {
                if let Some(sel) = self.selected_str() {
                    ctx.clipboard_set(&sel);
                    self.delete_selection();
                    ctx.mark_dirty();
                }
                true
            }
            Key::Other(0x56) if k.ctrl => {
                if let Some(s) = ctx.clipboard_get() {
                    self.paste(&s);
                    ctx.mark_dirty();
                }
                true
            }
            Key::Other(0x5A) if k.ctrl => {
                self.undo();
                ctx.mark_dirty();
                true
            }
            Key::Other(0x59) if k.ctrl => {
                self.redo();
                ctx.mark_dirty();
                true
            }
            // Tab：代码编辑器惯例是插入空格（2 个），而不是把焦点挪走——
            // SQL/shell 脚本里 Tab 是缩进不是导航。
            Key::Tab => {
                let (s0, e0, after) = match self.selection() {
                    Some((s0, e0)) => (s0, e0, s0 + 2),
                    None => {
                        let cur = self.cursor.get();
                        (cur, cur, cur + 2)
                    }
                };
                self.push_undo();
                self.apply_edit(s0, e0, "  ", after);
                ctx.mark_dirty();
                true
            }
            // 其余键：不消费，交宿主。
            _ => false,
        }
    }

    fn selected_str(&self) -> Option<String> {
        let sel = self.selection()?;
        self.text
            .with(|t| crate::widgets::sel_core::selected_str(t, Some(sel)))
    }

    /// 拖选时指针越过上下边界，按越界距离滚动（每次至多一行）。
    fn auto_scroll_drag(&self, b: Rect, pos: Point) {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        let (_, vh) = Self::view_size(b);
        let content_h = lay.rows.len() as f32 * lay.line_h;
        let max_sy = (content_h - vh).max(0.0);
        if max_sy <= 0.0 {
            return;
        }
        let py = pos.y as f32;
        let top = b.y as f32 + PAD as f32;
        let bot = (b.y + b.h - PAD) as f32;
        let sy = self.scroll_y.get();
        if py < top && sy > 0.0 {
            self.scroll_y.set((sy - lay.line_h).max(0.0));
        } else if py > bot && sy < max_sy {
            self.scroll_y.set((sy + lay.line_h).min(max_sy));
        }
    }
}

impl HighlightColors {
    fn placeholder_color(&self) -> Color {
        // 与其它控件占位一致：次要文字色。
        self.comment
    }
    fn placeholder_bg(&self) -> Color {
        // 编辑器底色：取主题 surface 语义色。
        let t = windui::theme::current();
        t.palette.surface
    }
}
