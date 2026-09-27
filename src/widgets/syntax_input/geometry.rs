//! 语法高亮控件的内部几何模型与测量辅助（从 `mod.rs` 拆出）。
//!
//! [`Row`]/[`Layout`]：行几何与布局缓存；[`MeasureKey`]：逐行宽度缓存的度量
//! 指纹；`byte_at`/`row_metrics`/`row_segs` 等自由函数：字节换算、前缀测宽、
//! token 绘制段切分。

use std::collections::HashMap;

use windui::geometry::Color;
use windui::text::TextStyle;

use super::lex::{LexerKind, TokenKind};

/// 排版度量指纹（逐行宽度缓存的有效性键）。
///
/// 字形 advance 只由 (字族, 字号, 字重, 斜体, DPI) 决定；行高与颜色不影响测宽。
/// `TextStyle` 里的字族是对 style 的借用，缓存要跨帧存活，故落成 String。
#[derive(Clone, Debug, PartialEq)]
pub struct MeasureKey {
    family: Option<String>,
    size: f32,
    weight: u16,
    italic: bool,
    scale: f32,
}

impl MeasureKey {
    pub fn of(ts: &TextStyle<'_>, scale: f32) -> Self {
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
pub const PAD: i32 = 4;

/// 一行的几何：`x` 为 **字符数 + 1** 项——前 `len` 项是第 0..len 个字符**左缘**
/// 相对行首的偏移，末项是行尾右缘（最后一个插入点）。命中/光标/选区共用这张
/// 表（对齐官方 TextInput VisLine.x 的 len+1 语义）。
#[derive(Debug, Default)]
pub struct Row {
    /// 本行第一个字符在全文（含 `\n`）中的字符下标。
    pub start: usize,
    pub x: Vec<f32>,
    /// 本行可见字符（不含 `\n`）。整行一次成型绘制用（对齐官方 TextInput 的
    /// `chars[ln.start..ln.end]` 逐行 `draw_text`，避免逐字符排版+入批）。
    pub text: String,
    /// 字符列 → 行内字节偏移（**len+1** 项，与 x 一一对应，末项 = 行字节长）。
    /// 按 token run 切段绘制必须按字节切 `text`：直接拿字符列当字节下标，
    /// 一旦行内有中文等多字节字符就会切进字符中间而 panic。
    pub bmap: Vec<u32>,
    /// 本行内每个 token run（token 类型 + 起始列 + 结束列，列相对行首）：
    /// 行级语法色缓存，paint 按 run 整段着色，不逐字符查表/排版。
    pub runs: Vec<(TokenKind, usize, usize)>,
}

impl Row {
    /// 本行最后一个字符之后的全文下标（= start + 字符数）。前缀 x 表下
    /// x 含行尾右缘共 len+1 项，字符数 = x.len()-1。
    pub fn end_full(&self) -> usize {
        self.start + self.x.len().saturating_sub(1)
    }
}

impl Row {
    pub fn len(&self) -> usize {
        self.x.len().saturating_sub(1)
    }
}

/// 最近一次 paint/measure 建立的布局缓存。
pub struct Layout {
    pub rows: Vec<Row>,
    /// 行高（逻辑 px）。
    pub line_h: f32,
    /// 字号（逻辑 px）。
    pub font: f32,
    /// 文本总字符数（含换行符）。
    pub total: usize,
    /// 建立本布局时的文本（缓存键：文本变化即失效）。
    pub text: String,
    /// 建立本布局时的 DPI 缩放（跨分辨率拖动窗口后字号取整会变，需重建）。
    pub scale: f32,
    /// 布局是否可用。`Layout::empty()` 初始为 false：事件早于首次 paint
    /// 到达时不应用空布局做命中/几何换算（会把光标跳到开头）。
    pub valid: bool,
    /// 建立本布局时的词法器（缓存键：换词法器颜色语义变，需重建 run 缓存）。
    pub lexer: LexerKind,
    /// 建立本布局时的行内色板（主题色）。缓存键：运行期换主题后需重建。
    pub palette: Vec<Color>,
    /// 逐行前缀宽度缓存（键 = 逻辑行内容）。前缀整串测量是 O(行长²) 字符工作，
    /// 长文档每次编辑全量重测是主要卡顿源；逐键编辑只改一行，其余行直接命中。
    pub widths: HashMap<String, Vec<f32>>,
    /// 宽度缓存建立时的度量指纹：字族/字号/字重/斜体/DPI 任一变化即整体作废。
    pub wkey: Option<MeasureKey>,
}

impl Layout {
    pub fn empty() -> Self {
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

    pub fn rows_valid(&self) -> bool {
        self.valid && !self.rows.is_empty()
    }

    /// 全文字符下标 → (所在行, 行内列)。列按该行长度钳制，保证 `x` 索引安全
    /// （游标可能停在文末空行 / 行尾新行符处）。
    ///
    /// 行首下标严格递增，二分定位（原为 O(行数) 全扫，长文档每帧多处调用）。
    pub fn line_of(&self, gi: usize) -> (usize, usize) {
        let gi = gi.min(self.total);
        let row = self
            .rows
            .partition_point(|r| r.start <= gi)
            .saturating_sub(1);
        let col = gi
            .saturating_sub(self.rows[row].start)
            .min(self.rows[row].len());
        (row, col)
    }

    /// 某行的内容字符区间在全文中的下标端点 [start, end)。
    /// `end` 是该行最后一字符之后、即行尾插入点（无换行时亦为文末）。
    pub fn row_span(&self, row: usize) -> (usize, usize) {
        // 只接受在界内的 row；越界回落到（空）末行而非 panicking 的 last().unwrap()。
        let n = self.rows.len();
        match self.rows.get(row.min(n.saturating_sub(1))) {
            Some(r) => (r.start, r.start + r.len()),
            None => (self.total, self.total),
        }
    }

    pub fn row_len(&self, row: usize) -> usize {
        self.rows.get(row).map(Row::len).unwrap_or(0)
    }
}
// 自由辅助：byte offset of char index。
/// 一行的 (前缀 x 表, 字节偏移表)。宽度优先命中逐行缓存；未命中才逐前缀
/// 整串测量（O(行长²) 字符工作量），测完回填缓存。
///
/// 前缀整串测量（而非逐字符独立测量再累加）：绘制是整串一次 DWrite 排版
/// （真实 advance），逐字符测量会把每字符的取整误差随列数线性放大，行尾右缘
/// 系统性偏右，光标/选区漂到文字右缘之外。与绘制同源后严格对齐。
pub fn row_metrics(
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
                x.push(if w.is_finite() {
                    w
                } else {
                    font * acc.chars().count() as f32
                });
            }
            widths.insert(line.to_owned(), x.clone());
            x
        }
    };
    (x, bmap)
}

/// 把一行的 token run 合并成绘制段（字符列区间，空隙补 Plain、相邻同色合并），
/// 与旧 paint 内联逻辑一致；提成自由函数让"切出来的段字节安全"可被单测覆盖。
pub fn row_segs(r: &Row) -> Vec<(usize, usize, TokenKind)> {
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
pub fn b_left_offset() -> f32 {
    PAD as f32
}

/// 逐行绘制文字时喂给排版引擎的"不换行宽"：显示行已是逻辑行，给个超宽盒子
/// 防止引擎二次折行（对齐官方 TextInput 多行绘制约定 `NO_WRAP_W = 100_000`）。
pub const NO_WRAP_W: i32 = 100_000;
