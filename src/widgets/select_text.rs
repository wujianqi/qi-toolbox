//! 只读可选文本控件（「拓展 label」）。
//!
//! windui 的 [`Element::label`] 只读不可选、`Element::text_input` 又引入了输入/占位等
//! 编辑语义，二者都无法满足"输出项要能选中并复制"。本控件补齐这一档：**只读**（不写信号、
//! 不可键入）、**可选中**（点击定位、拖拽划选、双击选词、三击选行、Shift+方向扩选、Ctrl+A）、
//! **可复制**（Ctrl+C / 右键「复制」），并把只读输出文本从"借输入框承载"的标准做法中解放出来。
//!
//! # 与 [`crate::ui::syntax_input::SyntaxInput`] 同一套纪律
//!
//! - **几何一律来自最近一帧 paint/measure 建好的布局缓存**：`measure`/`paint` 里按真实字号
//!   与内容宽重建 `Layout`（逐行逐字符左缘 x），`on_event` 读同一份缓存做命中——事件里绝不用
//!   写死的字号/行高，否则"点下去选区跑到别处"。
//! - **按词换行 + 超长词按字符折行**（无横向滚动，只有纵向滚动），长 URL / base64 / JSON 也
//!   能折进盒内。`rebuild` 用 `(文本, 行高, 缩放, 宽, 度量指纹)` 作缓存键，未变则跳过
//!   逐字符测量；另设**逐行前缀宽度缓存**（键 = 行内容），外部改写长文本时未变的行
//!   不再重测（前缀整串测量是 O(行长²)，是长输出下的主要卡顿源）。
//! - 行模型：显示行 [`VRow`] 的 `start..end` 是**全文字符下标**（含 `\n`），与文本 1:1，
//!   故选择区间直接用全文下标表示，命中/绘制/复制共用同一坐标系。
//!
//! 挂载方式与 [`SyntaxInput`] 一致：`Element::leaf().widget(SelectText::new(signal))`。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use super::sel_core;

use windui::core::{EventCtx, Widget};
use windui::event::{CursorShape, Event, Key, KeyEvent, MenuItem, MouseButton, PointerKind};
use windui::geometry::{Rect, Size};
use windui::render::{Canvas, Paint};
use windui::signal::Signal;
use windui::spec::Align;
use windui::style::Style;
use windui::text::{TextEngine, TextStyle};
use windui::ui::caret::{CaretOpts, CaretState};

use crate::lang;

use super::syntax_input::MeasureKey;

/// 内容区四边留白（逻辑 px）。绘制与命中必须用同一份值，否则选、画错位。
const PAD: i32 = 6;

/// 逐行宽度缓存上限（条数）。超过即整体清空：长会话下旧行版本会随外部改写
/// 文本不断累积，按条数兜底防缓存无限膨胀（单条 ≈ 行字节 + 4×字符数 字节）。
const WIDTH_CACHE_MAX_ENTRIES: usize = 8_192;

/// 逐行绘制文字时喂给排版引擎的"不换行宽"：显示行已按盒宽折好，给个超宽盒子
/// 防止引擎二次折行（对齐 windui TextInput 多行绘制约定 `NO_WRAP_W = 100_000`）。
const NO_WRAP_W: i32 = 100_000;

/// 一个**显示行**（按词换行后的视觉行）。`start..end` 为全文字符下标区间（1:1，不删字符），
/// `x[k]` 为第 k 个显示字符的左缘相对行首的像素偏移；`x.len() == (end - start) + 1`。
#[derive(Debug, Default)]
struct VRow {
    /// 本行第一个显示字符在全文（含 `\n`）中的字符下标。
    start: usize,
    /// 本行最后一个显示字符之后的全文下标（半开区间右端）。
    end: usize,
    /// 行内 x 坐标，**len+1** 项：前 len 项为第 0..len 个字符左缘（相对行首），
    /// 末项 = 行尾右缘（最后一个插入点）。缺末项则 hit_index 永远够不到行尾
    /// 插入点，末字符的选区区间为空——无法选中（对齐官方 TextInput VisLine.x
    /// 的 len+1 语义）。空行只有末项 0.0。
    x: Vec<f32>,
    /// 本行可见字符（不含 `\n`）。绘制时整串一次成型——逐字符 `draw_text`
    /// 时每个字符都单独走一次排版+入批，长文本下是主要开销，故缓存整行字符串。
    text: String,
}

impl VRow {
    fn len(&self) -> usize {
        self.x.len().saturating_sub(1)
    }
}

/// 最近一次 paint/measure 建立的布局缓存。
struct Layout {
    rows: Vec<VRow>,
    /// 行高（逻辑 px）。
    line_h: f32,
    /// 建立本布局时的内容宽（avail.w）；宽度变化即失效（换行依赖它）。
    width: i32,
    /// 建立本布局时的 DPI 缩放（跨分辨率拖动窗口后字号取整会变，需重建）。
    scale: f32,
    /// 建立本布局时的文本（缓存键：文本变化即失效）。
    text: String,
    /// 全文字符总数。
    total: usize,
    /// 布局是否可用。初始 false：事件早于首次 paint 时不用空布局做命中换算。
    valid: bool,
    /// 逐行前缀宽度缓存（键 = 逻辑行内容）。前缀整串测量是 O(行长²) 字符工作，
    /// 长输出每次改写全量重测是主要卡顿源；命中后未变的行不再重测。
    widths: HashMap<String, Vec<f32>>,
    /// 宽度缓存建立时的度量指纹：字族/字号/字重/斜体/DPI 任一变化即整体作废。
    wkey: Option<MeasureKey>,
}

impl Layout {
    fn empty() -> Self {
        Self {
            rows: vec![VRow {
                start: 0,
                end: 0,
                x: vec![0.0],
                text: String::new(),
            }],
            line_h: 18.2,
            width: 0,
            scale: 1.0,
            text: String::new(),
            total: 0,
            valid: false,
            widths: HashMap::new(),
            wkey: None,
        }
    }

