//! 应用主口令：全部敏感数据的应用级加密根
//!
//! - 首次启动强制设置主口令；之后每次启动尝试静默解锁
//! - 密钥由口令经 Argon2id（随机盐 16B）派生，AES-256-GCM 加密
//! - 校验器（随机验证值）与盐存 store.db `master_cred` 表，随 S3 备份走：
//!   换机恢复 store.db 后输同一口令即可解密全部敏感数据
//! - 静默解锁：派生密钥经系统 DPAPI 包裹后缓存 `unlock.bin`（仅本机本用户）。
//!   同环境启动直接解包免输口令；换机/换用户/恢复备份后解不开 → 弹口令框重输
//! - 新密文统一 `v2:` 前缀（base64(nonce)+密文），仅接受本格式

use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use base64ct::{Base64, Encoding};

/// 新格式密文前缀
const V2_PREFIX: &str = "v2:";

/// 解锁后的派生密钥（进程内缓存；None = 未解锁）
static KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

// ────────────────────── 派生与加解密 ──────────────────────

/// 口令派生 AES-256 密钥（Argon2id，默认参数，与 backup.rs 同款）
fn derive_key(pass: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    Argon2::default()
        .hash_password_into(pass.as_bytes(), salt, &mut out)
        .map_err(|e| format!("key derive failed: {}", e))?;
    Ok(out)
}

/// 用派生密钥加密（AES-256-GCM），输出 `v2:<base64(nonce|cipher)>`
fn seal(key: &[u8; 32], plain: &[u8]) -> Result<String, String> {
    let mut nonce_b = [0u8; 12];
    getrandom::getrandom(&mut nonce_b).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| e.to_string())?
        .encrypt(Nonce::from_slice(&nonce_b), Payload::from(plain))
        .map_err(|e| format!("encrypt failed: {}", e))?;
    let mut packed = nonce_b.to_vec();
    packed.extend_from_slice(&cipher);
    Ok(format!("{}{}", V2_PREFIX, Base64::encode_string(&packed)))
}

/// 用派生密钥解密 `v2:` 密文
fn open(key: &[u8; 32], enc: &str) -> Result<Vec<u8>, String> {
    let packed =
        Base64::decode_vec(enc.trim_start_matches(V2_PREFIX)).map_err(|e| e.to_string())?;
    if packed.len() < 13 {
        return Err("ciphertext too short".to_string());
    }
    let (nonce_b, cipher) = packed.split_at(12);
    Aes256Gcm::new_from_slice(key)
        .map_err(|e| e.to_string())?
        .decrypt(Nonce::from_slice(nonce_b), Payload::from(cipher))
        .map_err(|_| "wrong master passphrase".to_string())
}

// ────────────────────── 状态查询 ──────────────────────

/// 是否已设置主口令（库里存在 master_cred 记录）
pub fn is_set() -> bool {
    matches!(crate::core::store::master_cred_get(), Ok(Some(_)))
}

/// 是否已解锁（本次会话口令校验通过）
pub fn unlocked() -> bool {
    KEY.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// 密文是否为主口令加密格式
#[allow(dead_code)]
pub fn is_v2(enc: &str) -> bool {
    enc.starts_with(V2_PREFIX)
}

// ────────────────────── 设置 / 解锁 ──────────────────────

/// 首次设置主口令：生成盐与校验值写入 master_cred 并缓存派生密钥。口令过短返回 Err。
pub fn setup(pass: &str) -> Result<(), String> {
    let pass = pass.trim();
    if pass.len() < 6 {
        return Err(crate::lang::MASTER_PASS_SHORT().to_string());
    }
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| e.to_string())?;
    let key = derive_key(pass, &salt)?;
    // 校验值：用派生密钥加密固定魔数，解锁时解得魔数即口令正确
    let verifier = seal(&key, b"QITB-MASTER-OK")?;
    crate::core::store::master_cred_set(&Base64::encode_string(&salt), &verifier)?;
    if let Ok(mut g) = KEY.lock() {
        *g = Some(key);
    }
    // 首次设置即写本机解锁器，之后同环境启动免输口令
    let _ = store_unlock_bin(&key);
    Ok(())
}

/// 解锁：用输入口令对校验值解密验证，通过则缓存派生密钥并刷新本机解锁器
pub fn unlock(pass: &str) -> Result<(), String> {
    let key = unlock_with(pass.trim())?;
    let _ = store_unlock_bin(&key);
    Ok(())
}

/// 校验口令并缓存派生密钥（不写本机解锁器）
fn unlock_with(pass: &str) -> Result<[u8; 32], String> {
    let (salt_b64, verifier) = crate::core::store::master_cred_get()
        .map_err(|e| format!("read master cred: {}", e))?
        .ok_or_else(|| "master passphrase not set".to_string())?;
    let salt = Base64::decode_vec(salt_b64.trim()).map_err(|e| format!("master salt: {}", e))?;
    let key = derive_key(pass, &salt)?;
    open(&key, &verifier).map_err(|_| crate::lang::MASTER_WRONG().to_string())?;
    if let Ok(mut g) = KEY.lock() {
        *g = Some(key);
    }
    Ok(key)
}

// ────────────────────── 本机解锁器（unlock.bin） ──────────────────────

