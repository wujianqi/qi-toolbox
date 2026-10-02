//! 简单月历控件（与业务无关，供备忘页等选择日期的场景复用）
//!
//! 翻月/标记变化经 `host_signal` 重建格子（构建期算好颜色与粗细），
//! 选中高亮走 `visible_when` 逐帧跟随——修复了内嵌版「箭头点了不刷新」的问题。
//!
//! 用法：
//! ```ignore
//! let sel = signal(String::new()); // "YYYY-MM-DD"，空 = 未选
//! let marks = signal(vec!["2026-10-02".to_string()]);
//! calendar_panel(sel, marks)
//! ```

use windui::prelude::*;

/// 月历宽（逻辑 px，7 列日期格子）
pub const CAL_W: i32 = 260;
/// 单元格高
const CELL_H: i32 = 26;
/// 网格总格数（6 行 × 7 列，足够容纳任意月份的占位 + 日期）
const CELLS: usize = 42;
/// 有数据日期的绿色（加粗显示）
const HAS_DATA_GREEN: u32 = 0x2E9E44;

/// 独立月历面板：年月切换 + 周一起始的 7 列网格。
///
/// - `selected`：选中日期信号（"YYYY-MM-DD"，空 = 未选；再点同一天取消）。
///   点击只写本信号，过滤逻辑由调用方按需实现（如备忘列表按选中日过滤）
/// - `marks`：有数据的日期集合（如有关联备忘的日子）——这些日期显示为
///   加粗绿色，其余日期常规样式。列表变化会自动刷新格子
/// - `on_pick`：每次点选/取消日期后的回调（供调用方刷新过滤视图等）
pub fn calendar_panel(
    selected: Signal<String>,
    marks: Signal<Vec<String>>,
    on_pick: impl Fn() + 'static,
) -> Element {
    // 浏览年月是控件内部状态；epoch 为重建触发器（翻月/标记变化时 push，
    // host_signal 收到即整体重建格子）。顶层创建的信号跨整树重建存活
    let (ty, tm) = today_ym();
    let ym = signal((ty, tm));
    let epoch = signal(vec![()]);

    // ── 头部：◀ 年月 ▶（翻月 = set 年月 + 触发格子重建）──
    let (ym_p, epoch_p) = (ym, epoch);
    let prev_btn = Element::button("◀")
        .small()
        .neutral()
        .on_click(move |_| {
            let (y, m) = ym_p.get();
            if m == 1 {
                ym_p.set((y - 1, 12));
            } else {
                ym_p.set((y, m - 1));
            }
            // 替换式触发重建：push 会让 host_signal 视为新数据无限追加行
            epoch_p.set(vec![()]);
        });
    let (ym_n, epoch_n) = (ym, epoch);
    let next_btn = Element::button("▶")
        .small()
        .neutral()
        .on_click(move |_| {
            let (y, m) = ym_n.get();
            if m == 12 {
                ym_n.set((y + 1, 1));
            } else {
                ym_n.set((y, m + 1));
            }
            epoch_n.set(vec![()]);
        });
    let ym_label = Element::label_signal(ym.map(|v: &(i32, u32)| {
        let (y, m) = *v;
        format!("{}-{:02}", y, m)
    }))
    .font_size(14.0)
    .font_weight(600)
    .fg_role(Role::Text)
    // align 只定布局槽位，文字在本格内还需 text_align 才真正水平居中
    .text_align(Align::Center)
    .align(Align::Center);

    // ── 日期格子：epoch 变化（翻月/标记更新）时整体重建；选中高亮走信号 ──
    let (ym_g, marks_g, sel_g, pick_g) = (ym, marks, selected, std::rc::Rc::new(on_pick));
    let grid = Element::host_signal(epoch, move |_| {
        let (y, m) = ym_g.get();
        let marks_now = marks_g.get();
        let today = today_parts();
        // 表头：一 二 三 四 五 六 日
        let mut cells: Vec<Element> = Vec::new();
        for wd in ["一", "二", "三", "四", "五", "六", "日"] {
            cells.push(
                Element::label(wd.to_string())
                    .font_size(11.0)
                    .fg_role(Role::TextMuted)
                    .align(Align::Center),
            );
        }
        for i in 0..CELLS {
            let day = cell_day(y, m, i);
            let (sel_c, pick_c) = (sel_g, pick_g.clone());
            cells.push(match day {
                Some(d) => {
                    let day_str = d.clone();
                    let day_str2 = d.clone();
                    let day_num: u32 = d[8..].parse().unwrap_or(0);
                    let has_data = marks_now.contains(&d);
                    let is_today = (y, m) == (today.0, today.1) && today.2 == day_num;
                    let text = if is_today {
                        format!("{}*", day_num)
                    } else {
                        day_num.to_string()
                    };
                    let mut label = Element::label(text).font_size(12.0).align(Align::Center);
                    label = if has_data {
                        // 有数据：加粗绿色
                        label
                            .font_weight(700)
                            .fg(Color::hex(HAS_DATA_GREEN))
                    } else {
                        label.font_weight(400).fg_role(Role::Text)
                    };
                    Element::stack()
                        .width_match()
                        .height(CELL_H)
                        .corner(6.0)
                        .clickable()
                        .on_click(move |_| {
                            // 再点同一天取消选中；随后通知调用方刷新过滤视图
                            if sel_c.get() == day_str {
                                sel_c.set(String::new());
                            } else {
                                sel_c.set(day_str.clone());
                            }
                            pick_c();
                        })
                        // 底层：选中日高亮背景
                        .child(
                            Element::leaf()
                                .fill()
                                .corner(6.0)
                                .bg_role(Role::Accent)
                                .visible_when(move || sel_c.get() == day_str2),
                        )
                        .child(label)
                }
                // 无效格：空白占位
                None => Element::leaf().height(CELL_H),
            });
        }
        Element::grid(7, 2, cells)
    });

    Element::col()
        .width(CAL_W)
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding(12)
        .spacing(8)
        .child(
            Element::row()
                .width_match()
                .cross(Align::Center)
                .child(prev_btn)
                .child(ym_label.weight(1.0))
                .child(next_btn),
        )
        .child(grid)
}