    fn rows_valid(&self) -> bool {
        self.valid && !self.rows.is_empty()
    }
}

/// 控件配色（解析自当前主题，运行期换主题自动跟随）。
/// 底色/边框由核心层按节点 style 绘制（见模块文档），这里只留自绘用的两色。
struct Colors {
    /// 前景优先走 style（尊重调用方 `.fg_role(..)`），回退主题正文色。
    text: windui::geometry::Color,
    /// 选区高亮底色（绘制时再 `scale_alpha` 调淡）/ 焦点描边色。
    accent: windui::geometry::Color,
}

impl Colors {
    fn of(style: &Style) -> Self {
        let t = windui::theme::current();
        Self {
            text: style.resolved_fg(&t),
            accent: t.palette.accent,
        }
    }
}

/// 只读可选文本控件（见模块文档）。
pub struct SelectText {
    /// 只读文本源：只读不写。
    text: Signal<String>,

    // ── 选择态（全文字符下标）──
    /// 光标（选择焦点端），[0, 文本长度]。
    cursor: Cell<usize>,
    /// 选择锚点（另一端）；None = 无选区。
    anchor: Cell<Option<usize>>,

    // ── 几何（paint/measure 每帧按需重建）──
    layout: RefCell<Layout>,
    scroll_y: Cell<f32>,
    /// true 时下一帧 paint 把视口滚到光标处。
    follow: Cell<bool>,
    dragging: Cell<bool>,
    /// 插入光标闪烁相位/续帧（见 [`CaretState`]）：paint 自报脏区续帧驱动闪烁，
    /// 点击/移动后先实心保持再闪——与系统插入符同一套观感。
    caret: CaretState,
}

impl SelectText {
    pub fn new(text: Signal<String>) -> Self {
        let cursor = text.try_with(|s| s.chars().count()).unwrap_or(0);
        Self {
            text,
            cursor: Cell::new(cursor),
            anchor: Cell::new(None),
            layout: RefCell::new(Layout::empty()),
            scroll_y: Cell::new(0.0),
            follow: Cell::new(true),
            dragging: Cell::new(false),
            caret: CaretState::new(),
        }
    }

    // ── 文本 / 字节换算 ──────────────────────────────────────────────

    /// 死句柄容错读取：信号源可能来自会整树重建的宿主（主题切换 host_signal），
    /// 重建瞬间旧子树的句柄已失效但控件仍可能被事件/paint 摸到——读死句柄
    /// 回退空串而不是 panic（signal.rs:545 曾致窗体闪退）。
    fn try_text<R>(&self, f: impl FnOnce(&str) -> R) -> Option<R> {
        self.text.try_with(|s| f(s))
    }

    fn text_clone(&self) -> String {
        self.try_text(|s| s.to_string()).unwrap_or_default()
    }

    fn char_count(&self) -> usize {
        // 布局与当前文本一致时直接用缓存的 total（memcmp 级校验，远快于逐字符
        // 计数）——selection/clamp 每帧多处调用，长输出下 O(N) 计数是白付的。
        let lay = self.layout.borrow();
        if lay.valid && self.try_text(|s| lay.text.as_str() == s) == Some(true) {
            return lay.total;
        }
        drop(lay);
        self.try_text(|s| s.chars().count()).unwrap_or(0)
    }

    /// 规范化选区 [start, end)。无选区返回 None。
    fn selection(&self) -> Option<(usize, usize)> {
        sel_core::normalize_selection(self.cursor.get(), self.anchor.get(), self.char_count())
    }

    /// 选区文本（供复制）。无选区返回 None。
    fn selected_str(&self) -> Option<String> {
        let sel = self.selection()?;
        self.try_text(|t| sel_core::selected_str(t, Some(sel)))
            .unwrap_or_default()
    }

