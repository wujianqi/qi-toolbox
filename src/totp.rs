//! TOTP (Time-based One-Time Password) 工具
//!
//! 用于生成兼容 Google Authenticator 的 2FA 验证码
//! 使用与 server/src/util/two_fa.rs 完全相同的算法和库

use std::time::{SystemTime, UNIX_EPOCH};
use totp_rs::{Algorithm, Secret, TOTP};
use qrcode::QrCode;

/// 生成随机的 Base32 编码密钥（20字节 = 160位）
pub fn generate_secret_key() -> String {
    Secret::generate_secret().to_encoded().to_string()
}

/// 根据密钥、算法和时间步长生成 TOTP 验证码
pub fn generate_totp_with_time_step(secret: &str, algorithm: Algorithm, time_step: u64) -> Result<String, String> {
    let key_bytes = decode_secret(secret)
        .ok_or_else(|| "Invalid Base32 secret key".to_string())?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("SystemTime error: {}", e))?
        .as_secs();
    // 注意：generate() 内部已处理 step 除法，这里直接传原始时间戳
    let time = timestamp.saturating_add(time_step);

    let totp = TOTP::new_unchecked(algorithm, 6, 1, 30, key_bytes, None, String::new());

    Ok(totp.generate(time))
}

/// 生成 TOTP 验证码（使用当前时间）
pub fn generate_totp(secret_key: &str, algorithm: Algorithm) -> Result<String, String> {
    generate_totp_with_time_step(secret_key, algorithm, 0)
}

/// 运行 TOTP 工具并返回输出字符串
pub fn run(secret_key: &str, algorithm: Algorithm) -> String {
    match generate_totp(secret_key, algorithm) {
        Ok(code) => code,
        Err(e) => e,
    }
}

/// 为给定的密钥生成二维码 RGBA 数据（用于2FA配置）
/// 返回 (rgba_bytes, width, height)
pub fn generate_qr_code_data(secret_key: &str, account_name: &str, issuer: &str, algorithm: Algorithm) -> Result<(Vec<u8>, u32, u32), String> {
    let key_bytes = decode_secret(secret_key)
        .ok_or_else(|| "Invalid Base32 secret key".to_string())?;

    let totp = TOTP::new_unchecked(
        algorithm,
        6,
        1,
        30,
        key_bytes,
        Some(issuer.to_string()),
        account_name.to_string(),
    );

    let otpauth_uri = totp.get_url();

    let code = QrCode::new(otpauth_uri.as_bytes())
        .map_err(|e| format!("Failed to generate QR code: {}", e))?;

    let modules = code.to_colors();
    let total_size = modules.len();
    let module_count = (total_size as f64).sqrt() as usize;

    let scale = 4;
    let width = (module_count * scale) as u32;
    let height = (module_count * scale) as u32;

    let mut rgba = Vec::with_capacity((width as usize * height as usize * 4) as usize);
    for y in 0..module_count {
        for _sy in 0..scale {
            for x in 0..module_count {
                let idx = y * module_count + x;
                let value = if idx < total_size && modules[idx] == qrcode::Color::Dark { 0 } else { 255 };
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

/// 解码 Base32 密钥为字节数组（与后端相同）
fn decode_secret(secret: &str) -> Option<Vec<u8>> {
    Secret::Encoded(secret.to_string()).to_bytes().ok()
}
