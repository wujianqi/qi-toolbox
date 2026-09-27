//! 只读/编辑文本控件的共用选区与命中几何（`select_text` 与 `syntax_input` 复用）。
//!
//! 两个控件各自维护布局缓存，但以下逻辑语义完全一致，抽到这里统一实现与测试：
//! - [`normalize_selection`]：光标/锚点 → 规范化选区 `[start, end)`
//! - [`byte_at`] / [`selected_str`]：字符下标 → 字节下标换算与选区文本提取
//! - [`col_at_x`]：行内 x 坐标 → 字符列的二分定位（半开区间归属语义）
//! - [`row_at_y`]：y 坐标 → 行号（按行高钳制到首末行）
//! - [`word_around`]：双击选词的词边界扫描

/// 字符下标 → 行内字节偏移（越界钳到文末）。
pub fn byte_at(s: &str, gi: usize) -> usize {
    s.char_indices().nth(gi).map(|(b, _)| b).unwrap_or(s.len())
}

/// 规范化选区：光标 `cursor`、锚点 `anchor`（`None` = 无选区），钳到 `total`
/// 内并排序为 `[start, end)`；锚点缺失或与光标重合返回 `None`。
pub fn normalize_selection(
    cursor: usize,
    anchor: Option<usize>,
    total: usize,
) -> Option<(usize, usize)> {
    let c = cursor.min(total);
    let a = anchor?.min(total);
    if a == c {
        None
    } else if a < c {
        Some((a, c))
    } else {
        Some((c, a))
    }
}

/// 选区文本（供复制）。无选区（或文本被外部改写导致越界）返回 `None`。
pub fn selected_str(text: &str, sel: Option<(usize, usize)>) -> Option<String> {
    let (s, e) = sel?;
    let (bs, be) = (byte_at(text, s), byte_at(text, e));
    Some(text[bs..be].to_string())
}

