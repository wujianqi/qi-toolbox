//! 表格渲染：左侧表列表 + 右侧数据表格（Turso 页专用，纯 UI 层）
//!
//! Turso 页表格区采用数据驱动：`core::db::TablePage` 查询结果快照进
//! `Signal<Vec<TablePage>>`（`host_signal` 数据源），列/行变化时整块重建。
//! 左侧表列表用 `list_signal` 渲染，选中行通过派生状态高亮。
//!
//! 数据库操作一律在后台线程执行（见 `core::db::spawn_*`），本文件只在
//! 点击时把 windui 的 Sender 包装成业务层投递口（`super::sink`）发起任务。

use windui::prelude::*;

use super::icons;
use super::select_text;
use super::sink_opt;
use crate::core;
use crate::core::turso::TursoSource;
use crate::lang;

/// 默认在列表视图显示的列数（其余列默认隐藏，可在列设置面板勾选显示）
pub const DEFAULT_VISIBLE_COLS: usize = 8;

/// 默认可见列：前 `DEFAULT_VISIBLE_COLS` 列显示，其余隐藏
pub fn default_visible(len: usize) -> Vec<bool> {
    (0..len).map(|i| i < DEFAULT_VISIBLE_COLS).collect()
}

/// 详情视图单元格文本的信号池：按索引复用 thread_local 槽位，永不随重建回收。
/// 不能在 `build_table_view` 里 `signal(cell)` 现建——主题切换整树重建会 dispose
/// 构建期信号，`SelectText` 持有的句柄即成死句柄，再读即 panic（signal.rs:545）。
/// 每次构建详情时按位置取池中信号并刷新值；池按需扩容，缩容不回收（槽位廉价）。
fn cell_text_sig(i: usize, value: &str) -> Signal<String> {
    use std::cell::RefCell;
    thread_local! {
        static POOL: RefCell<Vec<Signal<String>>> = const { RefCell::new(Vec::new()) };
    }
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        // 自愈：首建若发生在整树重建的 SignalScope 内会被收走（is_alive=false），
        // 此时换新槽位重建，避免残留死句柄（读取即 panic）。调用方随后回填值。
        if i >= p.len() {
            p.resize_with(i + 1, || signal(String::new()));
        }
        if !p[i].is_alive() {
            p[i] = signal(String::new());
        }
        let s = p[i];
        s.set(value.to_string());
        s
    })
}

/// 列表视图单元格展示上限（字符）：超长 TEXT 在**渲染层**截断，避免 label 的
/// `truncate` 对全串做 O(N) 文本测量拖慢虚拟列表滚动重建。
/// 业务层存完整值（`core::turso` 不截断），详情视图读全文，不受此限。
const CELL_DISPLAY_MAX: usize = 256;

/// 列表单元格展示文本：超出 [`CELL_DISPLAY_MAX`] 时截断（借用切片，零分配；
/// 宽度溢出仍由 label 的 `truncate` 按列收尾）。
fn cell_display(cell: &str) -> &str {
    match cell.char_indices().nth(CELL_DISPLAY_MAX) {
        Some((idx, _)) => &cell[..idx],
        None => cell,
    }
}

/// 数据表行高：windui 的 `TABLE_ROW_H` 按纯文本行定（约 21px），装不下 26px 的
/// 「查看」按钮，故本地抬高行高；表头/正文统一用它保证对齐（height/width 均为 i32）。
pub const ROW_H: i32 = 32;

