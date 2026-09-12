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
use std::collections::HashMap;

use windui::core::{EventCtx, Widget};
use windui::event::{CursorShape, Event, Key, KeyEvent, MouseButton, PointerKind};
use windui::geometry::{Color, Point, Rect, Size};
use windui::render::{Canvas, Paint};
use windui::signal::Signal;
use windui::spec::Align;
use windui::style::Style;
use windui::text::{TextEngine, TextStyle};
use windui::ui::caret::{CaretOpts, CaretState};

/// 词法分析器：将文本切分为带颜色类型的 token 列表。
#[derive(Debug, Clone)]
pub struct LexToken<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    String,
    Number,
    Comment,
    Operator,
    Flag,
    Command,
    Variable,
    Plain,
}

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

/// SQL 词法分析 → LexToken 转换。
pub fn lex_sql(text: &str) -> Vec<LexToken<'_>> {
    sql_lexer::tokenize(text)
        .into_iter()
        .map(|t| LexToken {
            kind: match t.kind {
                sql_lexer::SqlTokenKind::Keyword => TokenKind::Keyword,
                sql_lexer::SqlTokenKind::String => TokenKind::String,
                sql_lexer::SqlTokenKind::Number => TokenKind::Number,
                sql_lexer::SqlTokenKind::Comment => TokenKind::Comment,
                sql_lexer::SqlTokenKind::Operator => TokenKind::Operator,
                sql_lexer::SqlTokenKind::Identifier
                | sql_lexer::SqlTokenKind::Paren
                | sql_lexer::SqlTokenKind::Semicolon => TokenKind::Plain,
                sql_lexer::SqlTokenKind::Whitespace
                | sql_lexer::SqlTokenKind::Newline
                | sql_lexer::SqlTokenKind::Unknown => TokenKind::Plain,
            },
            text: t.text,
        })
        .collect()
}

/// Shell 词法分析 → LexToken 转换。
pub fn lex_shell(text: &str) -> Vec<LexToken<'_>> {
    shell_lexer::tokenize(text)
        .into_iter()
        .map(|t| LexToken {
            kind: match t.kind {
                shell_lexer::ShellTokenKind::Command => TokenKind::Command,
                shell_lexer::ShellTokenKind::Flag => TokenKind::Flag,
                shell_lexer::ShellTokenKind::String => TokenKind::String,
                shell_lexer::ShellTokenKind::Variable => TokenKind::Variable,
                shell_lexer::ShellTokenKind::Operator => TokenKind::Operator,
                shell_lexer::ShellTokenKind::Comment => TokenKind::Comment,
                shell_lexer::ShellTokenKind::Argument => TokenKind::Plain,
                shell_lexer::ShellTokenKind::Whitespace
                | shell_lexer::ShellTokenKind::Newline => TokenKind::Plain,
            },
            text: t.text,
        })
        .collect()
}

/// 词法分析器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerKind {
    Sql,
    Shell,
}

/// 逐行宽度缓存上限（条数）。超过即整体清空：长会话下旧行版本会随编辑不断
/// 累积，按条数兜底防缓存无限膨胀（单条 ≈ 行字节 + 4×字符数 字节）。
const WIDTH_CACHE_MAX_ENTRIES: usize = 8_192;

/// 撤销栈上限：条数 + 快照总字节。快照是全文克隆，只限条数时"反复粘贴
/// 大文本"可堆到上百 MB，按字节兜底防内存膨胀。
const UNDO_MAX_ENTRIES: usize = 200;
const UNDO_MAX_BYTES: usize = 4 * 1024 * 1024;

/// 排版度量指纹（逐行宽度缓存的有效性键）。
///
/// 字形 advance 只由 (字族, 字号, 字重, 斜体, DPI) 决定；行高与颜色不影响测宽。
/// `TextStyle` 里的字族是对 style 的借用，缓存要跨帧存活，故落成 String。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeasureKey {
    family: Option<String>,
    size: f32,
    weight: u16,
    italic: bool,
    scale: f32,
}

