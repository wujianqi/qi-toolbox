//! 布局测量共享辅助（纯函数，无状态）
//!
//! windui 布局引擎对**无约束测量**（auto 高，如 scroll 内的 wrap 子项）传入的
//! `avail` 是哨兵值 `i32::MAX / 4`（见 windui `spec.rs`：`MeasureMode::Unbounded =>
//! i32::MAX / 4`），并非真实可用空间。需要"视口模型"（有具体高度就填满分配区）
//! 的自绘控件必须先识别并排除哨兵值，否则会把控件量成 5 亿高，父级布局
//! `used_main += …` 累加溢出直接 panic（release 下 panic=abort 崩溃）。

/// 父级给定的高度是否为**真实可用空间**。
///
/// `false` 的两种情形都应回退固有高度：
/// - `avail_h <= 0`：无约束/未指定
/// - `avail_h` 达到无约束哨兵值 `i32::MAX / 4`：windui 的 "Unbounded" 标记
#[inline]
pub fn has_real_height(avail_h: i32) -> bool {
    avail_h > 0 && avail_h < i32::MAX / 4
}

/// 视口模型测高：父级给真实高度就**填满分配区**（不随内容缩，保证短文本也能
/// 点到控件、可聚焦）；无约束（含哨兵值）时回退固有高度。
#[inline]
pub fn viewport_height(avail_h: i32, intrinsic: i32, min: i32) -> i32 {
    if has_real_height(avail_h) {
        avail_h.max(min)
    } else {
        intrinsic.max(min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实高度 → 填满分配区。
    #[test]
    fn viewport_fills_real_height() {
        assert_eq!(viewport_height(500, 100, 20), 500);
        assert_eq!(
            viewport_height(1, 100, 20),
            20,
            "真实高度但不达 min → 取 min"
        );
        assert!(has_real_height(1));
        assert!(!has_real_height(0));
        assert!(!has_real_height(-5));
    }

    /// 无约束 / 哨兵值 → 回退固有高度（不低于 min）。
    #[test]
    fn viewport_falls_back_on_unbounded() {
        assert_eq!(viewport_height(0, 100, 20), 100, "0 = 无约束");
        assert_eq!(viewport_height(-5, 100, 20), 100, "负值 = 无效");
        assert_eq!(
            viewport_height(i32::MAX / 4, 100, 20),
            100,
            "windui Unbounded 哨兵值必须被识别，否则布局溢出 panic"
        );
        // 恰好低于哨兵仍算真实高度（边界值本身不算）。
        assert_eq!(viewport_height(i32::MAX / 4 - 1, 100, 20), i32::MAX / 4 - 1);
        // 回退时固有高度也受 min 钳制。
        assert_eq!(viewport_height(0, 5, 20), 20);
    }

    /// 固有高度为哨兵/负值（异常输入）不 panic；min 只做下限钳制，不改大值。
    #[test]
    fn viewport_sane_on_weird_intrinsic() {
        assert_eq!(viewport_height(0, i32::MAX / 4, 20), i32::MAX / 4);
        assert_eq!(viewport_height(0, -100, 20), 20);
    }
}