/// 行内 x 坐标 → 字符列。`x` 为 len+1 项的前缀偏移表（末项 = 行尾右缘），
/// 单调不减，二分定位；点击点在某字符中部时按半开区间 `[x[lo], x[hi])`
/// 归属——中点之前算 lo、之后算 hi（与官方 TextInput 同语义）。
pub fn col_at_x(x: &[f32], cx: f32) -> usize {
    let len = x.len().saturating_sub(1);
    if cx <= x[0] {
        0
    } else if cx >= *x.last().unwrap_or(&0.0) {
        len
    } else {
        let mut lo = 0usize;
        let mut hi = len;
        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            if x[mid] <= cx {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        if cx <= (x[lo] + x[hi]) / 2.0 {
            lo
        } else {
            hi
        }
    }
}

/// 内容区 y 坐标 → 行号（按 `line_h` 换算并钳制到 `[0, rows-1]`）。
pub fn row_at_y(cy: f32, line_h: f32, rows: usize) -> usize {
    let line_h = line_h.max(1.0);
    let floor = (cy / line_h).floor();
    if floor < 0.0 {
        0
    } else {
        (floor as usize).min(rows.saturating_sub(1))
    }
}

/// 双击选词：`is_word` 判定字符是否属于词；命中非词字符（空白/分隔符）只选
/// 该字符本身。返回全文 `[start, end)` 字符区间。
pub fn word_around(chars: &[char], gi: usize, is_word: impl Fn(char) -> bool) -> (usize, usize) {
    let total = chars.len();
    if total == 0 {
        return (0, 0);
    }
    let mut gi = gi.min(total.saturating_sub(1));
    // 命中换行符时回退到上一行末字符（双击行尾空白处仍选到词）。
    while gi > 0 && chars[gi] == '\n' {
        gi -= 1;
    }
    if !is_word(chars[gi]) {
        return (gi, (gi + 1).min(total));
    }
    let mut s = gi;
    while s > 0 && is_word(chars[s - 1]) {
        s -= 1;
    }
    let mut e = gi + 1;
    while e < total && is_word(chars[e]) {
        e += 1;
    }
    (s, e)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn normalize_selection_orders_and_clamps() {
        assert_eq!(normalize_selection(3, Some(1), 10), Some((1, 3)));
        assert_eq!(normalize_selection(1, Some(3), 10), Some((1, 3)));
        assert_eq!(normalize_selection(2, Some(2), 10), None);
        assert_eq!(normalize_selection(2, None, 10), None);
        // 越界钳制。
        assert_eq!(normalize_selection(99, Some(1), 10), Some((1, 10)));
    }

    #[test]
    fn col_at_x_half_open() {
        // 每字符宽 10，共 3 字符（x 有 4 项）。
        let x = [0.0, 10.0, 20.0, 30.0];
        assert_eq!(col_at_x(&x, -5.0), 0);
        assert_eq!(col_at_x(&x, 0.0), 0);
        // 字符 1 占 [10,20)：中点 15 之前算本列 1、之后算下一列 2。
        assert_eq!(col_at_x(&x, 14.9), 1);
        assert_eq!(col_at_x(&x, 15.1), 2);
        assert_eq!(col_at_x(&x, 99.0), 3);
    }

    #[test]
    fn row_at_y_clamps() {
        assert_eq!(row_at_y(-10.0, 20.0, 5), 0);
        assert_eq!(row_at_y(45.0, 20.0, 5), 2);
        assert_eq!(row_at_y(999.0, 20.0, 5), 4);
        // 行高 0/负值按 1 处理，不 panic。
        assert_eq!(row_at_y(3.0, 0.0, 5), 3);
    }

    #[test]
    fn word_around_scans_boundaries() {
        let chars: Vec<char> = "foo barbaz\n qux".chars().collect();
        assert_eq!(word_around(&chars, 1, |c| !c.is_whitespace()), (0, 3));
        assert_eq!(word_around(&chars, 5, |c| !c.is_whitespace()), (4, 10));
        // 命中空白：只选该字符。
        assert_eq!(word_around(&chars, 3, |c| !c.is_whitespace()), (3, 4));
        // 命中换行：回退到上一行末字符。
        assert_eq!(word_around(&chars, 10, |c| !c.is_whitespace()), (4, 10));
    }

    /// byte_at 对空串 / 单字符多字节串的边界。
    #[test]
    fn byte_at_empty_and_multibyte_only() {
        assert_eq!(byte_at("", 0), 0, "空串任意下标钳到 0");
        assert_eq!(byte_at("", 9), 0);
        // 整串都是多字节：每个字符偏移都落在字符边界。
        let s = "中文";
        assert_eq!(byte_at(s, 1), 3);
        assert_eq!(byte_at(s, 2), 6);
        assert_eq!(byte_at(s, 99), 6);
    }

    /// selected_str：无选区 / 越界选区 / CJK 选区。
    #[test]
    fn selected_str_boundaries() {
        assert_eq!(selected_str("hello", None), None);
        assert_eq!(
            selected_str("hello", Some((2, 2))),
            Some(String::new()),
            "空选区 = 空串"
        );
        // 越界钳到文末（外部改写文本后旧选区可能越界）。
        assert_eq!(selected_str("hello", Some((2, 99))), Some("llo".into()));
        assert_eq!(
            selected_str("你好", Some((0, 1))),
            Some("你".into()),
            "CJK 按字符下标取"
        );
        // 外部改写后选区完全越界 → None 而非 panic。
        assert_eq!(selected_str("hi", Some((5, 9))), Some(String::new()));
    }

    /// col_at_x：空表 / 两项表 / 全越界。
    #[test]
    fn col_at_x_edge_tables() {
        assert_eq!(col_at_x(&[0.0], 50.0), 0, "单点表永远列 0");
        assert_eq!(col_at_x(&[0.0, 10.0], 3.0), 0);
        assert_eq!(col_at_x(&[0.0, 10.0], 7.0), 1);
        assert_eq!(col_at_x(&[0.0, 10.0], 99.0), 1, "越界钳到末列");
        // 负坐标也钳到列 0。
        assert_eq!(col_at_x(&[0.0, 10.0], -3.0), 0);
    }

    /// row_at_y：首行点击 / 恰在行边界 / 空行数表。
    #[test]
    fn row_at_y_edge_rows() {
        assert_eq!(row_at_y(0.0, 20.0, 1), 0);
        // 恰在行边界 y=20 → 第 1 行（半开区间 [0,20) 属行 0）。
        assert_eq!(row_at_y(20.0, 20.0, 3), 1);
        assert_eq!(row_at_y(0.0, 0.0, 3), 0, "行高 0 不 panic");
        assert_eq!(row_at_y(5.0, 20.0, 0), 0, "零行表钳到 0");
    }

    /// word_around：中文词、串首/串尾命中。
    #[test]
    fn word_around_cjk_and_edges() {
        let chars: Vec<char> = "中文 text".chars().collect();
        // 命中串首词。
        assert_eq!(word_around(&chars, 0, |c| !c.is_whitespace()), (0, 2));
        // 命中串尾词。
        assert_eq!(word_around(&chars, 6, |c| !c.is_whitespace()), (3, 7));
        // 单字符串。
        let one: Vec<char> = "x".chars().collect();
        assert_eq!(word_around(&one, 0, |c| !c.is_whitespace()), (0, 1));
    }
}
