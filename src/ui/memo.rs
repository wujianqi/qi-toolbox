//! 运维备忘页 UI：文本备忘的增删改（纯文本存 store.db `memos` 表）+ 月历选日期
//!
//! 页面状态封装在 [`MemoUi`]（run() 中创建，主题重建不丢状态）；
//! 数据读写直接走 [`crate::core::store`] 同步 API（本地 libSQL，毫秒级，无需后台线程）。
//! 月历为独立控件 [`crate::widgets::calendar_panel`]（信号驱动）。
//!
//! 布局：左列（上=月历 + 「全部」过滤按钮，下=备忘列表，始终同屏）；
//! 右列 = 编辑表单（布满）。点列表卡片进编辑（可改可删），列表卡片本身不放删除。

use windui::prelude::*;

use crate::core::store::{self, Memo};
use crate::lang;

/// 备忘页状态信号集合
#[derive(Clone)]
pub struct MemoUi {
    /// 全部备忘（新在前，未过滤的完整列表）
    pub list: Signal<Vec<Memo>>,
    /// 月历选中的日期（"YYYY-MM-DD"，空 = 未选 = 显示全部）
    pub day: Signal<String>,
    /// 有关联日期的备忘集合（月历绿色标记，reload 时同步）
    pub marks: Signal<Vec<String>>,
    /// 编辑中的备忘 id（None = 新建）
    pub editing: Signal<Option<i64>>,
    /// 编辑表单：标题 / 内容 / 关联日期（"YYYY-MM-DD"，空 = 不关联）
    pub title: Signal<String>,
    pub content: Signal<String>,
    /// 表单日期输入框（与月历选中双向联动）
    pub form_day: Signal<String>,
}

impl MemoUi {
    pub fn new() -> Self {
        let ui = Self {
            list: signal(store::memo_list().unwrap_or_default()),
            day: signal(String::new()),
            marks: signal(Vec::new()),
            editing: signal(None),
            title: signal(String::new()),
            content: signal(String::new()),
            form_day: signal(String::new()),
        };
        ui.sync_marks();
        ui
    }

    /// 重新从库加载列表并同步月历绿色标记
    fn reload(&self) {
        self.list.set(store::memo_list().unwrap_or_default());
        self.sync_marks();
    }

    /// 从列表提取有日期的备忘集合（月历标记）
    fn sync_marks(&self) {
        let marks: Vec<String> = self
            .list
            .get()
            .iter()
            .map(|m| m.day.clone())
            .filter(|d| !d.is_empty())
            .collect();
        self.marks.set(marks);
    }

    /// 进入「新建」空表单
    fn start_new(&self) {
        self.editing.set(None);
        self.title.set(String::new());
        self.content.set(String::new());
        self.form_day.set(String::new());
    }

    /// 进入「编辑」：表单回填选中备忘（可修改，也可在表单内删除）
    fn start_edit(&self, m: &Memo) {
        self.editing.set(Some(m.id));
        self.title.set(m.title.clone());
        self.content.set(m.content.clone());
        self.form_day.set(m.day.clone());
    }

    /// 保存表单（新建 / 更新按 editing 区分），成功后清表单并刷新列表
    fn save(&self) {
        let title = self.title.get();
        let content = self.content.get();
        let day = self.form_day.get();
        if title.trim().is_empty() && content.trim().is_empty() {
            return; // 空备忘不落库
        }
        let r = match self.editing.get() {
            Some(id) => store::memo_update(id, &title, &content, &day),
            None => store::memo_add(&title, &content, &day).map(|_| ()),
        };
        if r.is_ok() {
            super::toast::ok(lang::MEMO_SAVED());
            self.start_new();
            self.reload();
        } else if let Err(e) = r {
            super::toast::err(e);
        }
    }

    /// 删除正在编辑的备忘（按钮在表单内，列表卡片不放删除）
    fn delete_editing(&self) {
        if let Some(id) = self.editing.get() {
            if store::memo_del(id).is_ok() {
                super::toast::ok(lang::MEMO_DELETED());
                self.start_new();
                self.reload();
            }
        }
    }
}

impl Default for MemoUi {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_memo_tab(ui: &MemoUi) -> Element {
    // ── 左列：上 = 月历 + 「全部」按钮，下 = 备忘列表（始终同屏，过多可滚动）──
    // 月历点击写 ui.day 信号并触发列表刷新（on_pick 回调）；
    // 表单日期输入框与月历选中是同一信号（ui.form_day），天然双向联动
    let ui_refresh = ui.clone();
    let cal = crate::widgets::calendar_panel(ui.form_day, ui.marks, move || {
        // 点选日期后刷新过滤视图（memos 列表 host_signal 由 list 信号驱动，
        // 这里推送一个空变更触发重建）
        ui_refresh.list.update(|_| {});
    });

    let all_btn = Element::button(lang::MEMO_ALL())
        .small()
        .neutral()
        .on_click({
            // 「全部」= 清除日期选中，列表恢复全量
            let ui = ui.clone();
            move |_| {
                ui.day.set(String::new());
                ui.form_day.set(String::new());
            }
        });

    let list_panel = Element::col()
        .weight(1.0)
        .width_match()
        .spacing(6)
        .child(
            Element::row()
                .width_match()
                .cross(Align::Center)
                .child(Element::label(lang::MEMO_LIST_TITLE()).font_size(13.0).font_weight(600).fg_role(Role::Text))
                .child(Element::flex_spacer())
                .child(all_btn),
        )
        .child(memo_list_view(ui.clone()));