    /// 光标/锚点钳制到合法字符下标（文本被外部改写后可能悬空/越界）。
    ///
    /// 只有本帧**真的修正了越界值**、且修正后锚点塌缩到光标处，才清掉锚点
    /// （空选区无意义）；未越界时绝不碰 anchor——拖选起始帧 `anchor == cursor`
    /// 是正常起始态，若每帧 paint 都当"空选区"清掉，之后 Move 更新 cursor 时
    /// anchor 已丢失，整段拖选就没有高亮（表现为"只有双击才有选中效果"）。
    fn clamp(&self) -> bool {
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

    /// `scale` 为 DPI 缩放（measure 用 `TextEngine::scale()`，paint 用 `Canvas::dpi_scale()`
    /// ——同源，否则跨分辨率拖动后两帧缓存互踢）。`ts` 供宽度缓存取度量指纹
    /// （字族/字重/斜体变化时缓存作废）。
    fn rebuild(
        &self,
        line_h: f32,
        scale: f32,
        width: i32,
        ts: &TextStyle<'_>,
        mut measure: impl FnMut(&str) -> f32,
    ) {
        let mkey = MeasureKey::of(ts, scale);
        let mut lay = self.layout.borrow_mut();
        // 缓存键用无分配比较（&str 直比）：未变则跳过，**不克隆文本**。
        // measure/paint 每帧各调一次本函数，长输出下每帧两次整串 clone 是白付的。
        let text_changed = self.try_text(|s| lay.text.as_str() != s).unwrap_or(true);
        if lay.valid
            && !text_changed
            && lay.line_h == line_h
            && lay.scale == scale
            && lay.width == width
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
        let chars: Vec<char> = text.chars().collect();
        let total = chars.len();
        let max_w = (width - 2 * PAD).max(0) as f32;
        let mut rows: Vec<VRow> = Vec::new();
        // 逐逻辑行（'\n' 之间）换行；文本以 '\n' 结尾或为空时补一个空行，与输入框一致
        // （split('\n') 的切分语义与之完全相同）。每行宽度优先命中逐行缓存：
        // 前缀整串测量是 O(行长²) 字符工作，长输出全量重测是主要卡顿源。
        let mut gi = 0usize;
        for line in text.split('\n') {
            let lc = line.chars().count();
            let prefix = line_prefix(line, &chars[gi..gi + lc], &mut measure, &mut lay.widths);
            rows.extend(Self::wrap_line(&chars[gi..gi + lc], gi, &prefix, max_w));
            gi += lc + 1;
        }
        lay.rows = rows;
        lay.line_h = line_h;
        lay.scale = scale;
        lay.width = width;
        lay.text = text;
        lay.total = total;
        lay.valid = true;
    }

    /// 把逻辑行 `chars[ls..ls+lc)` 按 `max_w` 折成一个或多个显示行。
    ///
    /// `chars` 现在是**本逻辑行的切片**（不再是全文），`ls` 为该行在全文中的
    /// 起始字符下标（只用于给显示行定位 `start`）。`prefix[j]` = 行内前 j 个
    /// 字符的整串实测宽（相对段首），与绘制同源。
    /// 优先在空格处断开（空格留在上一行行尾）；超长无空格词按字符折行。
    ///
    /// 各显示行是本行的一个**严格划分**（每个字符恰好属于一行）；
    /// 行内 `x[i]` = 第 i 个字符左缘（行首恒 0）= 前缀差，与绘制/命中同源。
    fn wrap_line(chars: &[char], ls: usize, prefix: &[f32], max_w: f32) -> Vec<VRow> {
        // prefix 恰 lc+1 项（rebuild 保证 ≥1 项，不空）：prefix[j] = 前 j 个字符的
        // 整串实测宽，末项 = 行尾右缘（第 lc 个插入点）。
        let lc = prefix.len() - 1;
        let mut rows = Vec::new();
        if lc == 0 {
            rows.push(VRow {
                start: ls,
                end: ls,
                x: vec![0.0],
                text: String::new(),
            });
            return rows;
        }
        let row_for = |first: usize, brk: usize| -> VRow {
            // 行 = 逻辑行内列 [first, brk)；x 为 **brk-first+1** 项：前 brk-first
            // 项是第 0..brk-first 字符左缘（prefix[c] - base），末项 = 行尾右缘
            // （prefix[brk] - base）——行尾插入点，hit/光标/选区共用（对齐官方
            // VisLine.x 的 len+1 语义）。
            let base = prefix[first];
            let x: Vec<f32> = (first..=brk).map(|c| prefix[c] - base).collect();
            // 整行可见字符缓存：paint 逐行一次成型绘制（避免逐字符排版/入批）。
            let text: String = chars[first..brk].iter().collect();
            VRow {
                start: ls + first,
                end: ls + brk,
                x,
                text,
            }
        };
        let mut first = 0usize;
        while first < lc {
            // 从 first 尽量向右取满：last = 最后一个仍装得下的列。
            let mut last = first;
            while last + 1 < lc && prefix[last + 1] - prefix[first] <= max_w {
                last += 1;
            }
            if last + 1 == lc && prefix[lc] - prefix[first] <= max_w {
                // 剩余全部装下。末列右缘（prefix[lc]）必须显式检查：循环条件在
                // last+1 == lc 时提前退出，最后一个字符从未被比过——漏掉它会让
                // 末行（尤其整行只有 2 列时）超宽渲染而不折行。
                rows.push(row_for(first, lc));
                break;
            }
            // [first, last] 装不下 last+1，需断行：优先断在空格之后（空格留上行行尾）。
            let mut brk = last; // 无空格 → 断在 last 之后（本行取满）
            for c in (first + 1..=last).rev() {
                if chars[c] == ' ' {
                    brk = c + 1;
                    break;
                }
            }
            // 保底前进一列：首字符本身就超宽（如大字号 CJK 挤在窄盒里）时
            // 也要按字符折行，避免死循环。
            if brk <= first {
                brk = first + 1;
            }
            rows.push(row_for(first, brk));
            first = brk;
        }
        rows
    }

    // ── 几何换算（读最近一次 layout）────────────────────────────────

    /// 视图可用高（bounds 内缩 PAD）。
    fn view_h(b: Rect) -> f32 {
        (b.h - 2 * PAD).max(0) as f32
    }

    fn row_top(&self, b: Rect, line_h: f32, row: usize) -> f32 {
        b.y as f32 + PAD as f32 + row as f32 * line_h - self.scroll_y.get()
    }

    /// 全文下标 → 所在显示行下标。行首下标严格递增，二分定位
    /// （原为 O(行数) 全扫，光标行换算每帧多处调用）。
    fn row_of(&self, gi: usize) -> usize {
        let lay = self.layout.borrow();
        let gi = gi.min(lay.total);
        lay.rows
            .partition_point(|r| r.start <= gi)
            .saturating_sub(1)
    }

    /// 光标所在显示行：返回 (行下标, 该行引用)。
    fn caret_row<'a>(&self, lay: &'a Layout) -> (usize, &'a VRow) {
        let row = self.row_of(self.cursor.get());
        (row, &lay.rows[row.min(lay.rows.len().saturating_sub(1))])
    }

    /// 全文下标 → 行内插入点下标 [0, len]。
    ///
    /// `x` 已含行尾右缘（len+1 项），插入点与字符一一对应；gi 落在本行字符
    /// [start, end) 内时其列就是 `gi - start`（该字符左缘），落在 end 处时为
    /// 行尾插入点（行尾右缘）。
    fn col_of(lay: &Layout, row: usize, gi: usize) -> usize {
        let r = &lay.rows[row];
        if gi <= r.start {
            0
        } else if gi >= r.end {
            r.len()
        } else {
            gi - r.start
        }
    }

    /// 某显示行的内容下标区间 [start, end)。
    fn row_span(&self, row: usize) -> (usize, usize) {
        let lay = self.layout.borrow();
        let n = lay.rows.len();
        match lay.rows.get(row.min(n.saturating_sub(1))) {
            Some(r) => (r.start, r.end),
            None => (lay.total, lay.total),
        }
    }