impl MeasureKey {
    pub(crate) fn of(ts: &TextStyle<'_>, scale: f32) -> Self {
        Self {
            family: ts.family.map(str::to_owned),
            size: ts.size,
            weight: ts.weight,
            italic: ts.italic,
            scale,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// 内部几何模型
// ───────────────────────────────────────────────────────────────────────────

/// 控件内容区四边留白（逻辑 px）。绘制与命中必须用同一份值，否则点、画错位。
const PAD: i32 = 4;

/// 一行的几何：`x` 为 **字符数 + 1** 项——前 `len` 项是第 0..len 个字符**左缘**
/// 相对行首的偏移，末项是行尾右缘（最后一个插入点）。命中/光标/选区共用这张
/// 表（对齐官方 TextInput VisLine.x 的 len+1 语义）。
#[derive(Debug, Default)]
struct Row {
    /// 本行第一个字符在全文（含 `\n`）中的字符下标。
    start: usize,
    x: Vec<f32>,
    /// 本行可见字符（不含 `\n`）。整行一次成型绘制用（对齐官方 TextInput 的
    /// `chars[ln.start..ln.end]` 逐行 `draw_text`，避免逐字符排版+入批）。
    text: String,
    /// 字符列 → 行内字节偏移（**len+1** 项，与 x 一一对应，末项 = 行字节长）。
    /// 按 token run 切段绘制必须按字节切 `text`：直接拿字符列当字节下标，
    /// 一旦行内有中文等多字节字符就会切进字符中间而 panic。
    bmap: Vec<u32>,
    /// 本行内每个 token run（token 类型 + 起始列 + 结束列，列相对行首）：
    /// 行级语法色缓存，paint 按 run 整段着色，不逐字符查表/排版。
    runs: Vec<(TokenKind, usize, usize)>,
}

impl Row {
    /// 本行最后一个字符之后的全文下标（= start + 字符数）。前缀 x 表下
    /// x 含行尾右缘共 len+1 项，字符数 = x.len()-1。
    fn end_full(&self) -> usize {
        self.start + self.x.len().saturating_sub(1)
    }
}

impl Row {
    fn len(&self) -> usize {
        self.x.len().saturating_sub(1)
    }
}

/// 最近一次 paint/measure 建立的布局缓存。
struct Layout {
    rows: Vec<Row>,
    /// 行高（逻辑 px）。
    line_h: f32,
    /// 字号（逻辑 px）。
    font: f32,
    /// 文本总字符数（含换行符）。
    total: usize,
    /// 建立本布局时的文本（缓存键：文本变化即失效）。
    text: String,
    /// 建立本布局时的 DPI 缩放（跨分辨率拖动窗口后字号取整会变，需重建）。
    scale: f32,
    /// 布局是否可用。`Layout::empty()` 初始为 false：事件早于首次 paint
    /// 到达时不应用空布局做命中/几何换算（会把光标跳到开头）。
    valid: bool,
    /// 建立本布局时的词法器（缓存键：换词法器颜色语义变，需重建 run 缓存）。
    lexer: LexerKind,
    /// 建立本布局时的行内色板（主题色）。缓存键：运行期换主题后需重建。
    palette: Vec<Color>,
    /// 逐行前缀宽度缓存（键 = 逻辑行内容）。前缀整串测量是 O(行长²) 字符工作，
    /// 长文档每次编辑全量重测是主要卡顿源；逐键编辑只改一行，其余行直接命中。
    widths: HashMap<String, Vec<f32>>,
    /// 宽度缓存建立时的度量指纹：字族/字号/字重/斜体/DPI 任一变化即整体作废。
    wkey: Option<MeasureKey>,
}

impl Layout {
    fn empty() -> Self {
        Layout {
            rows: vec![Row {
                start: 0,
                x: vec![0.0],
                text: String::new(),
                bmap: vec![0],
                runs: Vec::new(),
            }],
            line_h: 18.2,
            font: 14.0,
            total: 0,
            text: String::new(),
            scale: 1.0,
            valid: false,
            lexer: LexerKind::Sql,
            palette: Vec::new(),
            widths: HashMap::new(),
            wkey: None,
        }
    }

    fn rows_valid(&self) -> bool {
        self.valid && !self.rows.is_empty()
    }

    /// 全文字符下标 → (所在行, 行内列)。列按该行长度钳制，保证 `x` 索引安全
    /// （游标可能停在文末空行 / 行尾新行符处）。
    ///
    /// 行首下标严格递增，二分定位（原为 O(行数) 全扫，长文档每帧多处调用）。
    fn line_of(&self, gi: usize) -> (usize, usize) {
        let gi = gi.min(self.total);
        let row = self.rows.partition_point(|r| r.start <= gi).saturating_sub(1);
        let col = gi.saturating_sub(self.rows[row].start).min(self.rows[row].len());
        (row, col)
    }

    /// 某行的内容字符区间在全文中的下标端点 [start, end)。
    /// `end` 是该行最后一字符之后、即行尾插入点（无换行时亦为文末）。
    fn row_span(&self, row: usize) -> (usize, usize) {
        // 只接受在界内的 row；越界回落到（空）末行而非 panicking 的 last().unwrap()。
        let n = self.rows.len();
        match self.rows.get(row.min(n.saturating_sub(1))) {
            Some(r) => (r.start, r.start + r.len()),
            None => (self.total, self.total),
        }
    }

    fn row_len(&self, row: usize) -> usize {
        self.rows.get(row).map(Row::len).unwrap_or(0)
    }
}

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
        let c = self.cursor.get().min(total);
        let a = self.anchor.get()?;
        let a = a.min(total);
        if a == c {
            None
        } else if a < c {
            Some((a, c))
        } else {
            Some((c, a))
        }
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
        let line_h = lay.line_h.max(1.0);
        let row_floor = (cy / line_h).floor();
        let row = if row_floor < 0.0 {
            0
        } else {
            (row_floor as usize).min(lay.rows.len().saturating_sub(1))
        };
        let r = &lay.rows[row];
        let len = r.len();
        // x 单调不减：二分定位 cx 所在列（原为 O(len) 全扫，长行逐帧拖选会卡）。
        // 语义与逐格比较一致：取使"左缘 ≤ cx"成立的最大列；cx 在某字符中部时
        // 落在左半偏左、右半偏右。
        let col = if cx <= r.x[0] {
            0
        } else if cx >= *r.x.last().unwrap_or(&0.0) {
            len
        } else {
            let mut lo = 0usize;
            let mut hi = len;
            while lo + 1 < hi {
                let mid = lo + (hi - lo) / 2;
                if r.x[mid] <= cx {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            // x 为 f32：当 cx 恰好落在某字符中部（与 lo/hi 中点同距）时，
            // 按半开区间 [x[lo], x[hi]) 归属——中点之前算 lo、之后算 hi。
            if cx <= (r.x[lo] + r.x[hi]) / 2.0 {
                lo
            } else {
                hi
            }
        };
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

    /// 选中下标 gi 处"词"的区间（用于双击）。简单实现：向两侧扫到空白/标点界。
    fn word_around(&self, gi: usize) -> (usize, usize) {
        let chars: Vec<char> = self.text_clone().chars().collect();
        let total = chars.len();
        if total == 0 {
            return (0, 0);
        }
        let is_w = |c: &char| c.is_alphanumeric() || *c == '_' || !c.is_whitespace() && !c.is_ascii_punctuation();
        // 双击命中的字符若本身是分隔符，则不扩展。
        let mut gi = gi.min(total.saturating_sub(1));
        while gi > 0 && chars[gi] == '\n' {
            gi -= 1;
        }
        if total == 0 || !is_w(&chars[gi]) {
            // 命中空白/分隔符：选中该字符本身即可（避免跳到别处）。
            let e = (gi + 1).min(total);
            return (gi, e);
        }
        let mut s = gi;
        while s > 0 && is_w(&chars[s - 1]) {
            s -= 1;
        }
        let mut e = gi + 1;
        while e < total && is_w(&chars[e]) {
            e += 1;
        }
        (s, e)
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

// 自由辅助：byte offset of char index。
fn byte_at(s: &str, gi: usize) -> usize {
    s.char_indices()
        .nth(gi)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// 一行的 (前缀 x 表, 字节偏移表)。宽度优先命中逐行缓存；未命中才逐前缀
/// 整串测量（O(行长²) 字符工作量），测完回填缓存。
///
/// 前缀整串测量（而非逐字符独立测量再累加）：绘制是整串一次 DWrite 排版
/// （真实 advance），逐字符测量会把每字符的取整误差随列数线性放大，行尾右缘
/// 系统性偏右，光标/选区漂到文字右缘之外。与绘制同源后严格对齐。
fn row_metrics(
    line: &str,
    font: f32,
    measure: &mut impl FnMut(&str) -> f32,
    widths: &mut HashMap<String, Vec<f32>>,
) -> (Vec<f32>, Vec<u32>) {
    let lc = line.chars().count();
    // 字符列 → 字节偏移表（与绘制切片共用，len+1 项）。
    let mut bmap: Vec<u32> = line.char_indices().map(|(b, _)| b as u32).collect();
    bmap.push(line.len() as u32);
    if lc == 0 {
        return (vec![0.0], bmap);
    }
    let x = match widths.get(line) {
        Some(cached) => cached.clone(),
        None => {
            let mut x = Vec::with_capacity(lc + 1);
            x.push(0.0);
            let mut acc = String::with_capacity(line.len());
            for ch in line.chars() {
                acc.push(ch);
                let w = measure(&acc).max(0.0);
                x.push(if w.is_finite() { w } else { font * acc.chars().count() as f32 });
            }
            widths.insert(line.to_owned(), x.clone());
            x
        }
    };
    (x, bmap)
}

/// 把一行的 token run 合并成绘制段（字符列区间，空隙补 Plain、相邻同色合并），
/// 与旧 paint 内联逻辑一致；提成自由函数让"切出来的段字节安全"可被单测覆盖。
fn row_segs(r: &Row) -> Vec<(usize, usize, TokenKind)> {
    let mut segs: Vec<(usize, usize, TokenKind)> = Vec::new();
    let mut pos = 0usize;
    for (kind, c1, c2) in &r.runs {
        if *c2 <= *c1 || *c2 > r.len() {
            continue;
        }
        if *c1 > pos {
            match segs.last_mut() {
                Some(s) if s.2 == TokenKind::Plain && s.1 == pos => s.1 = *c1,
                _ => segs.push((pos, *c1, TokenKind::Plain)),
            }
        }
        match segs.last_mut() {
            Some(s) if s.2 == *kind && s.1 == *c1 => s.1 = *c2,
            _ => segs.push((*c1, *c2, *kind)),
        }
        pos = (*c2).min(r.len());
    }
    if pos < r.len() {
        match segs.last_mut() {
            Some(s) if s.2 == TokenKind::Plain && s.1 == pos => s.1 = r.len(),
            _ => segs.push((pos, r.len(), TokenKind::Plain)),
        }
    }
    segs
}

// 行内 x 偏移的基点（相对 bounds 左缘的 PAD）。
fn b_left_offset() -> f32 {
    PAD as f32
}

/// 逐行绘制文字时喂给排版引擎的"不换行宽"：显示行已是逻辑行，给个超宽盒子
/// 防止引擎二次折行（对齐官方 TextInput 多行绘制约定 `NO_WRAP_W = 100_000`）。
const NO_WRAP_W: i32 = 100_000;

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
        let h = if avail.h > 0 {
            intrinsic.clamp(min, avail.h)
        } else {
            intrinsic.max(min)
        };
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
        self.rebuild(font, line_h, canvas.dpi_scale(), &ts, &palette, |p: &str| {
            canvas.measure_text(p, &ts).w.max(0) as f32
        });

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
                        canvas.fill_rect(rx, ry, (x2 - x1).max(1.0), lay.line_h, &Paint::fill(sel_c));
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
                self.caret_local.set(Some((cx as i32, (cy - bounds.y as f32) as i32, lay.line_h as i32)));
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
        let (s, e) = self.selection()?;
        self.text.with(|t| {
            let (bs, be) = (byte_at(t, s), byte_at(t, e));
            Some(t[bs..be].to_string())
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use windui::geometry::Rect;
    use windui::signal::signal;

    /// 构造一个已建立布局的控件（常量字符宽 10，便于断言 hit_index）。
    fn build(text: &str) -> SyntaxInput {
        let s = signal(text.to_string());
        let si = SyntaxInput::new(s, "", LexerKind::Sql);
        si.rebuild(
            14.0,
            19.6,
            1.0,
            &TextStyle::new(14.0),
            &HighlightColors::from_theme().as_palette(),
            // 前缀测量语义：宽 = 字符数 × 10（等价于常量字符宽 10 的累加）。
            |p| (p.chars().count() as f32) * 10.0,
        );
        si
    }

    #[test]
    fn byte_at_cjk() {
        // 你 = 3 字节；a/b/c 各 1 字节。
        assert_eq!(byte_at("你abc", 0), 0);
        assert_eq!(byte_at("你abc", 1), 3);
        assert_eq!(byte_at("你abc", 2), 4);
        assert_eq!(byte_at("你abc", 3), 5);
        // 越界钳到文末。
        assert_eq!(byte_at("你abc", 99), 6);
    }

    #[test]
    fn layout_line_of_multiline() {
        let si = build("ab\ncd\n");
        let lay = si.layout.borrow();
        assert_eq!(lay.line_of(0), (0, 0));
        assert_eq!(lay.line_of(1), (0, 1));
        assert_eq!(lay.line_of(2), (0, 2), "行尾换行符前 = 第 0 行末");
        assert_eq!(lay.line_of(3), (1, 0));
        assert_eq!(lay.line_of(4), (1, 1));
        assert_eq!(lay.line_of(6), (2, 0), "文末空行");
        // 越界下标钳制而非 panic。
        assert_eq!(lay.line_of(999), (2, 0));
    }

    #[test]
    fn layout_row_span_clamps_oob() {
        let si = build("ab\ncd\n");
        let lay = si.layout.borrow();
        // 越界 row 回落到 (total, total)，不再 last().unwrap() panic。
        assert_eq!(lay.row_span(99), (6, 6));
        assert_eq!(lay.row_span(0), (0, 2));
        assert_eq!(lay.row_len(99), 0);
    }

    #[test]
    fn hit_index_columns() {
        let si = build("hello"); // 单行，x = [0,10,20,30,40,50]
        let b = Rect::new(0, 0, 200, 60);
        let row_y = 10.0; // 落在第 0 行
        assert_eq!(si.hit_index(b, 4.0, row_y), 0, "行首");
        assert_eq!(si.hit_index(b, 19.0, row_y), 1, "第 1 字符中部偏右");
        assert_eq!(si.hit_index(b, 24.0, row_y), 2, "第 2 字符左半");
        assert_eq!(si.hit_index(b, 54.0, row_y), 5, "行尾插入点");
    }

    #[test]
    fn hit_index_invalid_layout_returns_cursor() {
        // 未 rebuild（布局未建立）：命中不 panic，返回钳制后的光标。
        let si = SyntaxInput::new(signal(String::from("abcd")), "", LexerKind::Sql);
        let b = Rect::new(0, 0, 200, 60);
        assert_eq!(si.hit_index(b, 4.0, 10.0), 4);
    }

    #[test]
    fn selection_normalizes_order() {
        let si = build("abcdef");
        // 锚在右、光标在左 → 规范化为升序。
        si.cursor.set(1);
        si.anchor.set(Some(4));
        assert_eq!(si.selection(), Some((1, 4)));
        // 重合 → 无选区。
        si.cursor.set(2);
        si.anchor.set(Some(2));
        assert_eq!(si.selection(), None);
        // 锚点越界被钳到文末。
        si.cursor.set(0);
        si.anchor.set(Some(99));
        assert_eq!(si.selection(), Some((0, 6)));
    }

    #[test]
    fn clamp_cursor_on_external_shrink() {
        let si = build("hello");
        si.cursor.set(5);
        si.anchor.set(Some(5));
        // 外部把文本改短（不经过控件事件）。
        si.text.set(String::from("hi"));
        assert!(si.clamp_cursor(), "越界光标应被修正");
        assert_eq!(si.cursor.get(), 2);
        assert_eq!(si.anchor.get(), None, "与光标重合的锚点应清除");
        // 再次调用幂等。
        assert!(!si.clamp_cursor());
    }

    #[test]
    fn type_char_cjk_keeps_char_cursor() {
        let si = build("ab");
        si.cursor.set(1);
        si.type_char('你');
        assert_eq!(si.text.with(|t| t.clone()), "a你b");
        assert_eq!(si.cursor.get(), 2, "光标按字符下标前进");
    }

    #[test]
    fn backspace_cjk() {
        let si = build("a你");
        si.cursor.set(2);
        si.backspace();
        assert_eq!(si.text.with(|t| t.clone()), "a");
        assert_eq!(si.cursor.get(), 1);
    }

    #[test]
    fn delete_forward_at_end_noop() {
        let si = build("ab");
        si.cursor.set(2);
        si.delete_forward(); // 已在文末，应无变化
        assert_eq!(si.text.with(|t| t.clone()), "ab");
        assert_eq!(si.cursor.get(), 2);
    }

    #[test]
    fn paste_crlf_normalized() {
        let si = build("");
        si.cursor.set(0);
        si.paste("a\r\nb\rc");
        assert_eq!(si.text.with(|t| t.clone()), "a\nb\nc", "CRLF/CR 统一为 LF");
        assert_eq!(si.cursor.get(), 5);
        // 全 CR 的纯空白剪贴内容 → 不产生空粘贴。
        let si2 = build("");
        si2.paste("\r\r");
        assert_eq!(si2.text.with(|t| t.clone()), "\n\n");
    }

    #[test]
    fn word_around() {
        let si = build("hello world");
        assert_eq!(si.word_around(7), (6, 11), "选中 world");
        assert_eq!(si.word_around(2), (0, 5), "选中 hello");
        // 空文本不 panic。
        let empty = build("");
        assert_eq!(empty.word_around(0), (0, 0));
    }

    #[test]
    fn move_vertical_invalid_layout_no_panic() {
        let si = SyntaxInput::new(signal(String::from("ab")), "", LexerKind::Sql);
        si.cursor.set(1);
        si.move_vertical(true, false); // 布局未建立，静默返回
        si.move_home(false);
        si.move_end(false);
        assert_eq!(si.cursor.get(), 1, "未建立布局时导航不改动光标");
    }

    #[test]
    fn undo_redo_roundtrip() {
        let si = build("ab");
        si.cursor.set(2);
        si.type_char('c');
        assert_eq!(si.text.with(|t| t.clone()), "abc");
        si.undo();
        assert_eq!(si.text.with(|t| t.clone()), "ab");
        si.redo();
        assert_eq!(si.text.with(|t| t.clone()), "abc");
    }

    #[test]
    fn select_line_triple_click() {
        let si = build("ab\ncd\n");
        si.cursor.set(3); // 第 1 行（"cd"）行首
        si.select_line();
        assert_eq!(si.selection(), Some((3, 5)), "整行选中，不含换行");
    }

    #[test]
    fn apply_edit_replaces_multibyte_range() {
        let si = build("你abc");
        // 删除字符 [1,3) = "ab"，插入 "X"。
        si.apply_edit(1, 3, "X", 1);
        assert_eq!(si.text.with(|t| t.clone()), "你Xc");
    }

    #[test]
    fn select_all_bounds() {
        let si = build("abcd");
        si.select_all();
        assert_eq!(si.selection(), Some((0, 4)));
    }

    #[test]
    fn rebuild_cache_skips_when_unchanged() {
        let si = build("hello");
        let before: Vec<(usize, usize)> = si
            .layout
            .borrow()
            .rows
            .iter()
            .map(|r| (r.start, r.x.len()))
            .collect();
        // 同参数再次 rebuild → 命中缓存，rows 不变。
        si.rebuild(
            14.0,
            19.6,
            1.0,
            &TextStyle::new(14.0),
            &HighlightColors::from_theme().as_palette(),
            |p| (p.chars().count() as f32) * 10.0,
        );
        let after: Vec<(usize, usize)> = si
            .layout
            .borrow()
            .rows
            .iter()
            .map(|r| (r.start, r.x.len()))
            .collect();
        assert_eq!(before, after);
        // DPI 变化 → 强制重建。
        si.rebuild(14.0, 19.6, 2.0, &TextStyle::new(14.0), &HighlightColors::from_theme().as_palette(), |_| 20.0);
        let x0 = si.layout.borrow().rows[0].x[1];
        assert_eq!(x0, 20.0, "重建后按新测宽");
    }

    /// 逐行宽度缓存：文本未变的行在强制重建（如换色板）时不再逐前缀测量。
    #[test]
    fn prefix_cache_hits_on_forced_rebuild() {
        let si = build("ab\ncd");
        let mut calls = 0;
        // 换色板强制走重建路径，但两行内容未变 → 全部命中缓存，measure 零调用。
        si.rebuild(
            14.0,
            19.6,
            1.0,
            &TextStyle::new(14.0),
            &[windui::geometry::Color::rgb(1, 2, 3); 9],
            |p| {
                calls += 1;
                (p.chars().count() as f32) * 10.0
            },
        );
        assert_eq!(calls, 0, "未变的行应命中宽度缓存");
        let lay = si.layout.borrow();
        assert_eq!(lay.rows[0].x, vec![0.0, 10.0, 20.0]);
    }

    /// 回归：行内有中文时，按 token run 切段必须经 bmap 按字节切——字符列直当
    /// 字节下标会切进多字节字符中间而 panic（旧实现），且段拼接须零丢失零重复。
    #[test]
    fn runs_on_cjk_slice_safely() {
        let si = build("SELECT '中文' AS x -- 注释\nGO");
        let lay = si.layout.borrow();
        let mut painted = String::new();
        for r in &lay.rows {
            assert_eq!(r.bmap.len(), r.x.len(), "bmap 与 x 同为 len+1 项");
            for (c1, c2, _) in row_segs(r) {
                let (b1, b2) = (r.bmap[c1] as usize, r.bmap[c2] as usize);
                painted.push_str(&r.text[b1..b2]);
            }
        }
        // 各段首尾相接覆盖整行（不含换行）。
        assert_eq!(painted, "SELECT '中文' AS x -- 注释GO");
    }

    /// 单行注释 token 吞进行尾 '\n' 时，run 末列按行长钳制：注释本身仍高亮，
    /// 不再因超出行长被整段丢弃（旧实现后继行之前的注释永远不高亮）。
    #[test]
    fn line_comment_before_more_lines_is_highlighted() {
        let si = build("SELECT 1 -- x\nSELECT 2");
        let lay = si.layout.borrow();
        let runs = &lay.rows[0].runs;
        assert!(
            runs.iter().any(|(k, c1, c2)| *k == TokenKind::Comment && *c1 == 9 && *c2 == 13),
            "注释 run 应钳到行内 [9, 13)，实际 {:?}",
            runs
        );
    }

    /// 撤销栈字节上限：反复入栈大快照时按字节淘汰最旧条目，防内存膨胀。
    #[test]
    fn undo_stack_bounded_by_bytes() {
        let si = build("");
        si.text.set("x".repeat(200_000));
        for _ in 0..40 {
            si.push_undo(); // 每条快照 200KB，40 条共 8MB > 4MB 上限
        }
        let u = si.undo_stack.borrow();
        assert!(u.len() < 40, "应按字节淘汰最旧快照");
        let total: usize = u.iter().map(|(t, _)| t.len()).sum();
        assert!(total <= UNDO_MAX_BYTES, "快照总字节应不超过上限");
    }
}