    let left_col = Element::col()
        .width(crate::widgets::calendar::CAL_W + 24)
        .height_match()
        .spacing(10)
        .child(cal)
        .child(list_panel);

    // ── 右列：编辑表单（weight 分配剩余宽度，避免 fill 撑出窗体）──
    let right_col = memo_editor(ui.clone()).weight(1.0);

    Element::row()
        .fill()
        .padding(20)
        .spacing(12)
        .child(left_col)
        .child(right_col)
}

/// 备忘列表：按选中日期过滤（host_signal 随选中/数据信号重建）
fn memo_list_view(ui: MemoUi) -> Element {
    let (day_v, list_v) = (ui.day, ui.list);
    Element::host_signal(list_v, move |_| {
        let day_now = day_v.get();
        let items: Vec<Memo> = if day_now.is_empty() {
            list_v.get()
        } else {
            list_v.get().into_iter().filter(|m| m.day == day_now).collect()
        };
        if items.is_empty() {
            return Element::label(if day_now.is_empty() {
                lang::MEMO_EMPTY()
            } else {
                lang::MEMO_EMPTY_DAY(&day_now)
            })
            .font_size(12.0)
            .fg_role(Role::TextMuted);
        }
        Element::scroll()
            .fill()
            .width_match()
            .child(
                Element::col()
                    .width_match()
                    .spacing(6)
                    .children(items.into_iter().map(|m| memo_card(ui.clone(), m))),
            )
    })
}

/// 编辑表单（右列，布满）：新建/编辑共用；编辑态可保存或删除
fn memo_editor(ui: MemoUi) -> Element {
    let editing_sig = ui.editing;
    let (title_in, content_in, day_in) = (ui.title, ui.content, ui.form_day);
    Element::col()
        .weight(1.0)
        .height_match()
        .bg_role(Role::Surface)
        .corner(10.0)
        .padding(16)
        .spacing(8)
        .child(
            Element::row()
                .width_match()
                .spacing(8)
                .cross(Align::Center)
                .child(
                    Element::label_signal(editing_sig.map(|e: &Option<i64>| {
                        if e.is_some() {
                            lang::MEMO_EDIT_TITLE()
                        } else {
                            lang::MEMO_NEW_TITLE()
                        }
                    }))
                    .font_size(14.0)
                    .font_weight(600)
                    .fg_role(Role::Text),
                )
                .child(Element::flex_spacer())
                // 新建入口：清空表单进入新增态
                .child(
                    Element::button(lang::MEMO_NEW())
                        .small()
                        .neutral()
                        .on_click({
                            let ui = ui.clone();
                            move |_| ui.start_new()
                        }),
                )
                // 编辑态才出现的删除按钮（列表卡片不放删除）
                .child(
                    Element::button(lang::MEMO_DELETE())
                        .small()
                        .neutral()
                        .danger()
                        .visible_when(move || editing_sig.get().is_some())
                        .on_click({
                            let ui = ui.clone();
                            move |_| ui.delete_editing()
                        }),
                ),
        )
        .child(
            Element::row()
                .width_match()
                .spacing(8)
                .child(Element::text_input(title_in, lang::MEMO_TITLE_PH()).weight(1.0))
                .child(Element::text_input(day_in, lang::MEMO_DAY()).width(140)),
        )
        .child(
            Element::text_input(content_in, lang::MEMO_CONTENT_PH())
                .multiline()
                .wrap(true)
                .font_size(13.0)
                .weight(1.0)
                .width_match(),
        )
        .child(
            Element::row()
                .width_match()
                .child(Element::flex_spacer())
                .child(
                    Element::button(lang::MEMO_SAVE())
                        .small()
                        .on_click(move |_| ui.save()),
                ),
        )
}

/// 单条备忘卡片：标题 + 摘要 + 日期；点卡片进编辑（表单可改可删）
fn memo_card(ui: MemoUi, m: Memo) -> Element {
    let ui_click = ui.clone();
    let summary: String = {
        let s: String = m
            .content
            .lines()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        let s = s.trim().to_string();
        if s.chars().count() > 60 {
            format!("{}…", s.chars().take(60).collect::<String>())
        } else {
            s
        }
    };
    let m_click = m.clone();
    Element::col()
        .width_match()
        .bg_role(Role::Bg)
        .corner(8.0)
        .padding_xy(12, 8)
        .spacing(4)
        .clickable()
        .on_click(move |_| ui_click.start_edit(&m_click))
        .child(
            Element::row()
                .width_match()
                .spacing(8)
                .cross(Align::Center)
                .child(
                    Element::label(if m.title.is_empty() {
                        summary.clone()
                    } else {
                        m.title.clone()
                    })
                    .font_size(14.0)
                    .font_weight(600)
                    .fg_role(Role::Text),
                )
                .child(Element::flex_spacer())
                .child(
                    Element::label(if m.day.is_empty() {
                        String::new()
                    } else {
                        m.day.clone()
                    })
                    .font_size(11.0)
                    .fg_role(Role::TextMuted),
                ),
        )
        .child(
            Element::label(summary)
                .font_size(12.0)
                .fg_role(Role::TextMuted),
        )
}