    /// 绝对画布坐标 → 全文字符下标。与绘制同源：内容坐标系原点在
    /// bounds 左上 + PAD，绘制时再按 scroll 平移，故命中前把坐标减 PAD、加回滚动量。
    fn hit_index(&self, b: Rect, px: f32, py: f32) -> usize {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return self.cursor.get().min(self.char_count());
        }
        let cy = py - (b.y as f32 + PAD as f32) + self.scroll_y.get();
        let row = sel_core::row_at_y(cy, lay.line_h, lay.rows.len());
        let r = &lay.rows[row];
        // 行内 x 单调不减：二分定位点击点所在列。点击点在某显示字符中部时，
        // 落在左半偏左、右半偏右（与 SyntaxInput 同语义）。
        let cx = (px - (b.x as f32 + PAD as f32)).max(0.0);
        let col = sel_core::col_at_x(&r.x, cx);
        (r.start + col).min(lay.total)
    }

    // ── 选择操作（只读：无编辑/撤销/粘贴）────────────────────────────

    fn select_all(&self) {
        self.anchor.set(Some(0));
        self.cursor.set(self.char_count());
        self.follow.set(true);
    }

    /// 选中下标 gi 处"词"的区间（用于双击）：向两侧扫到空白界。
    fn word_around(&self, gi: usize) -> (usize, usize) {
        let chars: Vec<char> = self.text_clone().chars().collect();
        // 词 = 连续非空白（含 `_`，排除纯空白）；命中空白则只选该字符。
        sel_core::word_around(&chars, gi, |c| !c.is_whitespace())
    }

    /// 选中光标所在显示行整段（三击）。
    fn select_line(&self) {
        let row = self.row_of(self.cursor.get());
        let (s, e) = self.row_span(row);
        self.anchor.set(Some(s));
        self.cursor.set(e);
        self.follow.set(true);
    }

    /// 移动光标到 gi，同时按 shift 扩选 / 非 shift 清除选区。
    fn move_to(&self, gi: usize, shift: bool) {
        if shift && self.anchor.get().is_none() {
            self.anchor.set(Some(self.cursor.get()));
        }
        self.cursor.set(gi.min(self.char_count()));
        if !shift {
            self.anchor.set(None);
        }
        self.follow.set(true);
    }

    fn move_left(&self, shift: bool) {
        if !shift {
            if let Some((s, _)) = self.selection() {
                self.cursor.set(s);
                self.anchor.set(None);
                self.follow.set(true);
                return;
            }
        }
        if self.cursor.get() > 0 {
            self.move_to(self.cursor.get() - 1, shift);
        }
    }

    fn move_right(&self, shift: bool) {
        if !shift {
            if let Some((_, e)) = self.selection() {
                self.cursor.set(e);
                self.anchor.set(None);
                self.follow.set(true);
                return;
            }
        }
        if self.cursor.get() < self.char_count() {
            self.move_to(self.cursor.get() + 1, shift);
        }
    }

    fn move_home(&self, shift: bool) {
        let row = self.row_of(self.cursor.get());
        let s = self.row_span(row).0;
        self.move_to(s, shift);
    }

    fn move_end(&self, shift: bool) {
        let row = self.row_of(self.cursor.get());
        let e = self.row_span(row).1;
        self.move_to(e, shift);
    }

    fn move_vertical(&self, down: bool, shift: bool) {
        let row = self.row_of(self.cursor.get());
        let target = if down {
            (row + 1).min(self.layout.borrow().rows.len().saturating_sub(1))
        } else {
            row.saturating_sub(1)
        };
        if target == row {
            return;
        }
        // 跨显示行：落在目标行的同一全文下标（行内列概念在折行后不再成立，
        // 以全文下标平移语义即可满足输出文本的浏览需求）。
        let (s, e) = self.row_span(target);
        let gi = self.cursor.get().clamp(s, e);
        self.move_to(gi, shift);
    }

    /// 若光标越界，把视口滚到让光标可见（paint 在 follow 时调用）。
    fn ensure_visible(&self, b: Rect) {
        let vh = Self::view_h(b);
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        let row = self.row_of(self.cursor.get());
        let cy = row as f32 * lay.line_h;
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

    /// 拖选时指针越过上下边界，按越界距离滚动（每次至多一行）。
    fn auto_scroll_drag(&self, b: Rect, py: f32) {
        let lay = self.layout.borrow();
        if !lay.rows_valid() {
            return;
        }
        let vh = Self::view_h(b);
        let content_h = lay.rows.len() as f32 * lay.line_h;
        let max_sy = (content_h - vh).max(0.0);
        if max_sy <= 0.0 {
            return;
        }
        let top = b.y as f32 + PAD as f32;
        let bot = (b.y + b.h - PAD) as f32;
        let sy = self.scroll_y.get();
        if py < top && sy > 0.0 {
            self.scroll_y.set((sy - lay.line_h).max(0.0));
        } else if py > bot && sy < max_sy {
            self.scroll_y.set((sy + lay.line_h).min(max_sy));
        }
    }

    /// 右键菜单：复制（有选区才可点）/ 全选（有文本才可点）。
    /// 动作经合成 Ctrl+C / Ctrl+A 回送本控件，与 TextInput 右键菜单同一条通路。
    fn menu_items(&self) -> Vec<MenuItem> {
        let has_sel = self.selection().is_some();
        let has_text = self.char_count() > 0;
        let ctrl = |vk: u32| KeyEvent {
            key: Key::Other(vk),
            pressed: true,
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };
        vec![
            MenuItem::key(lang::MENU_COPY(), ctrl(0x43), has_sel),
            MenuItem::key(lang::MENU_SELECT_ALL(), ctrl(0x41), has_text),
        ]
    }
}

// 自由辅助：byte offset of char index。