/// 当前浏览年月下第 `i` 格（0..42）对应的日期串，无效格返回 None
fn cell_day(y: i32, m: u32, i: usize) -> Option<String> {
    let first_wd = weekday_of_first(y, m) as usize;
    let dim = days_in_month(y, m);
    let day = i as i32 - first_wd as i32 + 1;
    if day >= 1 && day as u32 <= dim {
        Some(format!("{:04}-{:02}-{:02}", y, m, day))
    } else {
        None
    }
}

/// 本地今天（东八区近似，与 ui/s3.rs chrono_now_stamp 同口径）
pub fn today_parts() -> (i32, u32, u32) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = now.div_euclid(86_400);
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
    (y as i32, m as u32, d as u32)
}

/// 本地今天的 (年, 月)
fn today_ym() -> (i32, u32) {
    let (y, m, _) = today_parts();
    (y, m)
}

/// 某年某月的天数
pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        // 闰年：能被 4 整除但不能被 100 整除，或能被 400 整除
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 30,
    }
}

/// 1 号是星期几（周一=0..周日=6）。
/// 锚点：1970-01-01 是周四 → (天数 + 3) mod 7（天数经 civil 反推）。
pub fn weekday_of_first(y: i32, m: u32) -> u32 {
    // Howard Hinnant days_from_civil
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 - 3) / 12; // m 已 ≥3，恒为 0，保留通用式
    let doy = (153 * (m as i64 - 3 + 12 * mp) + 2) / 5 + 1 - 1;
    let doe = yoe as i64 * 365 + yoe as i64 / 4 - yoe as i64 / 100 + doy;
    let days = era as i64 * 146_097 + doe - 719_468;
    // 1970-01-01 是周四：周一=0 记法下偏移 +3
    (days.rem_euclid(7) + 3) as u32 % 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_in_month_cases() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
    }

    #[test]
    fn weekday_anchor() {
        // 1970-01-01 周四 → first_wd = 3（周一=0）
        assert_eq!(weekday_of_first(1970, 1), 3);
        // 2024-01-01 周一
        assert_eq!(weekday_of_first(2024, 1), 0);
        // 2026-10-01 周四
        assert_eq!(weekday_of_first(2026, 10), 3);
    }

    #[test]
    fn cell_day_layout_oct_2026() {
        // 2026-10：1 号周四（first_wd=3），31 天 → 第 3 格是 1 号，第 33 格 31 号，
        // 第 34 格起是无效占位
        assert_eq!(cell_day(2026, 10, 3).as_deref(), Some("2026-10-01"));
        assert_eq!(cell_day(2026, 10, 33).as_deref(), Some("2026-10-31"));
        assert_eq!(cell_day(2026, 10, 34), None);
        assert_eq!(cell_day(2026, 10, 0), None);
    }
}