/// 本机解锁器文件：派生密钥经系统 DPAPI 包裹后落盘，仅同机同用户可解包，
/// 用于同环境启动免输口令；换机/换用户/恢复备份后解不开 → 走口令弹窗。
#[cfg(windows)]
fn unlock_file() -> std::path::PathBuf {
    crate::core::settings::config_dir().join("unlock.bin")
}

/// DPAPI 包裹一段字节（仅本机本用户可解）
#[cfg(windows)]
fn dpapi_wrap(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_PROMPTSTRUCT, CRYPT_INTEGER_BLOB,
    };
    const UI_FORBIDDEN: u32 = 0x1;
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &in_blob,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null::<CRYPTPROTECT_PROMPTSTRUCT>(),
            UI_FORBIDDEN,
            &mut out,
        )
    };
    if ok == 0 {
        return Err("DPAPI wrap failed".to_string());
    }
    let wrapped = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(out.pbData as *mut core::ffi::c_void);
    }
    Ok(wrapped)
}

/// DPAPI 解包
#[cfg(windows)]
fn dpapi_unwrap(wrapped: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: wrapped.len() as u32,
        pbData: wrapped.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &in_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut out,
        )
    };
    if ok == 0 {
        return Err("DPAPI unwrap failed".to_string());
    }
    let plain = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(out.pbData as *mut core::ffi::c_void);
    }
    Ok(plain)
}

/// 写本机解锁器（尽力而为：失败只影响下次免输，不报错）
#[cfg(windows)]
fn store_unlock_bin(key: &[u8; 32]) -> Result<(), String> {
    let wrapped = dpapi_wrap(key)?;
    std::fs::write(unlock_file(), wrapped).map_err(|e| e.to_string())
}

/// 尝试静默解锁：本机解锁器可解包且校验值匹配 → 免输口令返回 true
#[cfg(windows)]
pub fn try_silent_unlock() -> bool {
    if !is_set() || unlocked() {
        return unlocked();
    }
    let Ok(wrapped) = std::fs::read(unlock_file()) else {
        return false;
    };
    let Ok(key_b) = dpapi_unwrap(&wrapped) else {
        return false;
    };
    let Ok(key_b32) = <[u8; 32]>::try_from(key_b.as_slice()) else {
        return false;
    };
    // 校验：解得密钥必须能解开校验值（防 unlock.bin 与库不配套，如恢复过旧备份）
    let verifier = match crate::core::store::master_cred_get() {
        Ok(Some((_, v))) => v,
        _ => return false,
    };
    if open(&key_b32, &verifier).is_err() {
        // 解锁器与当前库不配套：丢弃，走口令弹窗
        let _ = std::fs::remove_file(unlock_file());
        return false;
    }
    if let Ok(mut g) = KEY.lock() {
        *g = Some(key_b32);
    }
    true
}

/// 非 Windows：无静默解锁（每次启动输口令）
#[cfg(not(windows))]
pub fn try_silent_unlock() -> bool {
    unlocked()
}

/// 取当前派生密钥（未解锁返回 Err）
pub fn current_key() -> Result<[u8; 32], String> {
    KEY.lock()
        .ok()
        .and_then(|g| *g)
        .ok_or_else(|| "master passphrase locked".to_string())
}

/// 加密一段明文（主口令派生密钥；未解锁返回 Err）
pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    let key = current_key()?;
    let s = seal(&key, plain)?;
    Ok(s.into_bytes())
}

/// 解密主口令密文（口令错误 / 密文损坏返回 Err）
pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    let s = std::str::from_utf8(cipher).map_err(|_| "bad ciphertext".to_string())?;
    if !s.starts_with(V2_PREFIX) {
        return Err("not a v2 ciphertext".to_string());
    }
    let key = current_key()?;
    open(&key, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// derive_key 同口令同盐结果一致，不同盐结果不同
    #[test]
    fn derive_key_deterministic_and_salt_sensitive() {
        let k1 = derive_key("pass-abc", b"salt-salt-salt-16").unwrap();
        let k2 = derive_key("pass-abc", b"salt-salt-salt-16").unwrap();
        let k3 = derive_key("pass-abc", b"other-other-16x").unwrap();
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
        assert_eq!(k1.len(), 32);
    }

    /// seal/open 回路；密文带 v2: 前缀；随机 nonce 使同明文密文不同
    #[test]
    fn seal_open_roundtrip() {
        let key = [7u8; 32];
        let enc = seal(&key, "hello 奇兔宝".as_bytes()).unwrap();
        assert!(is_v2(&enc));
        assert_eq!(open(&key, &enc).unwrap(), "hello 奇兔宝".as_bytes());
        let enc2 = seal(&key, "hello 奇兔宝".as_bytes()).unwrap();
        assert_ne!(enc, enc2, "随机 nonce 应产生不同密文");
    }

    /// 错误密钥 / 损坏密文 / 非 v2 前缀应报错而非 panic
    #[test]
    fn open_rejects_wrong_key_and_garbage() {
        let enc = seal(&[1u8; 32], b"secret").unwrap();
        let wrong = open(&[2u8; 32], &enc);
        assert!(wrong.is_err());
        assert!(open(&[1u8; 32], "v2:!!!not-base64!!!").is_err());
        assert!(open(&[1u8; 32], "v2:AAAA").is_err(), "过短密文应报错");
        assert!(!is_v2("plain-text"));
    }
}
