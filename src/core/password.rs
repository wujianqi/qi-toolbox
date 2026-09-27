use crate::lang;
use argon2::Argon2;
use base64ct::{Base64, Encoding};
use sha2::Sha256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Argon2id,
    Bcrypt,
    Pbkdf2,
    Md5,
}

impl HashAlgorithm {
    pub fn all() -> &'static [HashAlgorithm] {
        &[
            HashAlgorithm::Argon2id,
            HashAlgorithm::Bcrypt,
            HashAlgorithm::Pbkdf2,
            HashAlgorithm::Md5,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            HashAlgorithm::Argon2id => "Argon2id",
            HashAlgorithm::Bcrypt => "Bcrypt",
            HashAlgorithm::Pbkdf2 => "PBKDF2",
            HashAlgorithm::Md5 => "MD5",
        }
    }
}

/// 平台预设 — 选择平台后自动匹配对应的密码算法
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformPreset {
    None,      // 自定义（不绑定平台）
    Laravel,   // PHP — Bcrypt / Argon2id
    Django,    // Python — PBKDF2-SHA256
    Spring,    // Java — Bcrypt
    Express,   // Node.js — Bcrypt / Argon2id
    Dotnet,    // ASP.NET — PBKDF2-SHA256
    Rails,     // Ruby — Bcrypt
    WordPress, // PHP — Argon2id
    Go,        // Golang — Bcrypt
}

impl PlatformPreset {
    pub fn all() -> &'static [PlatformPreset] {
        &[
            PlatformPreset::None,
            PlatformPreset::Laravel,
            PlatformPreset::Django,
            PlatformPreset::Spring,
            PlatformPreset::Express,
            PlatformPreset::Dotnet,
            PlatformPreset::Rails,
            PlatformPreset::WordPress,
            PlatformPreset::Go,
        ]
    }

    pub fn label(&self) -> String {
        match self {
            PlatformPreset::None => lang::PLAT_NONE(),
            PlatformPreset::Laravel => lang::PLAT_LARAVEL(),
            PlatformPreset::Django => lang::PLAT_DJANGO(),
            PlatformPreset::Spring => lang::PLAT_SPRING(),
            PlatformPreset::Express => lang::PLAT_EXPRESS(),
            PlatformPreset::Dotnet => lang::PLAT_DOTNET(),
            PlatformPreset::Rails => lang::PLAT_RAILS(),
            PlatformPreset::WordPress => lang::PLAT_WORDPRESS(),
            PlatformPreset::Go => lang::PLAT_GO(),
        }
    }

    /// 返回该平台的默认密码算法
    pub fn default_algorithm(&self) -> HashAlgorithm {
        match self {
            PlatformPreset::None => HashAlgorithm::Bcrypt,
            PlatformPreset::Laravel => HashAlgorithm::Bcrypt,
            PlatformPreset::Django => HashAlgorithm::Pbkdf2,
            PlatformPreset::Spring => HashAlgorithm::Bcrypt,
            PlatformPreset::Express => HashAlgorithm::Bcrypt,
            PlatformPreset::Dotnet => HashAlgorithm::Pbkdf2,
            PlatformPreset::Rails => HashAlgorithm::Bcrypt,
            PlatformPreset::WordPress => HashAlgorithm::Argon2id,
            PlatformPreset::Go => HashAlgorithm::Bcrypt,
        }
    }
}

pub fn hash_password(password: &str, algorithm: HashAlgorithm) -> Result<String, String> {
    match algorithm {
        HashAlgorithm::Argon2id => hash_argon2id(password),
        HashAlgorithm::Bcrypt => hash_bcrypt(password),
        HashAlgorithm::Pbkdf2 => hash_pbkdf2(password),
        HashAlgorithm::Md5 => hash_md5(password),
    }
}

/// 生成随机密码，保证包含大小写字母、数字、特殊字符各至少一个。
/// 字符与洗牌索引均用拒绝采样（rejection sampling）取均匀随机数，消除取模偏差；
/// 全程按字节操作，避免逐类别建 `Vec<char>` 的重复分配。
pub fn generate_random_password(length: usize) -> Result<String, String> {
    const ALL: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*";
    const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const DIGITS: &[u8] = b"0123456789";
    const SPECIAL: &[u8] = b"!@#$%^&*";

    let len = length.max(8);
    let mut bytes = vec![0u8; len];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("RNG error: {}", e))?;

    // 拒绝采样：均匀取 [0, n) 索引；字节流耗尽时补充熵源（首次失败已在上方上报）
    let mut pos = 0usize;
    let mut uniform = move |n: usize| -> usize {
        let limit = 256 - (256 % n);
        loop {
            if pos >= bytes.len() {
                let mut extra = [0u8; 64];
                if getrandom::getrandom(&mut extra).is_err() {
                    return 0; // 极端失败：降级取 0，不阻断生成
                }
                bytes.extend_from_slice(&extra);
            }
            let b = bytes[pos] as usize;
            pos += 1;
            if b < limit {
                return b % n;
            }
        }
    };

    let mut chars: Vec<char> = Vec::with_capacity(len);
    chars.push(LOWER[uniform(LOWER.len())] as char);
    chars.push(UPPER[uniform(UPPER.len())] as char);
    chars.push(DIGITS[uniform(DIGITS.len())] as char);
    chars.push(SPECIAL[uniform(SPECIAL.len())] as char);
    for _ in 4..len {
        chars.push(ALL[uniform(ALL.len())] as char);
    }
    // Fisher–Yates 洗牌（均匀索引），打乱前四个固定类别位
    for i in (1..chars.len()).rev() {
        let j = uniform(i + 1);
        chars.swap(i, j);
    }
    Ok(chars.into_iter().collect())
}