/// 一逻辑行的前缀宽度表（`prefix[j]` = 前 j 个字符的**整串**实测宽，相对行首）。
/// 优先命中逐行宽度缓存；未命中才逐前缀整串测量（O(行长²) 字符工作量），测完回填。
///
/// 整串测量（而非逐字符独立测量再累加）：逐字符测量会各自取整，每字符最多
/// +1px、误差随行长线性放大——长 JSON 行累计漂移几十 px，换行点与绘制 x 一起
/// 错位，表现为字符串重叠。整串测量只有首尾各取整一次，与排版器实际 advance
/// 一致（含字距）。
fn line_prefix(
    line: &str,
    line_chars: &[char],
    measure: &mut impl FnMut(&str) -> f32,
    widths: &mut HashMap<String, Vec<f32>>,
) -> Vec<f32> {
    let lc = line_chars.len();
    if lc == 0 {
        return vec![0.0];
    }
    if let Some(cached) = widths.get(line) {
        return cached.clone();
    }
    let mut prefix = Vec::with_capacity(lc + 1);
    prefix.push(0.0);
    let mut acc = String::with_capacity(line.len());
    for &ch in line_chars {
        acc.push(ch);
        prefix.push(measure(&acc).max(0.0));
    }
    widths.insert(line.to_owned(), prefix.clone());
    prefix
}

// ───────────────────────────────────────────────────────────────────────────
// Widget
// ───────────────────────────────────────────────────────────────────────────

