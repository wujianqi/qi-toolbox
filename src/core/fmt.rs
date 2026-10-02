//! 通用格式化工具（UI 与 core 共用）。

use std::time::{SystemTime, UNIX_EPOCH};

/// 字节数人性化显示（B / KB / MB / GB，KB 起保留 1 位小数）。
pub fn format_size(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let v = n as f64;
    if v >= GB {
        format!("{:.1} GB", v / GB)
    } else if v >= MB {
        format!("{:.1} MB", v / MB)
    } else if v >= KB {
        format!("{:.1} KB", v / KB)
    } else {
        format!("{} B", n)
    }
}

/// Unix 秒 → 本地日期时间 `MM-DD HH:MM`（当年显示月日，往年补年份；
/// 无时间 crate：civil-from-days 手算，按东八区近似折算，与日志时间戳同一套路）。
pub fn format_date(unix_secs: i64) -> String {
    let local = unix_secs + 8 * 3600;
    let days = local.div_euclid(86_400);
    let rem = local.rem_euclid(86_400);
    // Howard Hinnant 民用算法：天数 → y/m/d
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (h, mi) = (rem / 3600, (rem % 3600) / 60);
    // 当年省略年份（列表列宽紧张；跨年文件补全）
    if y == chrono_now_year() {
        format!("{:02}-{:02} {:02}:{:02}", m, d, h, mi)
    } else {
        format!("{:04}-{:02}-{:02}", y, m, d)
    }
}

/// 当前年份（本地，东八区近似）
fn chrono_now_year() -> i64 {
    let local = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 + 8 * 3600)
        .unwrap_or(0);
    let days = local.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y + 1
    } else {
        y
    }
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(204800), "200.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