/// 左侧表列表（list_signal 数据驱动，点击异步加载表数据）
/// `loading`/`pending`：节流信号，连点表名时同一时间只允许一个后台加载
pub fn render_table_list(
    tables: Signal<Vec<String>>,
    selected: Signal<Option<String>>,
    make_source: impl Fn() -> Option<TursoSource> + 'static,
    tx: Option<Sender<core::db::DbMsg>>,
    loading: Signal<bool>,
    pending: Signal<Option<(String, usize)>>,
) -> Element {
    // row_fn 是 Fn（可多次调用），make_source 非 Clone（impl Fn），Rc 包装供内层点击回调共享
    let make_source = std::rc::Rc::new(make_source);
    Element::virtual_list(tables, 30, move |_idx, name: String| {
        let is_sel = selected.get().as_deref() == Some(name.as_str());
        // 官方 virtual_list 的 row_fn 是 Fn（可多次调用），先克隆 tx 供内层 move 闭包捕获
        let tx = tx.clone();
        let make_source = std::rc::Rc::clone(&make_source);
        // 输入法候选窗风格：选中行 = 左侧强调条 + 浅色底；悬停由 clickable 提供淡层
        let tint = if is_sel {
            Role::Accent.resolve(&windui::theme::current())
        } else {
            Role::TextMuted.resolve(&windui::theme::current())
        };
        let mut row = Element::row()
            .width_match()
            .height(30)
            .cross(Align::Center)
            .spacing(8)
            .padding_xy(8, 0)
            .child(
                Element::leaf()
                    .width(3)
                    .height(20)
                    .bg_role_alpha(Role::Accent, if is_sel { 0.9 } else { 0.0 })
                    .corner(1.5),
            )
            .child(
                // 表名图标：选中随强调色亮起，未选中为弱化灰
                Element::image_content(
                    ImageContent::from_svg_bytes(icons::TABLE_ICON, None).tint(tint),
                )
                // 矢量源固有 24dp：钉回原 Some(15) 的逻辑尺寸
                .width(15)
                .height(15)
                .align(Align::Center),
            )
            .child(
                Element::label(&name)
                    .font_size(13.0)
                    .font_weight(if is_sel { 600 } else { 400 })
                    .fg_role(if is_sel { Role::Text } else { Role::TextMuted })
                    .weight(1.0)
                    // 不 truncate：max_lines(1) 的 clip 裁剪即可防溢出，
                    // 避免 truncate 的 O(N) 逐字符测量（虚拟列表重建时每实例缓存失效）
                    .max_lines(1),
            );
        if is_sel {
            row = row.bg_role_alpha(Role::Accent, 0.12).corner(4.0);
        }
        row.clickable().on_click(move |_| {
            if is_sel {
                return;
            }
            selected.set(Some(name.clone()));
            // 节流：已在加载则只记录排队表，不并发开新连接（连点不卡死）
            if loading.get() {
                pending.set(Some((name.clone(), 0)));
                return;
            }
            loading.set(true);
            // 异步加载表数据（第 1 页），结果经 channel 回传 UI 线程
            if let Some(src) = make_source() {
                core::db::spawn_load_table(sink_opt(tx.clone()), src, name.clone(), 0);
            }
            // 强制触发虚拟列表重建，刷新选中高亮
            tables.set(tables.get());
        })
    })
    .bg_role(Role::SurfaceAlt)
    .corner(6.0)
}

/// 右侧数据表格（元信息驱动视图切换：表头 + 行列表 + 详情视图；整表行由官方虚拟滚动表格渲染）
pub fn render_data_table(
    meta: Signal<Vec<core::db::TablePage>>,
    rows: Signal<Vec<Vec<String>>>,
) -> Element {
    Element::host_signal(meta, move |t: core::db::TablePage| {
        build_table_view(&t, meta, rows)
    })
}

/// 分级表列表的摊平行模型（供虚拟列表逐行渲染）
#[derive(Clone)]
enum GroupedRow {
    /// 组头（库/schema）：名称 + 表数 + 是否展开
    Group {
        name: String,
        count: usize,
        open: bool,
    },
    /// 表行：所属组 + 表名（key = "组.表" 用于高亮）
    Table {
        group: String,
        table: String,
        key: String,
    },
}