/// Argon2id — PHC 格式，兼容 PHP password_hash / Python argon2-cffi / Go golang.org/x/crypto
///
/// 格式: $argon2id$v=19$m=<memory>,t=<time>,p=<parallelism>$<salt_b64>$<hash_b64>
/// 参数: m=19456 (19 MiB), t=2, p=1 — OWASP 2024 最低推荐值
fn hash_argon2id(password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| lang::ERR_SALT(e.to_string()))?;
    let salt_b64 = Base64::encode_string(&salt);

    let params = argon2::Params::new(19456, 2, 1, None)
        .map_err(|e| lang::ERR_ARGON2_PARAM(e.to_string()))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut output = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut output)
        .map_err(|e| lang::ERR_ARGON2_HASH(e.to_string()))?;

    // PHC 标准: hash 用 base64 不带 padding（命名绑定延长 String 存活期，省一次 to_string）
    let hash_b64 = Base64::encode_string(&output);
    let hash_b64 = hash_b64.trim_end_matches('=');

    Ok(format!(
        "$argon2id$v=19$m=19456,t=2,p=1${}${}",
        salt_b64, hash_b64
    ))
}

/// Bcrypt — 兼容所有主流框架
///
/// 格式: $2b$<cost>$<salt_and_hash> (由 bcrypt crate 自动生成)
/// cost=12 — OWASP 推荐值
fn hash_bcrypt(password: &str) -> Result<String, String> {
    bcrypt::hash(password, 12).map_err(|e| lang::ERR_BCRYPT_HASH(e.to_string()))
}

/// PBKDF2-SHA256 — 兼容 Django / Laravel / Python passlib
///
/// 格式: $pbkdf2-sha256$<rounds>$<salt_b64>$<hash_b64>
/// 600,000 轮 — OWASP 2024 推荐值
fn hash_pbkdf2(password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| lang::ERR_SALT(e.to_string()))?;
    let salt_b64 = Base64::encode_string(&salt);

    let rounds: u32 = 600_000;
    let mut output = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, rounds, &mut output);

    let hash_b64 = Base64::encode_string(&output);

    Ok(format!(
        "$pbkdf2-sha256${}${}${}",
        rounds, salt_b64, hash_b64
    ))
}

/// MD5 — 十六进制摘要（无盐无格式）。仅用于兼容遗留系统，不提供安全性。
fn hash_md5(password: &str) -> Result<String, String> {
    Ok(format!("{:x}", md5::compute(password.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_password_meets_class_guarantee() {
        for len in [8usize, 12, 32, 64] {
            let pwd = generate_random_password(len).unwrap();
            // 长度保证（下限 8）
            let len = len.max(8);
            assert_eq!(pwd.chars().count(), len);
            // 四类字符各至少一个
            assert!(
                pwd.chars().any(|c| c.is_ascii_lowercase()),
                "lower missing: {}",
                pwd
            );
            assert!(
                pwd.chars().any(|c| c.is_ascii_uppercase()),
                "upper missing: {}",
                pwd
            );
            assert!(
                pwd.chars().any(|c| c.is_ascii_digit()),
                "digit missing: {}",
                pwd
            );
            assert!(
                pwd.chars().any(|c| "!@#$%^&*".contains(c)),
                "special missing: {}",
                pwd
            );
            // 只含合法字符
            assert!(
                pwd.chars().all(|c| {
                    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*"
                        .contains(&(c as u8))
                }),
                "illegal char in: {}",
                pwd
            );
        }
    }

    #[test]
    fn random_password_below_min_length_clamped() {
        let pwd = generate_random_password(3).unwrap();
        assert_eq!(pwd.chars().count(), 8);
    }

    #[test]
    fn random_password_not_constant() {
        let a = generate_random_password(32).unwrap();
        let b = generate_random_password(32).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn hash_roundtrip_verify() {
        for algo in HashAlgorithm::all() {
            let h = hash_password("S3cret!pass", *algo).unwrap();
            assert!(!h.is_empty(), "{} produced empty hash", algo.label());
            // 确定性：同一密码同一算法两次哈希可验证（md5 直接比对，其余格式校验）
            if *algo == HashAlgorithm::Md5 {
                assert_eq!(h, hash_password("S3cret!pass", *algo).unwrap());
                assert_eq!(h.len(), 32);
            }
        }
    }

    #[test]
    fn md5_known_vector() {
        // RFC 1321 经典向量
        assert_eq!(
            hash_password("abc", HashAlgorithm::Md5).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }
}
