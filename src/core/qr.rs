//! 通用二维码编码（业务层，不含任何 UI 代码）
//!
//! 任意文本 → RGBA 像素数据（UI 渲染源）。TOTP 密钥 URI 与远程检测端点
//! 地址的二维码共用此处编码逻辑，避免各功能各自复制一份放大染色循环。

use qrcode::QrCode;

/// 二维码 RGBA 像素数据（UI 渲染源，0 或 1 条）
#[derive(Clone)]
pub struct QrEntry {
    pub id: u64,
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// 将任意文本编码为二维码 RGBA 像素（放大 4 倍）。
/// 返回 (rgba_bytes, width, height)；编码失败返回错误文本。
pub fn qr_rgba(text: &str) -> Result<(Vec<u8>, u32, u32), String> {
    let code = QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let modules = code.to_colors();
    let total_size = modules.len();
    let module_count = (total_size as f64).sqrt() as usize;

    let scale = 4;
    let width = (module_count * scale) as u32;
    let height = (module_count * scale) as u32;

    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..module_count {
        for _sy in 0..scale {
            for x in 0..module_count {
                let idx = y * module_count + x;
                let value = if idx < total_size && modules[idx] == qrcode::Color::Dark {
                    0
                } else {
                    255
                };
                for _sx in 0..scale {
                    rgba.push(value);
                    rgba.push(value);
                    rgba.push(value);
                    rgba.push(255); // alpha
                }
            }
        }
    }

    Ok((rgba, width, height))
}

#[cfg(test)]
mod tests {
    use super::qr_rgba;

    #[test]
    fn qr_rgba_shape() {
        let (rgba, w, h) = qr_rgba("https://example.com/otp?x=1").expect("编码成功");
        assert!(w > 0 && h > 0 && w == h, "二维码应为正方形");
        assert_eq!(w % 4, 0, "放大 4 倍");
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        // alpha 通道全不透明
        assert!(rgba.iter().skip(3).step_by(4).all(|&a| a == 255));
        // 左上定位角中心应为黑色（值 0）
        let center = ((2 * 4) * w as usize + 2 * 4) * 4;
        assert_eq!(rgba[center], 0);
    }

    #[test]
    fn qr_rgba_too_long_data_errors() {
        // 超过 QR 最大容量（约 2953 字节）必须返回 Err 而非 panic
        let big = "a".repeat(4000);
        assert!(qr_rgba(&big).is_err());
    }
}
