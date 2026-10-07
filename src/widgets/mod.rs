//! 扩展基础组件（与业务无关的基础性 UI 组件，供 `crate::ui` 各页面复用）。

mod layout_hint;
mod sel_core;
mod select_text;
pub mod syntax_input;
#[allow(clippy::module_inception)] // widgets/widgets.rs：与业务无关的基础控件实现体
mod widgets;

pub mod calendar;

// 公共导出：部分符号当前仅被 widgets 内部/单测使用，保留导出便于扩展
#[allow(unused_imports)]
pub use calendar::{calendar_panel, days_in_month, weekday_of_first};
#[allow(unused_imports)]
pub use layout_hint::viewport_height;
#[allow(unused_imports)]
pub use sel_core::{byte_at, col_at_x, normalize_selection, row_at_y, selected_str, word_around};
#[allow(unused_imports)]
pub use select_text::SelectText;
#[allow(unused_imports)]
pub use syntax_input::{LexerKind, SyntaxInput};
pub use widgets::{
    card, input_dialog, mgr_dialog, mgr_form_col, mgr_list_col, pick_dir, pick_file, save_qr_png,
    select_text,
};