/// 分级表列表（MySQL/PG 页专用）：两级结构——第一级为组（库/schema，点击展开/收起），
/// 第二级为表（点击加载数据，节流防连点）。`selected_key` = "组.表"（用于高亮）。
/// 展开状态收在 `expanded` 信号（HashSet 语义：空串集合 = 全部收起）。
///
/// 性能：摊平成行模型交给 `virtual_list` 只渲染视口内行——数百表也不卡顿
/// （原 host_signal 为每张表构建真实子树，任何状态变化都整列重建）。
pub fn render_grouped_table_list(
    groups: Signal<Vec<(String, Vec<String>)>>,
    selected_key: Signal<String>,
    expanded: Signal<Vec<String>>,
    on_table_click: impl Fn(String, String) + Clone + 'static,
) -> Element {
    let on_click = std::rc::Rc::new(on_table_click);
    // 摊平：组头行 + 展开组的表行。派生信号随 groups 重算；expanded 变化后
    // 由点击回调 groups.set(get()) 触发重算（map 只注册 groups 一个依赖）。
    let flat = groups.map(move |gs: &Vec<(String, Vec<String>)>| {
        let open = expanded.get();
        let mut rows = Vec::new();
        for (g, ts) in gs {
            let is_open = open.iter().any(|e| e == g);
            rows.push(GroupedRow::Group {
                name: g.clone(),
                count: ts.len(),
                open: is_open,
            });
            if is_open {
                for t in ts {
                    rows.push(GroupedRow::Table {
                        group: g.clone(),
                        table: t.clone(),
                        key: format!("{}.{}", g, t),
                    });
                }
            }
        }
        rows
    });

    Element::virtual_list(flat, 28, move |_idx, row: GroupedRow| match row {
        GroupedRow::Group { name, count, open } => {
            let (exp, groups_sig) = (expanded, groups);
            let group_click = name.clone();
            Element::row()
                .width_match()
                .height(28)
                .cross(Align::Center)
                .spacing(6)
                .padding_xy(8, 0)
                .clickable()
                .on_click(move |_| {
                    exp.update(|v| {
                        if let Some(pos) = v.iter().position(|e| e == &group_click) {
                            v.remove(pos);
                        } else {
                            v.push(group_click.clone());
                        }
                    });
                    // 触发摊平重算 + 虚拟列表重建（expanded 不在派生依赖里）
                    groups_sig.set(groups_sig.get());
                })
                .child(
                    Element::label(if open { "\u{25BE}" } else { "\u{25B8}" })
                        .font_size(11.0)
                        .fg_role(Role::TextMuted)
                        .width(14),
                )
                .child(
                    // 库/schema 组头图标（数据库圆柱体）
                    Element::image_content(
                        ImageContent::from_svg_bytes(icons::DATABASE, None)
                            .tint(Role::Accent.resolve(&windui::theme::current())),
                    )
                    // 矢量源固有 24dp：钉回原 Some(14) 的逻辑尺寸
                    .width(14)
                    .height(14)
                    .align(Align::Center),
                )
                .child(
                    Element::label(name)
                        .font_size(13.0)
                        .font_weight(600)
                        .fg_role(Role::Text)
                        .weight(1.0)
                        .max_lines(1),
                )
                .child(
                    Element::label(format!("{}", count))
                        .font_size(11.0)
                        .fg_role(Role::TextMuted),
                )
        }
        GroupedRow::Table { group, table, key } => {
            let is_sel = selected_key.get() == key;
            let tint = if is_sel {
                Role::Accent.resolve(&windui::theme::current())
            } else {
                Role::TextMuted.resolve(&windui::theme::current())
            };
            let mut row = Element::row()
                .width_match()
                .height(28)
                .cross(Align::Center)
                .spacing(8)
                .padding_xy(24, 0) // 二级缩进
                .child(
                    Element::leaf()
                        .width(3)
                        .height(18)
                        .bg_role_alpha(Role::Accent, if is_sel { 0.9 } else { 0.0 })
                        .corner(1.5),
                )
                .child(
                    Element::image_content(
                        ImageContent::from_svg_bytes(icons::TABLE_ICON, None).tint(tint),
                    )
                    // 矢量源固有 24dp：钉回原 Some(14) 的逻辑尺寸
                    .width(14)
                    .height(14)
                    .align(Align::Center),
                )
                .child(
                    Element::label(table.clone())
                        .font_size(13.0)
                        .font_weight(if is_sel { 600 } else { 400 })
                        .fg_role(if is_sel { Role::Text } else { Role::TextMuted })
                        .weight(1.0)
                        .max_lines(1),
                );
            if is_sel {
                row = row.bg_role_alpha(Role::Accent, 0.12).corner(4.0);
            }
            let on_click = std::rc::Rc::clone(&on_click);
            let groups_sig = groups;
            row.clickable().on_click(move |_| {
                on_click(group.clone(), table.clone());
                // 刷新选中高亮（selected_key 不在派生依赖里）
                groups_sig.set(groups_sig.get());
            })
        }
    })
}

