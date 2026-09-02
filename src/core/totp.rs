//! TOTP (Time-based One-Time Password) 工具
//!
//! 用于生成兼容 Google Authenticator 的 2FA 验证码
//! 使用与 server/src/util/two_fa.rs 完全相同的算法和库

use std::time::{SystemTime, UNIX_EPOCH};
use totp_rs::{Algorithm, Secret, TOTP};

// 二维码像素数据与通用编码在 `core::qr` 实现（TOTP 与远程检测共用）；
// re-export 保持本模块 API 兼容（`totp::QrEntry` 仍可引用）
pub use crate::core::qr::QrEntry;
use crate::core::qr::qr_rgba;

/// TOTP 算法下拉索引 → Algorithm
pub fn algo_from_index(index: usize) -> Algorithm {
    match index {
        1 => Algorithm::SHA256,
        2 => Algorithm::SHA512,
        _ => Algorithm::SHA1,
    }
}

/// 读取已保存的密钥文件（不存在时返回空串）
pub fn load_saved_key() -> String {
    std::fs::read_to_string("qi_key.txt")
        .map(|k| k.trim().to_string())
        .unwrap_or_default()
}

/// 生成二维码 RGBA 数据并包装为 QrEntry（catch_unwind 防 panic 崩溃）
pub fn generate_qr_entry(
    secret_key: &str,
    account_name: &str,
    issuer: &str,
    algorithm: Algorithm,
) -> Option<QrEntry> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        generate_qr_code_data(secret_key, account_name, issuer, algorithm)
    }));
    match result {
        Ok(Ok((rgba, w, h))) => Some(QrEntry {
            id: 0,
            rgba,
            w,
            h,
        }),
        _ => None,
    }
}

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

/// 生成 TOTP 验证码并返回输出字符串
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

    // 通用 RGBA 编码（放大 4 倍）在 core::qr 中实现，与远程检测端点二维码共用
    qr_rgba(&otpauth_uri).map_err(|e| format!("Failed to generate QR code: {}", e))
}

/// 解码 Base32 密钥为字节数组（与后端相同）
fn decode_secret(secret: &str) -> Option<Vec<u8>> {
    Secret::Encoded(secret.to_string()).to_bytes().ok()
}
