//! 业务层（core）：纯逻辑，不依赖任何 UI 框架（windui 等）。
//!
//! 分层约定：
//! - `core` 不得 `use windui`（或其它界面库）；错误一律返回 `String`，由 UI 层展示。
//! - 后台线程 → UI 线程的消息通过 [`MsgSink`] 投递，具体通道实现由 UI 层注入。
//! - UI 层（`crate::ui`）只做渲染与交互，业务调用一律走本层。
//!
//! 板块：
//! - [`totp`]：TOTP 验证码 / 密钥 / 二维码数据
//! - [`password`]：随机密码生成 / 哈希
//! - [`sftp`]：SFTP/SSH 工作线程（命令/消息协议 + 传输）
//! - [`turso`]：Turso/libSQL 数据库连接与查询（含连接缓存）
//! - [`mysql`]：MySQL 数据库浏览（库→表分级列表）
//! - [`pg`]：PostgreSQL 数据库浏览（schema→表分级列表）
//! - [`db`]：数据库后台任务编排（消息协议 + 结果快照 + spawn 线程）

pub mod db;
pub mod error;
pub mod fmt;
pub mod log;
pub mod master;
pub mod mysql;
pub mod password;
pub mod pg;
pub mod qr;
pub mod redis;
pub mod remote;
pub mod s3;
pub mod settings;
pub mod sftp;
pub mod store;
pub mod totp;
pub mod turso;
pub mod update;

/// 后台线程 → UI 线程的消息投递口。
///
/// UI 层把 windui 的 channel Sender 包装成该闭包注入，业务层不感知具体通道库。
pub type MsgSink<T> = Box<dyn Fn(T) + Send + 'static>;