/// 构建单个表格视图（表头信息行 + 列表/详情二选一）
fn build_table_view(
    t: &core::db::TablePage,
    meta: Signal<Vec<core::db::TablePage>>,
    rows: Signal<Vec<Vec<String>>>,
) -> Element {
    if t.columns.is_empty() {
        // 空态：居中表格图标 + 弱化提示，比单行文字更有“未加载”的分区感
        return Element::col()
            .fill()
            .cross(Align::Center)
            .spacing(10)
            .child(
                Element::image_content(
                    ImageContent::from_svg_bytes(icons::TABLE_ICON, None)
                        .tint(Role::TextMuted.resolve(&windui::theme::current())),
                )
                // 矢量源固有 24dp：钉回原 Some(34) 的逻辑尺寸
                .width(34)
                .height(34)
                .align(Align::Center),
            )
            .child(
                Element::label(lang::DT_NO_TABLE())
                    .font_size(13.0)
                    .fg_role(Role::TextMuted),
            );
    }

    // 表头信息行：表名 + 列/行数
    let table_name = t.table_name.clone().unwrap_or_default();
    let mut info_row = Element::row()
        .width_match()
        .height(30)
        .cross(Align::Center)
        .spacing(10)
        .child(
            // 数据视图标题旁的表格小图标，弱化灰不抢标题焦点
            Element::image_content(
                ImageContent::from_svg_bytes(icons::TABLE_ICON, None)
                    .tint(Role::TextMuted.resolve(&windui::theme::current())),
            )
            // 矢量源固有 24dp：钉回原 Some(15) 的逻辑尺寸
            .width(15)
            .height(15)
            .align(Align::Center),
        )
        .child(
            Element::label(table_name)
                .font_size(13.0)
                .font_weight(700)
                .fg_role(Role::Text),
        )
        .child(
            Element::label(lang::DT_COL_ROW(t.columns.len(), t.row_count))
                .font_size(11.0)
                .fg_role(Role::TextMuted),
        );
    // 隐藏列提示：其余列在列设置面板中勾选显示
    let hidden = t
        .columns
        .iter()
        .enumerate()
        .filter(|(i, _)| !t.visible.get(*i).copied().unwrap_or(true))
        .count();
    if hidden > 0 {
        info_row = info_row.child(
            Element::label(lang::DT_COL_MORE(hidden))
                .font_size(11.0)
                .fg_role(Role::TextMuted),
        );
    }

    // ── 详情视图 ──
    if let Some(sel) = t.selected_row {
        let all = rows.get();
        if sel < all.len() {
            let row_data = &all[sel];
            let mut detail = Element::col().spacing(4);
            let back = Element::button(lang::DT_BACK())
                .icon_content(icons::stateful_icon(icons::ARROW_LEFT))
                .small()
                .outline()
                .neutral()
                .on_click(move |_| {
                    // update 原地修改，避免 get→set 克隆整表
                    meta.update(|v| {
                        if let Some(p) = v.first_mut() {
                            p.selected_row = None;
                        }
                    });
                });
            detail = detail.child(
                Element::row()
                    .spacing(8)
                    .cross(Align::Center)
                    .child(back)
                    .child(
                        Element::label(lang::DT_ROW_DETAIL(sel + 1))
                            .font_size(13.0)
                            .font_weight(700),
                    ),
            );
            for (col_i, col_name) in t.columns.iter().enumerate() {
                // 防御：行数据可能短于列名列表，越界会 panic（release 下 panic=abort 直接崩溃）
                let cell = row_data.get(col_i).map(String::as_str).unwrap_or("");
                detail = detail.child(
                    Element::row()
                        .spacing(8)
                        .child(
                            Element::label(format!("{}:", col_name))
                                .font_size(12.0)
                                .font_weight(600)
                                .width(180),
                        )
                        .child(
                            // 详情值用只读可选文本承载：高度不写死，随内容行数上下浮动
                            // （SelectText 未约束高度时按内容返回固有高度），可选中复制
                            // 信号从 thread_local 池按索引复用并就地刷新值，
                            // 不在构建期新建（防主题切换整树回收后成死句柄）
                            select_text(cell_text_sig(col_i, cell))
                                .width_match()
                                .weight(1.0),
                        ),
                );
            }
            return Element::col()
                .width_match()
                .spacing(6)
                .child(info_row)
                .child(Element::scroll().width_match().weight(1.0).child(detail));
        }
    }

    // ── 列表视图：自建表头（单行裁切防溢出）+ 官方虚拟滚动正文 ──
    // 官方 table_header 的 label 无 max_lines，长列标题在窄列下换行会溢出到正文（表头文字
    // 与第一行数据重叠），故表头自建并强制单行裁切；正文用官方 `Element::virtual_list`
    // （只构建视口内 ±overscan 的行，10 万行也恒定节点数）。
    //
    // 列显示由 `t.visible` 控制（列设置面板勾选）：隐藏列不构建；可见列权重按
    // 「列名 + 前 200 行内容」最大字符数估算——长字段列宽、短字段列窄，避免列多时
    // 全部被裁成省略号。借用只读前 200 行，不克隆整表。
    let head_lens = rows.with(|v| {
        let mut acc = vec![0usize; t.columns.len()];
        for row in v.iter().take(200) {
            for (i, s) in row.iter().take(t.columns.len()).enumerate() {
                // 只统计展示上限内的字符：超出即视为「长列」，避免超长单元格
                // 拖慢列宽估算
                acc[i] = acc[i].max(s.chars().take(CELL_DISPLAY_MAX).count());
            }
        }
        acc
    });
    let visible_idx: Vec<usize> = (0..t.columns.len())
        .filter(|&i| t.visible.get(i).copied().unwrap_or(true))
        .collect();
    let weights: Vec<f32> = visible_idx
        .iter()
        .map(|&i| {
            let longest = t.columns[i]
                .chars()
                .count()
                .max(head_lens.get(i).copied().unwrap_or(0));
            // 权重下限 1.0 防零宽列，上限 12 防单列霸屏
            (1.0f32 + longest as f32 * 0.6).min(12.0)
        })
        .collect();

    // 表头：与正文行高一致，列标题单行裁切（不换行、不溢出）
    let mut header = Element::row()
        .width_match()
        .height(ROW_H)
        .cross(Align::Center)
        .bg_role(Role::SurfaceAlt)
        // 左侧预留「查看」按钮列（与正文按钮同宽），保证表头与数据列对齐
        .child(Element::leaf().width(26));
    for (k, &i) in visible_idx.iter().enumerate() {
        header = header.child(
            Element::label(&t.columns[i])
                .font_size(13.0)
                .font_weight(600)
                .fg_role(Role::TextMuted)
                .max_lines(1)
                .truncate(Truncate::End)
                .weight(weights[k]),
        );
    }

    // 正文：官方虚拟滚动列表，每行 = 查看按钮 + 可见列单元格（单行裁切）
    let body = Element::virtual_list(rows, ROW_H, move |idx, row: Vec<String>| {
        let mut r = Element::row()
            .width_match()
            .height(ROW_H)
            .cross(Align::Center);
        // 斑马纹区分行；行内按钮自带 hover 反馈，避免整行 clickable 的补间重绘
        if idx % 2 == 1 {
            r = r.bg_role(Role::SurfaceAlt);
        }
        let ri = idx;
        // 「查看」按钮固定最左列：列多了右侧会被裁掉，放左侧始终可见
        r = r.child(
            Element::icon_button_content(
                // ImageContent 非 Clone，行构建期现构造（虚拟列表仅构建可见行）
                icons::stateful_icon(icons::SEARCH),
            )
            .size(26, 26)
            .on_click(move |_| {
                meta.update(|v| {
                    if let Some(p) = v.first_mut() {
                        p.selected_row = Some(ri);
                    }
                });
            }),
        );
        for (k, &i) in visible_idx.iter().enumerate() {
            let cell = row.get(i).map(String::as_str).unwrap_or("");
            let is_null = cell == "NULL";
            r = r.child(
                Element::label(cell_display(cell))
                    .font_size(13.0)
                    .max_lines(1)
                    .truncate(Truncate::End)
                    .weight(weights[k])
                    .fg_role(if is_null { Role::TextMuted } else { Role::Text }),
            );
        }
        r
    });

    Element::col()
        .width_match()
        .child(header)
        .child(
            Element::leaf()
                .width_match()
                .height(1)
                .bg_role(Role::Divider),
        )
        .child(body.weight(1.0))
}