impl Widget for SelectText {
    fn measure(&self, avail: Size, style: &Style, text: &mut dyn TextEngine) -> Size {
        let font = style.font_size.max(1.0);
        let line_h = style.line_height.unwrap_or(1.4) * font;
        let ts = TextStyle::of(style);
        self.rebuild(line_h, text.scale(), avail.w, &ts, |s| {
            text.measure(s, &ts, None).w.max(0) as f32
        });
        let lay = self.layout.borrow();
        let intrinsic = (lay.rows.len() as f32 * lay.line_h) as i32 + 2 * PAD;
        let min = (lay.line_h as i32 + 2 * PAD).max(1);
        // 视口模型：父级给了真实高度就填满分配区（不随内容缩——否则短文本时控件只是
        // 一小条，点下方空白落不到控件上、无法聚焦）；无约束（含 windui 哨兵值
        // i32::MAX/4）时回退固有高度。哨兵判定统一走 layout_hint::viewport_height。
        let h = super::layout_hint::viewport_height(avail.h, intrinsic, min);
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
        // 底色/圆角/边框由核心层按节点 style 统一绘制（leaf 挂 `.bg_role` + `.corner`
        // 即可）；这里只画文字与选区。
        let colors = Colors::of(style);
        let font = style.font_size.max(1.0);
        let line_h = style.line_height.unwrap_or(1.4) * font;
        let ts = TextStyle::of(style);
        self.rebuild(line_h, canvas.dpi_scale(), bounds.w, &ts, |s| {
            canvas.measure_text(s, &ts).w.max(0) as f32
        });

        // 外部改写信号（清空/回填）不发事件：每帧先钳制一次，悬空的光标/锚点
        // 若不及时收口，下一帧的选区绘制就会错位。
        let _ = self.clamp();
        if self.follow.get() {
            self.ensure_visible(bounds);
            self.follow.set(false);
        }

        let lay = self.layout.borrow();
        // 外部改写信号可能把文本改短：scroll_y 是纯内部状态、无事件可钳制，每帧在此
        // 收口，否则内容变短后视口停在越界偏移上，可视区一片空白。
        let vh = Self::view_h(bounds);
        let content_h = lay.rows.len() as f32 * lay.line_h;
        let max_sy = (content_h - vh).max(0.0);
        if self.scroll_y.get() > max_sy {
            self.scroll_y.set(max_sy);
        }
        // 可视行范围 O(1) 求出（行高等距），替代逐行全扫：10 万行的输出每帧也只
        // 走可视的那几十行，高亮/绘制的循环次数与内容长度无关。
        let lh = lay.line_h.max(1.0);
        let sy = self.scroll_y.get();
        let n = lay.rows.len();
        let first_vis = ((sy / lh).floor() as usize).min(n.saturating_sub(1));
        let last_vis = (((sy + vh) / lh).ceil() as usize).min(n.saturating_sub(1));
        canvas.save();
        canvas.clip_rect(Rect::new(
            bounds.x + PAD,
            bounds.y + PAD,
            (bounds.w - 2 * PAD).max(0),
            (bounds.h - 2 * PAD).max(0),
        ));
        // 视口：整行落在视口外的一律剔除、根本不画——对齐 windui TextInput 多行渲染
        // 约定。可视行范围已在上方 O(1) 求出（first_vis..=last_vis）。
        // 选区高亮：半透明强调色，铺在文字**之下**（字形压在色块上保持可读）。
        // 必须画在 clip 内——否则滚动后隐藏行的底色会越过控件边界叠到相邻 UI 上
        // （文本被裁掉了、高亮没裁，边界错乱）。选区只属于焦点控件：失焦不画，
        // 且失焦帧（下方 else 分支）会清 anchor。隐藏行同样剔除。
        if focused {
            if let Some((as_, ae)) = self.selection() {
                let sel_c = colors.accent.scale_alpha(0.20);
                for row in first_vis..=last_vis {
                    let r = &lay.rows[row];
                    let ry = self.row_top(bounds, lay.line_h, row);
                    let pos = |gi: usize| -> usize {
                        if gi <= r.start {
                            0
                        } else if gi >= r.end {
                            r.len()
                        } else {
                            gi - r.start
                        }
                    };
                    let c1 = pos(as_);
                    let c2 = pos(ae);
                    if c1 < c2 {
                        let x1 = bounds.x as f32 + PAD as f32 + r.x[c1];
                        let x2 = bounds.x as f32 + PAD as f32 + r.x[c2];
                        canvas.fill_rect(
                            x1,
                            ry,
                            (x2 - x1).max(1.0),
                            lay.line_h,
                            &Paint::fill(sel_c),
                        );
                    }
                }
            }
        }
        // 正文：逐显示行一次成型（对齐 windui TextInput 多行渲染）。逐字符 draw_text
        // 时每字符都单独走一次排版 + 入批，长文本下这是主要开销；整行已折进盒内，
        // 这里整串画一次（NO_WRAP_W 防引擎二次折行）。行首 x[0] 作为该行绘制原点。
        let line_h_i = lay.line_h as i32;
        for row in first_vis..=last_vis {
            let r = &lay.rows[row];
            if r.text.is_empty() {
                continue; // 空行无字形
            }
            let ry = self.row_top(bounds, lay.line_h, row);
            let dx = (bounds.x as f32 + PAD as f32 + r.x[0]) as i32;
            canvas.draw_text(
                &r.text,
                Rect::new(dx, ry as i32, NO_WRAP_W, line_h_i),
                colors.text,
                Align::Start,
                &ts,
            );
        }
        canvas.restore();

        // 光标：聚焦时经 CaretState 落条——闪烁相位/续帧/点击后实心保持全由框架托管，
        // 与 TextInput 光标同一观感（方波跟系统闪烁周期；本控件纯文字无底色，
        // 不做输入框的"反色重绘字形"那一步）。
        if focused {
            // 复用上方已借用的 lay（同帧同一份布局，不再重复 borrow）
            if lay.rows_valid() {
                let (row, r) = self.caret_row(&lay);
                let x =
                    bounds.x as f32 + PAD as f32 + r.x[Self::col_of(&lay, row, self.cursor.get())];
                let y = self.row_top(bounds, lay.line_h, row);
                // 光标在 restore 之后画（不在文本 clip 内），故须自带视口裁剪：
                // 滚动后光标行滑出视口时若不裁，光标条会越过控件边界叠到相邻 UI 上
                // （表现为"光标飘到别处"）。视口外整根不画。
                let vtop = bounds.y + PAD;
                let vbot = bounds.y + bounds.h - PAD;
                if y >= vtop as f32 && y + lay.line_h <= vbot as f32 {
                    let t = windui::theme::current();
                    let opts = CaretOpts::from_theme(&t.input);
                    let _ = self.caret.paint(
                        canvas,
                        x as i32,
                        y as i32,
                        lay.line_h as i32,
                        t.input.cursor(&t.palette),
                        &opts,
                    );
                }
            }
        } else {
            // 失焦：选区只属于焦点控件——清掉锚点，避免屏幕上残留多个控件的高亮
            // （点别的输出框后，旧框的高亮随之消失，与系统编辑框失焦即取消选区一致）。
            if self.anchor.get().is_some() {
                self.anchor.set(None);
            }
            // blur 不走框架的 reset_interaction（只在隐藏/禁用翻转时触发），拖选态须自清：
            // 拖选中焦点被抢（如弹窗弹出）后指针再动会隐形改选区；也一并清光标滑行
            // 起点与脏区记录，下次聚焦按"首次"处理（不滑过来、不闪旧相位）。
            self.dragging.set(false);
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
                    // 不设 follow：点击处本就在视口内，若触发 ensure_visible 会把内容
                    // 滚到光标行——拖选期间文本在指针下方滑动，后续 Move 的命中坐标
                    // 随之错位（表现为选区/光标飘忽乱跳）。视口滚动只归键盘导航与
                    // auto_scroll_drag 管。
                    // 右键：弹上下文菜单（复制/全选），不改动选区。
                    if p.button != MouseButton::Left {
                        if p.button == MouseButton::Right {
                            ctx.show_context_menu(p.pos, self.menu_items());
                        }
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
                    ctx.capture();
                    ctx.mark_dirty();
                    true
                }
                PointerKind::Move => {
                    if self.dragging.get() {
                        self.auto_scroll_drag(b, p.pos.y as f32);
                        let idx = self.hit_index(b, p.pos.x as f32, p.pos.y as f32);
                        self.cursor.set(idx);
                        ctx.mark_dirty();
                        true
                    } else {
                        false
                    }
                }
                PointerKind::Up => {
                    if self.dragging.get() {
                        self.dragging.set(false);
                        ctx.release_capture();
                        if self.anchor.get() == Some(self.cursor.get()) {
                            self.anchor.set(None);
                        }
                        ctx.mark_dirty();
                        true
                    } else {
                        false
                    }
                }
                PointerKind::Wheel(steps) => {
                    let lay = self.layout.borrow();
                    if !lay.rows_valid() {
                        return false;
                    }
                    let vh = Self::view_h(b);
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
                _ => false,
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

    fn wants_right_click(&self) -> bool {
        true // 右键弹出上下文菜单（复制/全选）
    }

    /// 选区（字符下标）——供框架/IME 查询（只读无 IME，但仍返回供一致）。
    fn selection_range(&self) -> Option<(usize, usize)> {
        let c = self.cursor.get();
        Some(self.selection().unwrap_or((c, c)))
    }

    fn reset_interaction(&mut self) {
        self.anchor.set(None);
        let c = self.cursor.get().min(self.char_count());
        self.cursor.set(c);
        self.dragging.set(false);
        self.follow.set(true);
        self.caret.reset();
    }
}

impl SelectText {
    /// 键盘处理（win32 语义）：导航键移动/扩选，Ctrl+C 复制，Ctrl+A 全选。
    /// 其余键（可打印字符/退格等）不消费——只读控件不应响应编辑键。
    fn on_key(&self, ctx: &mut EventCtx, k: KeyEvent) -> bool {
        match k.key {
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
            Key::Other(0x43) if k.ctrl => {
                // Ctrl+C：复制选区（有选区时）。
                if let Some(sel) = self.selected_str() {
                    ctx.clipboard_set(&sel);
                }
                true
            }
            Key::Other(0x41) if k.ctrl => {
                self.select_all();
                ctx.mark_dirty();
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windui::signal::{signal, SignalScope};

    /// 死句柄容错：信号被 dispose 后，new / 读取 / rebuild / 复制 全链路不 panic，
    /// 一律回退空串 / 0（曾致主题切换整树重建时窗体闪退，signal.rs:545）。
    #[test]
    fn dead_signal_handle_degrades_gracefully() {
        let sig = {
            let mut scope = SignalScope::new();
            let s = scope.collect(|| signal(String::from("hello")));
            assert!(s.is_alive());
            s
            // scope drop → dispose → 句柄失效（模拟整树重建回收构建期信号）
        };
        assert!(!sig.is_alive());

        // 构造（含 cursor 初始化）不 panic
        let st = SelectText::new(sig);
        // 各读取路径回退空串 / 0，不再触发 signal.rs:545
        assert_eq!(st.char_count(), 0);
        assert_eq!(st.text_clone(), "");
        assert_eq!(st.selected_str(), None);
        // 布局重建走"文本已变"分支，产出空布局而非 panic
        let ts = TextStyle::new(14.0);
        st.rebuild(19.6, 1.0, 300, &ts, |p| (p.chars().count() as f32) * 10.0);
        let lay = st.layout.borrow();
        assert!(lay.valid);
        assert_eq!(lay.total, 0);
    }

    /// 活信号路径回归：容错改造不改变正常读写行为。
    #[test]
    fn alive_signal_behavior_unchanged() {
        let sig = signal(String::from("ab\ncd"));
        let st = SelectText::new(sig);
        assert_eq!(st.char_count(), 5);
        assert_eq!(st.text_clone(), "ab\ncd");
        // 全选后复制拿到全文
        st.select_all();
        assert_eq!(st.selected_str().as_deref(), Some("ab\ncd"));
        // 外部改写文本后 char_count 跟随
        sig.set(String::from("xyz"));
        assert_eq!(st.char_count(), 3);
    }

    /// 等宽 10px 的前缀宽度表（前 j 字符宽 = 10j）。
    fn mono_prefix(lc: usize) -> Vec<f32> {
        (0..=lc).map(|j| j as f32 * 10.0).collect()
    }

    /// 核心不变量：各显示行严格划分输入行（连续、无缝、不重叠），
    /// 行内 x 单调不减且首为 0，行宽不超 max_w（首字符超宽除外）。
    fn assert_partition(chars: &str, rows: &[VRow], lc: usize, max_w: f32) {
        assert_eq!(rows[0].start, 0);
        for r in rows {
            assert_eq!(
                r.x.len(),
                r.end - r.start + 1,
                "x 列数 = 字符数+1（末项行尾右缘）"
            );
            assert!(r.x.first() == Some(&0.0), "行首 x 归零");
            for w in r.x.windows(2) {
                assert!(w[1] >= w[0], "x 单调不减");
            }
            if r.end - r.start > 1 {
                assert!(r.x.last().unwrap() - 0.0 <= max_w + 1e-3, "行宽不超盒");
            }
        }
        for (i, r) in rows.iter().enumerate() {
            if i + 1 < rows.len() {
                assert_eq!(r.end, rows[i + 1].start, "严格划分：无缝不重叠");
            }
        }
        assert_eq!(rows.last().unwrap().end, lc);
        // 每个字符恰出现一次（拼回原文）。start/end 是字符下标，须按字符切片。
        let joined: String = rows
            .iter()
            .flat_map(|r| chars.chars().skip(r.start).take(r.end - r.start))
            .collect();
        assert_eq!(joined, chars, "字符零丢失零重复");
    }

    #[test]
    fn wrap_single_row_when_fits() {
        let cs: Vec<char> = "hello".chars().collect();
        let rows = SelectText::wrap_line(&cs, 0, &mono_prefix(5), 100.0);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].start, rows[0].end), (0, 5));
        assert_eq!(rows[0].x, vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
    }

    #[test]
    fn wrap_no_space_breaks_mid_word_at_capacity() {
        // 5 字符 × 10px，盒 30px → 每行 3 字符；无空格，逐字符折。
        let cs: Vec<char> = "abcdef".chars().collect();
        let rows = SelectText::wrap_line(&cs, 0, &mono_prefix(6), 30.0);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].start, rows[0].end), (0, 3));
        assert_eq!((rows[1].start, rows[1].end), (3, 6));
        // x = 3 字符左缘 + 行尾右缘（整行宽 30）。
        assert_eq!(rows[0].x, vec![0.0, 10.0, 20.0, 30.0]);
        assert_eq!(rows[1].x, vec![0.0, 10.0, 20.0, 30.0]);
        assert_partition("abcdef", &rows, 6, 30.0);
    }

    #[test]
    fn wrap_prefers_space_break_keeps_trailing_space() {
        // "ab cd ef"，盒 35px。逐字 10px：0-34 装 "ab cd "（6 字 60px？不，35 装 3 字）。
        // 35px → 每行 3 字符。首行 "ab "（在空格后断，空格留上行）；次行 "cd "；末行 "ef"。
        let cs: Vec<char> = "ab cd ef".chars().collect();
        let rows = SelectText::wrap_line(&cs, 0, &mono_prefix(8), 35.0);
        let spans: Vec<(usize, usize)> = rows.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(spans, vec![(0, 3), (3, 6), (6, 8)], "优先在空格后断行");
        assert_partition("ab cd ef", &rows, 8, 35.0);
        // 行内 x 归零且正确（相对行首前缀差）；行 2 = "cd " 三字符 + 行尾右缘。
        assert_eq!(rows[1].x, vec![0.0, 10.0, 20.0, 30.0]);
    }

    #[test]
    fn wrap_oversized_single_char_advances() {
        // 首字符自身就超宽（10px 字符 > 5px 盒）：保底断 1 列/行，不死循环。
        let cs: Vec<char> = "ab".chars().collect();
        let rows = SelectText::wrap_line(&cs, 0, &mono_prefix(2), 5.0);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].start, rows[0].end), (0, 1));
        assert_eq!((rows[1].start, rows[1].end), (1, 2));
    }

    #[test]
    fn wrap_empty_line() {
        let cs: Vec<char> = String::new().chars().collect();
        let rows = SelectText::wrap_line(&cs, 0, &mono_prefix(0), 100.0);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].start, rows[0].end), (0, 0));
        assert_eq!(rows[0].x, vec![0.0]);
    }

    /// 回归：断行后第二行不得把上一行末尾字符重复收进 x（旧实现 off-by-one
    /// 导致行 2 起 x 多一列且整体错位，多行文本叠在一起）。
    #[test]
    fn wrap_second_row_starts_fresh() {
        let cs: Vec<char> = "aaaaa".chars().collect();
        let rows = SelectText::wrap_line(&cs, 0, &mono_prefix(5), 30.0);
        assert_eq!(rows.len(), 2);
        // 行 2 的 x 必须恰 3 项（2 字符 a a + 行尾右缘），而不是 4 项
        // （误含行 1 末尾的 a 会变成 3 字符 + 右缘）。
        assert_eq!(
            rows[1].x.len(),
            3,
            "第二行列数 = 第二行字符数+1（行尾右缘）"
        );
        assert_eq!(rows[1].x, vec![0.0, 10.0, 20.0]);
        assert_partition("aaaaa", &rows, 5, 30.0);
    }

    /// 逐行宽度缓存：同 key 第二次调用不再逐前缀测量。
    #[test]
    fn line_prefix_cache_hits_on_same_line() {
        let mut calls = 0;
        let mut widths = HashMap::new();
        let cs: Vec<char> = "hello".chars().collect();
        let _ = line_prefix(
            "hello",
            &cs,
            &mut |_| {
                calls += 1;
                10.0
            },
            &mut widths,
        );
        assert_eq!(calls, 5, "未命中时逐前缀测量");
        let _ = line_prefix(
            "hello",
            &cs,
            &mut |_| {
                calls += 1;
                10.0
            },
            &mut widths,
        );
        assert_eq!(calls, 5, "命中缓存后不再测量");
    }

    /// 宽度变化强制 rebuild（换行点变），但行内容未变 → 前缀全部命中缓存。
    #[test]
    fn rebuild_width_change_reuses_prefix_cache() {
        // 等宽：宽 = 字符数 × 10。
        let st = SelectText::new(signal(String::from("ab\ncd")));
        let ts = TextStyle::new(14.0);
        st.rebuild(19.6, 1.0, 300, &ts, |p| (p.chars().count() as f32) * 10.0);
        let mut calls = 0;
        st.rebuild(19.6, 1.0, 25, &ts, |p| {
            calls += 1;
            (p.chars().count() as f32) * 10.0
        });
        assert_eq!(calls, 0, "未变的行应命中宽度缓存");
        // 宽 25 → max_w 13 → 每行 1 字符（第 2 字符右缘 20 > 13），逐字符折行。
        let lay = st.layout.borrow();
        let spans: Vec<(usize, usize)> = lay.rows.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(spans, vec![(0, 1), (1, 2), (3, 4), (4, 5)]);
    }

    /// CJK（多字节字符）折行：按字符列而非字节下标折，宽表仍按字符计数。
    #[test]
    fn wrap_cjk_no_panic_and_partition() {
        let cs: Vec<char> = "中文文本段落".chars().collect(); // 6 字符
        let rows = SelectText::wrap_line(&cs, 0, &mono_prefix(6), 30.0);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].start, rows[0].end), (0, 3));
        assert_eq!((rows[1].start, rows[1].end), (3, 6));
        assert_partition("中文文本段落", &rows, 6, 30.0);
    }

    /// 空文本 rebuild：产出单个空显示行，total = 0，不 panic。
    #[test]
    fn rebuild_empty_text() {
        let st = SelectText::new(signal(String::new()));
        let ts = TextStyle::new(14.0);
        st.rebuild(19.6, 1.0, 300, &ts, |p| (p.chars().count() as f32) * 10.0);
        let lay = st.layout.borrow();
        assert_eq!(lay.total, 0);
        assert_eq!(lay.rows.len(), 1, "空文本仍有一行（空显示行）");
        assert_eq!(lay.rows[0].x, vec![0.0]);
    }

    /// 文本以 '\n' 结尾 → 补一个空显示行（与输入框一致的切分语义）。
    #[test]
    fn rebuild_trailing_newline_gets_empty_row() {
        let st = SelectText::new(signal(String::from("ab\n")));
        let ts = TextStyle::new(14.0);
        st.rebuild(19.6, 1.0, 300, &ts, |p| (p.chars().count() as f32) * 10.0);
        let lay = st.layout.borrow();
        assert_eq!(lay.total, 3);
        assert_eq!(lay.rows.len(), 2, "trailing \\n 产生文末空行");
        assert_eq!((lay.rows[1].start, lay.rows[1].end), (3, 3));
    }

    /// 宽度缓存按条数兜底：超上限整体清空，不无限膨胀。
    #[test]
    fn width_cache_capped_by_entries() {
        let st = SelectText::new(signal(String::new()));
        let ts = TextStyle::new(14.0);
        // 借 rebuild 的缓存清理路径：直接灌满缓存再触发一次 rebuild。
        {
            let mut lay = st.layout.borrow_mut();
            for i in 0..=WIDTH_CACHE_MAX_ENTRIES {
                lay.widths.insert(format!("line-{i}"), vec![0.0; i % 5 + 1]);
            }
        }
        let ts2 = TextStyle::new(15.0); // 换度量指纹 → 强制走 rebuild 路径
        st.rebuild(19.6, 1.0, 300, &ts2, |p| (p.chars().count() as f32) * 10.0);
        // 指纹变化时缓存即清空；条数兜底是第二道防线（针对同指纹下不断涌入新行）。
        assert!(st.layout.borrow().widths.len() <= WIDTH_CACHE_MAX_ENTRIES);
        let _ = ts; // 度量指纹按 (字族, 字号, scale) 计算
    }

    /// 窄盒 + 超宽 CJK 首字符：保底前进一列，不死循环。
    #[test]
    fn wrap_oversized_cjk_first_char_terminates() {
        // 每字符宽 100，盒 50 → 首字符即超宽。
        let cs: Vec<char> = "中文".chars().collect();
        let prefix: Vec<f32> = (0..=2).map(|j| j as f32 * 100.0).collect();
        let rows = SelectText::wrap_line(&cs, 0, &prefix, 50.0);
        assert_eq!(rows.len(), 2, "每行保底 1 字符，恰好终止");
        assert_partition("中文", &rows, 2, 50.0);
    }
}
