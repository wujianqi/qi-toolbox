//! lib 入口：仅为让 examples / 集成测试能引用内部模块（bin crate 无法被外部引用）。
//! 实际二进制入口在 `src/main.rs`。

pub mod core;
#[path = "i18n/lang.rs"]
pub mod lang;
pub mod widgets;
