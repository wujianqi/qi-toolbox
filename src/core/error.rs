//! 统一错误形态约定（轻量，不引入 thiserror 等依赖）。
//!
//! 项目现状与约定：core 各模块错误统一为 `Result<T, String>`，错误串是
//! 经 i18n 本地化后的用户可读文案（lang::ERR_*），直接被 UI 展示、并按
//! 「模块标签 + 内容」写入日志（见 mysql/pg/turso/sftp 各 connect 的
//! `log::warn` 先例）。
//!
//! 不引入错误枚举的原因：错误本就是面向用户的本地化文案，没有按类型
//! 分派的需求；全量改造 100+ 处签名只换来等价表达，风险大于收益。
//! 本模块固化该约定并提供构造助手：
//! - 新代码签名一律写 [`Error`]（即 `Result<T, String>`）；
//! - [`ctx`] 给错误串补模块前缀，供日志与 UI 统一观感。

/// core 模块统一错误类型（本地化文案字符串）
#[allow(dead_code)] // 约定先行：新代码逐步采用
pub type Error = String;

/// core 模块统一 Result 别名
#[allow(dead_code)] // 约定先行：新代码逐步采用
pub type Result<T> = std::result::Result<T, Error>;

/// 给错误补模块前缀：`ctx("mysql", e)` → `"[mysql] <e>"`。
/// 用于跨模块转发时保留来源；模块内首报错点不必加（connect 等已记日志）。
#[allow(dead_code)] // 约定先行：新代码逐步采用
pub fn ctx(module: &str, e: impl Into<Error>) -> Error {
    format!("[{}] {}", module, e.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_prefixes_module() {
        assert_eq!(ctx("pg", "connect failed"), "[pg] connect failed");
        // 已经带同类前缀的不重复叠加
        assert_eq!(ctx("pg", ctx("pg", "x")), "[pg] [pg] x");
    }
}
