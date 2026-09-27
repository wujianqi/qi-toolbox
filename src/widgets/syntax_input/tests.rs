use super::*;
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
    si.rebuild(
        14.0,
        19.6,
        2.0,
        &TextStyle::new(14.0),
        &HighlightColors::from_theme().as_palette(),
        |_| 20.0,
    );
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
        runs.iter()
            .any(|(k, c1, c2)| *k == TokenKind::Comment && *c1 == 9 && *c2 == 13),
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

/// 多行布局不变量：行内 x 表形态正确；相邻行 start 严格递增且只隔 1 个
/// 换行符（syntax_input 的显示行**不含** `\n`，与 SelectText 的全文划分不同）。
#[test]
fn layout_rows_partition_the_text() {
    let si = build("SELECT a,\n  b\nFROM t");
    let lay = si.layout.borrow();
    assert_eq!(lay.rows[0].start, 0);
    for r in &lay.rows {
        assert_eq!(
            r.x.len(),
            r.len() + 1,
            "x 列数 = 行字符数+1（含行尾插入点）"
        );
        assert_eq!(r.x[0], 0.0, "行首 x 归零");
        for w in r.x.windows(2) {
            assert!(w[1] >= w[0], "x 单调不减");
        }
    }
    for (i, r) in lay.rows.iter().enumerate() {
        if i + 1 < lay.rows.len() {
            assert_eq!(
                lay.rows[i + 1].start,
                r.end_full() + 1,
                "下一行起点 = 本行末 + 1 个换行符"
            );
        }
    }
    // "SELECT a,"(9) + \n + "  b"(3) + \n + "FROM t"(6) = 20 字符。
    assert_eq!(
        lay.rows.last().unwrap().end_full(),
        20,
        "末行 end = 全文字符数"
    );
    assert_eq!(lay.total, 20);
}

/// 布局对空文本 / 仅换行文本的边界：不 panic、行数正确、越界查询钳制。
#[test]
fn layout_empty_and_newline_only() {
    let e = build("");
    {
        let lay = e.layout.borrow();
        assert_eq!(lay.rows.len(), 1, "空文本 = 单个空显示行");
        assert_eq!(lay.line_of(0), (0, 0));
        assert_eq!(lay.row_len(0), 0);
    }
    let n = build("\n");
    {
        let lay = n.layout.borrow();
        assert_eq!(lay.rows.len(), 2, "\\n 切出两个空行");
        assert_eq!(lay.row_span(99), (1, 1), "越界行钳到文末空行");
    }
}

/// 外部把信号改长（不经过控件事件）：未越界的光标/锚点原样保留（clamp 只钳越界值）。
#[test]
fn clamp_cursor_on_external_grow_keeps_valid_state() {
    let si = build("hi");
    si.cursor.set(2);
    si.anchor.set(Some(0));
    si.text.set(String::from("hello world"));
    assert!(!si.clamp_cursor(), "光标 2 未越界，无需修正");
    assert_eq!(si.cursor.get(), 2);
    assert_eq!(si.anchor.get(), Some(0), "合法选区不因文本变长作废");
}

/// 选区删除（delete_selection）：CJK 与跨多字节边界均按字符下标处理。
#[test]
fn delete_selection_cjk() {
    let si = build("你好世界");
    si.cursor.set(1);
    si.anchor.set(Some(3));
    assert!(si.delete_selection());
    assert_eq!(si.text.with(|t| t.clone()), "你界");
    assert_eq!(si.cursor.get(), 1, "删除后光标落在选区起点");
    // 无选区时返回 false、文本不变。
    assert!(!si.delete_selection());
    assert_eq!(si.text.with(|t| t.clone()), "你界");
}

/// 连续撤销回到初始快照后，再撤销是 no-op；redo 栈随新编辑清空。
#[test]
fn undo_past_base_and_redo_cleared_by_edit() {
    let si = build("ab");
    si.cursor.set(2);
    si.type_char('c');
    si.undo();
    assert_eq!(si.text.with(|t| t.clone()), "ab");
    si.undo(); // 已到初始快照，再撤销无变化、不 panic
    assert_eq!(si.text.with(|t| t.clone()), "ab");
    si.redo();
    assert_eq!(si.text.with(|t| t.clone()), "abc");
    si.type_char('d'); // 新编辑 → redo 栈必须作废
    si.undo();
    assert_eq!(si.text.with(|t| t.clone()), "abc", "重做栈已被新编辑清空");
}

/// hit_index 对 CJK（多字节）文本按字符列返回，不落进字节中间。
#[test]
fn hit_index_cjk_columns() {
    let si = build("中文");
    let b = Rect::new(0, 0, 200, 60);
    assert_eq!(si.hit_index(b, 5.0, 10.0), 0, "第 0 字符左半");
    assert_eq!(si.hit_index(b, 15.0, 10.0), 1, "第 1 字符中部");
    assert_eq!(si.hit_index(b, 25.0, 10.0), 2, "行尾插入点");
}
