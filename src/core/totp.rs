//! TOTP (Time-based One-Time Password) 工具
//!
//! 用于生成兼容 Google Authenticator 的 2FA 验证码
//! 使用与 server/src/util/two_fa.rs 完全相同的算法和库

use std::time::{SystemTime, UNIX_EPOCH};
use totp_rs::{Algorithm, Secret, TOTP};

// 二维码像素数据与通用编码在 `core::qr` 实现（TOTP 与远程检测共用）；
// re-export 保持本模块 API 兼容（`totp::QrEntry` 仍可引用）
use crate::core::qr::qr_rgba;
pub use crate::core::qr::QrEntry;

/// TOTP 算法下拉索引 → Algorithm
pub fn algo_from_index(index: usize) -> Algorithm {
    match index {
        1 => Algorithm::SHA256,
        2 => Algorithm::SHA512,
        _ => Algorithm::SHA1,
    }
}

/// 读取当前使用的密钥（存于本地 store 数据库 kv 表，敏感键加密存储；
/// 经 settings::load 解密回原文，无记忆/解密失败返回空串）
pub fn load_saved_key() -> String {
    crate::core::settings::load()
        .get("totp.key")
        .cloned()
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
        Ok(Ok((rgba, w, h))) => Some(QrEntry { id: 0, rgba, w, h }),
        _ => None,
    }
}

/// 生成随机的 Base32 编码密钥。
/// `bits`：期望密钥位数（16 位 = 10 字节 80bit，32 位 = 20 字节 160bit）；
/// 其他取值按就近有效长度处理（<16 → 16，其余 → 32）。
/// Base32 每字节 8/5 字符，编码后长度为 16 / 32 字符（无填充）。
pub fn generate_secret_key_bits(bits: usize) -> String {
    let nbytes = if bits < 32 { 10 } else { 20 };
    let mut raw = vec![0u8; nbytes];
    getrandom::getrandom(&mut raw).map_err(|_| ()).ok();
    encode_base32(&raw)
}

/// 兼容旧调用的默认生成：160bit（32 字符）
#[cfg(test)]
pub fn generate_secret_key() -> String {
    generate_secret_key_bits(32)
}

/// Base32 编码（RFC 4648 标准字母表，无填充）。
/// 手写实现避免为 10 字节编码引入新依赖。
fn encode_base32(data: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity((data.len() * 8 + 4) / 5);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// 根据密钥、算法和时间步长生成 TOTP 验证码
pub fn generate_totp_with_time_step(
    secret: &str,
    algorithm: Algorithm,
    time_step: u64,
) -> Result<String, String> {
    let key_bytes = decode_secret(secret).ok_or_else(|| "Invalid Base32 secret key".to_string())?;

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
pub fn generate_qr_code_data(
    secret_key: &str,
    account_name: &str,
    issuer: &str,
    algorithm: Algorithm,
) -> Result<(Vec<u8>, u32, u32), String> {
    let key_bytes =
        decode_secret(secret_key).ok_or_else(|| "Invalid Base32 secret key".to_string())?;

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

/// 解码 Base32 密钥为字节数组（与后端相同；空密钥视为无效）
fn decode_secret(secret: &str) -> Option<Vec<u8>> {
    let bytes = Secret::Encoded(secret.to_string()).to_bytes().ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 标准测试密钥（"12345678901234567890" 的 Base32）
    const RFC_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn algo_from_index_maps() {
        assert!(matches!(algo_from_index(0), Algorithm::SHA1));
        assert!(matches!(algo_from_index(1), Algorithm::SHA256));
        assert!(matches!(algo_from_index(2), Algorithm::SHA512));
        // 越界回落 SHA1
        assert!(matches!(algo_from_index(99), Algorithm::SHA1));
    }

    #[test]
    fn totp_code_shape_and_determinism() {
        let a = generate_totp_with_time_step(RFC_SECRET, Algorithm::SHA1, 0).expect("valid secret");
        let b = generate_totp_with_time_step(RFC_SECRET, Algorithm::SHA1, 0).expect("valid secret");
        assert_eq!(a, b, "同一时间步长应产生相同验证码");
        assert_eq!(a.len(), 6, "验证码固定 6 位");
        assert!(a.chars().all(|c| c.is_ascii_digit()), "验证码应为纯数字");

        // 不同时间步长（±30s 以上）大概率不同；至少格式一致
        let c = generate_totp_with_time_step(RFC_SECRET, Algorithm::SHA1, 1).expect("valid");
        assert_eq!(c.len(), 6);
    }

    #[test]
    fn totp_rejects_invalid_secret() {
        assert!(generate_totp("not-base32!!", Algorithm::SHA1).is_err());
        assert!(generate_totp("", Algorithm::SHA1).is_err());
    }

    #[test]
    fn run_returns_code_or_error_text() {
        assert_eq!(run(RFC_SECRET, Algorithm::SHA1).len(), 6);
        assert!(!run("bad!", Algorithm::SHA1).is_empty());
    }

    #[test]
    fn generated_secret_is_usable() {
        let secret = generate_secret_key();
        assert!(!secret.is_empty());
        // 随机生成的密钥必须能直接产出验证码
        assert_eq!(generate_totp(&secret, Algorithm::SHA1).unwrap().len(), 6);
    }

    #[test]
    fn generated_secret_lengths() {
        // 16 位选项 → 16 字符（10 字节 80bit），32 位 → 32 字符（20 字节 160bit）
        assert_eq!(generate_secret_key_bits(16).len(), 16);
        assert_eq!(generate_secret_key_bits(32).len(), 32);
        // 越界取值就近归一
        assert_eq!(generate_secret_key_bits(8).len(), 16);
        assert_eq!(generate_secret_key_bits(64).len(), 32);
        // 两种长度都必须能直接产出验证码
        for bits in [16usize, 32] {
            let s = generate_secret_key_bits(bits);
            assert_eq!(generate_totp(&s, Algorithm::SHA1).unwrap().len(), 6);
        }
    }

    #[test]
    fn base32_encoding_matches_reference() {
        // RFC 4648 测试向量（无填充）
        assert_eq!(encode_base32(b""), "");
        assert_eq!(encode_base32(b"f"), "MY======".trim_end_matches('='));
        assert_eq!(encode_base32(b"fo"), "MZXQ====".trim_end_matches('='));
        assert_eq!(encode_base32(b"foo"), "MZXW6===".trim_end_matches('='));
        assert_eq!(encode_base32(b"foob"), "MZXW6YQ");
        assert_eq!(encode_base32(b"fooba"), "MZXW6YTB");
        assert_eq!(encode_base32(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn qr_entry_valid_and_invalid() {
        let entry = generate_qr_entry(RFC_SECRET, "user@test", "QiToolbox", Algorithm::SHA1);
        let e = entry.expect("合法密钥应产出二维码");
        assert!(e.w > 0 && e.h > 0);
        assert_eq!(
            e.rgba.len(),
            (e.w * e.h * 4) as usize,
            "RGBA 长度须与尺寸匹配"
        );
        assert!(e.w % 4 == 0 && e.h % 4 == 0, "放大 4 倍");

        assert!(generate_qr_entry("bad!", "u", "i", Algorithm::SHA1).is_none());
    }
}
