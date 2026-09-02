//! 数据库后台任务编排（业务层，不含任何 UI 代码）
//!
//! Turso 页的后台线程协议：`DbMsg` 定义后台线程 → UI 线程的消息，
//! `TablePage` 是查询结果的纯数据快照；`spawn_*` 在独立线程中执行
//! 连接/加载表/SQL，结果经 [`MsgSink`] 投递回 UI 线程（由 UI 层注入实现）。

use crate::core::MsgSink;
use crate::core::turso::{TursoSource, TursoViewer};
use crate::lang;

/// 后台数据库操作结果（经 channel 回传 UI 线程）
pub enum DbMsg {
    /// 连接结果：(表列表, 显示文件名)
    Connected(Result<(Vec<String>, String), String>),
    /// 表数据加载（点击表名/刷新）结果
    TableLoaded(Result<TablePage, String>),
    /// SQL 执行结果
    SqlDone(Result<TablePage, String>),
}

/// 表格数据快照：整表数据（全量行，虚拟滚动按需渲染）+ 元信息
#[derive(Clone)]
pub struct TablePage {
    /// 当前表名；None = SQL 查询结果
    pub table_name: Option<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// 各列是否在列表视图显示（列设置面板勾选）
    pub visible: Vec<bool>,
    /// 详情视图行号
    pub selected_row: Option<usize>,
    pub row_count: usize,
    /// 当前分页偏移（0 起，每页 PAGE_SIZE 行）
    pub page_offset: usize,
}

impl TablePage {
    pub fn empty() -> Self {
        Self {
            table_name: None,
            columns: Vec::new(),
            rows: Vec::new(),
            visible: Vec::new(),
            selected_row: None,
            row_count: 0,
            page_offset: 0,
        }
    }
}

impl Default for TablePage {
    fn default() -> Self {
        Self::empty()
    }
}

/// 从 viewer 快照表格状态
fn snapshot(viewer: &TursoViewer) -> TablePage {
    TablePage {
        table_name: viewer.selected_table.clone(),
        columns: viewer.column_names.clone(),
        rows: viewer.table_data.clone(),
        visible: vec![true; viewer.column_names.len()],
        selected_row: None,
        row_count: viewer.row_count,
        page_offset: viewer.page_offset,
    }
}

/// 后台线程：连接数据库，结果经 sink 回传
pub fn spawn_connect(sink: MsgSink<DbMsg>, source: TursoSource) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            // 展示名：本地=文件名，远程=URL
            let fname = match &source {
                TursoSource::Local(p) => std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.clone()),
                TursoSource::Remote { url, .. } => url.clone(),
            };
            let mut viewer = TursoViewer {
                source,
                ..Default::default()
            };
            viewer.connect()?;
            Ok::<_, String>((viewer.tables.clone(), fname))
        }));
        let msg = match result {
            Ok(Ok(v)) => DbMsg::Connected(Ok(v)),
            Ok(Err(e)) => DbMsg::Connected(Err(e)),
            Err(_) => DbMsg::Connected(Err(lang::TURSO_UNKNOWN_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：加载表数据（点击表名/刷新/翻页），结果经 sink 回传
pub fn spawn_load_table(sink: MsgSink<DbMsg>, source: TursoSource, table: String, offset: usize) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut viewer = TursoViewer {
                source,
                ..Default::default()
            };
            viewer.set_selected_table(table, offset)?;
            Ok::<_, String>(snapshot(&viewer))
        }));
        let msg = match result {
            Ok(Ok(page)) => DbMsg::TableLoaded(Ok(page)),
            Ok(Err(e)) => DbMsg::TableLoaded(Err(e)),
            Err(_) => DbMsg::TableLoaded(Err(lang::TURSO_UNKNOWN_PANIC().to_string())),
        };
        sink(msg);
    });
}

/// 后台线程：执行 SQL，结果经 sink 回传
pub fn spawn_execute_sql(sink: MsgSink<DbMsg>, source: TursoSource, sql: String) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut viewer = TursoViewer {
                source,
                ..Default::default()
            };
            viewer.execute_sql(&sql)?;
            Ok::<_, String>(snapshot(&viewer))
        }));
        let msg = match result {
            Ok(Ok(page)) => DbMsg::SqlDone(Ok(page)),
            Ok(Err(e)) => DbMsg::SqlDone(Err(e)),
            Err(_) => DbMsg::SqlDone(Err(lang::TURSO_UNKNOWN_PANIC().to_string())),
        };
        sink(msg);
    });
}
