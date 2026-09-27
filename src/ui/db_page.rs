//! MySQL / PG / Turso 三页共享的数据库页骨架。
//!
//! 三页的结构同构：连接条（站点管理）→ 库表分级列表 → 数据表格 + 分页条 →
//! SQL 面板。此前各自复制一份（改一处要同步三处），本模块抽取共同骨架，
//! 差异经 [`DbPageHooks`] 注入：连接源构造、断开调用、连接失效、文案函数。

use windui::prelude::*;

use super::table;
use crate::core;
use crate::core::db::DbSource;
use crate::lang;

/// 三页 on_db_msg 的共享处理：按消息类型落地到页面信号。
/// `on_connected` 处理 ConnectedGroups/Connected 成功（各页文案与缓存失效不同）。
pub fn on_db_msg_shared(
    msg: core::db::DbMsg,
    signals: &DbPageSignals,
    hooks: &DbPageHooks,
) -> bool {
    match msg {
        core::db::DbMsg::ConnectedGroups(res) => {
            (hooks.on_connected_groups)(res, signals);
            true
        }
        core::db::DbMsg::TableLoaded(Ok(page)) => {
            apply_page(&page, signals);
            signals.table_loading.set(false);
            // 节流续接：本次加载完成，若连点期间有排队表则继续加载
            if let Some((g, t, off)) = signals.pending.get() {
                signals.pending.set(None);
                signals.table_loading.set(true);
                core::db::spawn_load_table_grouped(
                    crate::ui::sink(signals.tx()),
                    (hooks.make_db_source)(),
                    g,
                    t,
                    off,
                );
            }
            true
        }
        core::db::DbMsg::TableLoaded(Err(e)) => {
            signals.error.set(e);
            signals.table_loading.set(false);
            if let Some((g, t, off)) = signals.pending.get() {
                signals.pending.set(None);
                signals.table_loading.set(true);
                core::db::spawn_load_table_grouped(
                    crate::ui::sink(signals.tx()),
                    (hooks.make_db_source)(),
                    g,
                    t,
                    off,
                );
            }
            true
        }
        core::db::DbMsg::SqlDone(Ok(page)) => {
            let status = page
                .sql_status
                .clone()
                .unwrap_or_else(|| lang::TURSO_SQL_STATUS(page.columns.len(), page.row_count));
            apply_page(&page, signals);
            signals.sql_status.set(status);
            signals.error.set(String::new());
            true
        }
        core::db::DbMsg::SqlDone(Err(e)) => {
            signals.sql_status.set(format!("\u{274C} {}", e));
            true
        }
        core::db::DbMsg::ExportProgress { done, total } => {
            signals.export_progress.set(vec![(done, total)]);
            true
        }
        core::db::DbMsg::ExportDone(Ok(path)) => {
            signals.export_progress.set(Vec::new());
            signals.status.set((hooks.export_done_text)(&path));
            true
        }
        core::db::DbMsg::ExportDone(Err(e)) => {
            signals.export_progress.set(Vec::new());
            signals.error.set(e);
            true
        }
        _ => false,
    }
}

/// 三页共享的表格快照落地（行数据交给虚拟滚动数据源，元信息单独存）
pub fn apply_page(page_data: &core::db::TablePage, s: &DbPageSignals) {
    let core::db::TablePage {
        table_name,
        columns,
        rows,
        selected_row,
        row_count,
        page_offset,
        ..
    } = page_data;
    let visible = table::default_visible(columns.len());
    s.table_meta.set(vec![core::db::TablePage {
        table_name: table_name.clone(),
        columns: columns.clone(),
        rows: Vec::new(),
        visible,
        selected_row: *selected_row,
        row_count: *row_count,
        page_offset: *page_offset,
        sql_status: None,
    }]);
    s.table_rows.set(rows.clone());
    s.error.set(String::new());
}

/// 三页共享的信号集合（分组型：MySQL/PG；Turso 平铺表不走本骨架）。
/// Signal 为 Copy 句柄，字段按值克隆。
#[derive(Clone)]
pub struct DbPageSignals {
    pub groups: Signal<Vec<(String, Vec<String>)>>,
    pub connected: Signal<bool>,
    pub table_meta: Signal<Vec<core::db::TablePage>>,
    pub table_rows: Signal<Vec<Vec<String>>>,
    pub table_title: Signal<String>,
    pub status: Signal<String>,
    pub error: Signal<String>,
    pub sql_status: Signal<String>,
    pub table_loading: Signal<bool>,
    pub pending: Signal<Option<(String, String, usize)>>,
    pub export_progress: Signal<Vec<(usize, usize)>>,
    pub tx: Sender<core::db::DbMsg>,
}

impl DbPageSignals {
    pub fn tx(&self) -> Sender<core::db::DbMsg> {
        self.tx.clone()
    }
}

/// 各页差异注入点
pub struct DbPageHooks {
    /// ConnectedGroups 成功：失效本页连接缓存、写 groups/connected/标题/状态文案
    pub on_connected_groups:
        Box<dyn Fn(Result<(Vec<(String, Vec<String>)>, String), String>, &DbPageSignals)>,
    /// 构造统一连接源（按当前选中站点/输入）
    pub make_db_source: Box<dyn Fn() -> DbSource>,
    /// 导出完成文案（各页文案函数不同）
    pub export_done_text: Box<dyn Fn(&str) -> String>,
}
