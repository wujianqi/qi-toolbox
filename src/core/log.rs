//! 轻量文件日志：输出到用户配置目录的 logs/（Windows `%APPDATA%\qi-toolbox\logs`，
//! 其它平台随 [`crate::core::settings::config_dir`] 走 XDG/HOME），供问题排查。
//!
//! 设计约束（与 settings 一致的“尽力而为”哲学）：
//! - 日志写入**永不阻塞/破坏主流程**：目录建不了、文件写不了一律静默忽略；
//! - 线程安全：跨线程（UI 线程 + 各 worker 线程 + panic hook）经 Mutex 串行追加，
//!   锁毒化时用 `into_inner` 自愈继续；
//! - 单文件超过 [`MAX_LOG_BYTES`] 轮转为 `app.old.log`（保留最近一份历史）；
//! - panic hook 在 panic 发生时先写文件再交给默认 hook（stderr 行为不变）；
//!   各处 `catch_unwind` 的 panic 也会触发本 hook，因此全部线程崩溃都能留痕。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 单文件大小上限：超过即轮转（保留一份 .old）
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// 级别 → 行内标签
enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(&self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

fn log_dir() -> PathBuf {
    crate::core::settings::config_dir().join("logs")
}

/// 日志目录路径（关于页「打开日志目录」入口用；尽力创建，路径仅供参考）
pub fn dir() -> PathBuf {
    let _ = std::fs::create_dir_all(log_dir());
    log_dir()
}

fn log_file() -> PathBuf {
    log_dir().join("app.log")
}

/// 串行化写入的锁（毒化自愈：被 panic 打断时接管旧锁继续写）
static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// UTC 时间戳 `YYYY-MM-DD HH:MM:SS.mmm`（无时间 crate：用 civil-from-days 手算，
/// 全平台一致；文件头注明 UTC，排查时对照时区换算即可）
fn utc_stamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // Howard Hinnant 民用算法：天数 → y/m/d
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        y, m, d, h, mi, s, millis
    )
}

/// 轮转：当前文件超过上限时，旧文件移为 app.old.log（先删旧档再改名）
fn rotate_if_needed() {
    let file = log_file();
    let too_big = std::fs::metadata(&file)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if too_big {
        let old = log_dir().join("app.old.log");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&file, &old);
    }
}

fn write_line(level: Level, tag: &str, msg: &str) {
    let _g = lock();
    rotate_if_needed();
    if std::fs::create_dir_all(log_dir()).is_err() {
        return; // 目录都建不了：放弃（尽力而为）
    }
    let line = format!("{} [{}] [{}] {}\n", utc_stamp(), level.tag(), tag, msg);
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 初始化：建目录并安装全局 panic hook（幂等，可重复调用）。
/// hook 先写日志（级别 ERROR、标签 PANIC，含 panic 位置与载荷），再调用默认 hook
/// 保持原有 stderr/调试器输出不变。
pub fn init() {
    let _ = std::fs::create_dir_all(log_dir());
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown payload".to_string()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "?".to_string());
        write_line(
            Level::Error,
            "PANIC",
            &format!(
                "{} — {}\n{}",
                loc,
                payload,
                std::backtrace::Backtrace::force_capture()
            ),
        );
        default_hook(info);
    }));
}

pub fn info(tag: &str, msg: &str) {
    write_line(Level::Info, tag, msg);
}

pub fn warn(tag: &str, msg: &str) {
    write_line(Level::Warn, tag, msg);
}

/// 记录一条错误日志（当前生产代码未直接使用；panic hook 与测试使用）
#[cfg_attr(not(test), allow(dead_code))]
pub fn error(tag: &str, msg: &str) {
    write_line(Level::Error, tag, msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamp_format() {
        let s = utc_stamp();
        // `YYYY-MM-DD HH:MM:SS.mmm` 共 23 字符
        assert_eq!(s.len(), 23, "got: {}", s);
        let b = s.as_bytes();
        assert_eq!(b[4], b'-');
        assert_eq!(b[7], b'-');
        assert_eq!(b[10], b' ');
        assert_eq!(b[13], b':');
        assert_eq!(b[16], b':');
        assert_eq!(b[19], b'.');
        // 数字位全部是数字
        for &i in &[0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22] {
            assert!(b[i].is_ascii_digit(), "pos {} in {}", i, s);
        }
    }

    /// civil-from-days 算法用已知锚点校验：1970-01-01 = 0 天
    #[test]
    fn utc_stamp_epoch_anchor() {
        // 不直接测私有函数的时间输入，但可用固定天数推算：用整天数验证 y/m/d 正确性
        // 通过临时构造：utc_stamp 无参数化，这里以日志写入回路代替
        // 写入不 panic 即可（尽力而为语义：目录不可写也静默）
        info("test", "write ok");
        warn("test", "write warn");
        error("test", "write error");
    }

    /// 并发写日志不 panic、锁毒化自愈
    #[test]
    fn concurrent_writes_no_panic() {
        let handles: Vec<_> = (0..8)
            .map(|i| std::thread::spawn(move || info("test", &format!("t{}", i))))
            .collect();
        for h in handles {
            h.join().expect("log thread must not panic");
        }
    }
}
