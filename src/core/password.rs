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

/// 校验密码与哈希是否匹配（自动识别哈希格式；MD5 直接比对）。
/// 各格式参数从哈希串解析，与生成端格式一一对应。
pub fn verify_password(password: &str, hash: &str) -> Result<bool, String> {
    let hash = hash.trim();
    if hash.starts_with("$argon2id$")
        || hash.starts_with("$argon2i$")
        || hash.starts_with("$argon2d$")
    {
        return verify_argon2(password, hash);
    }
    if hash.starts_with("$2a$") || hash.starts_with("$2b$") || hash.starts_with("$2y$") {
        // bcrypt crate 原生 verify（自带常量时间比较）
        return bcrypt::verify(password, hash).map_err(|e| lang::ERR_BCRYPT_HASH(e.to_string()));
    }
    if hash.starts_with("$pbkdf2-sha256$") {
        return verify_pbkdf2(password, hash);
    }
    // MD5：32 位十六进制直接比对
    if hash.len() == 32 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(hash_password(password, HashAlgorithm::Md5)? == hash.to_ascii_lowercase());
    }
    Err(lang::ERR_HASH_UNKNOWN())
}

/// 解析 PHC 参数段 "m=19456,t=2,p=1" → (m, t, p)；缺项用默认值
fn parse_phc_params(params: &str) -> (u32, u32, u32) {
    let (mut m, mut t, mut p) = (19456u32, 2u32, 1u32);
    for kv in params.split(',') {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        match k {
            "m" => m = v.parse().unwrap_or(m),
            "t" => t = v.parse().unwrap_or(t),
            "p" => p = v.parse().unwrap_or(p),
            _ => {}
        }
    }
    (m, t, p)
}

/// 解析 Base64（无 padding 容错）
fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    // base64ct 要求规范长度，手动补 padding
    let padded = match s.len() % 4 {
        2 => format!("{s}=="),
        3 => format!("{s}="),
        _ => s.to_string(),
    };
    Base64::decode_vec(&padded).map_err(|e| lang::ERR_B64(e.to_string()))
}

/// Argon2（id/i/d 变体）校验：格式 $argon2id$v=19$m=..,t=..,p=..$salt_b64$hash_b64
fn verify_argon2(password: &str, hash: &str) -> Result<bool, String> {
    let parts: Vec<&str> = hash.split('$').filter(|s| !s.is_empty()).collect();
    // parts: [argon2id, v=19, m=..., salt, hash]
    if parts.len() < 5 {
        return Err(lang::ERR_HASH_MALFORMED());
    }
    let variant = match parts[0] {
        "argon2id" => argon2::Algorithm::Argon2id,
        "argon2i" => argon2::Algorithm::Argon2i,
        "argon2d" => argon2::Algorithm::Argon2d,
        _ => return Err(lang::ERR_HASH_MALFORMED()),
    };
    let (m, t, p) = parse_phc_params(parts[2]);
    let params =
        argon2::Params::new(m, t, p, None).map_err(|e| lang::ERR_ARGON2_PARAM(e.to_string()))?;
    let argon2 = Argon2::new(variant, argon2::Version::V0x13, params);
    let salt = b64_decode(parts[3])?;
    let expected = b64_decode(parts[4])?;
    let mut out = vec![0u8; expected.len()];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut out)
        .map_err(|e| lang::ERR_ARGON2_HASH(e.to_string()))?;
    // 常量时间比较
    Ok(subtle_ct_eq(&out, &expected))
}

/// PBKDF2-SHA256 校验：格式 $pbkdf2-sha256$<rounds>$salt_b64$hash_b64
fn verify_pbkdf2(password: &str, hash: &str) -> Result<bool, String> {
    let parts: Vec<&str> = hash.split('$').filter(|s| !s.is_empty()).collect();
    // parts: [pbkdf2-sha256, rounds, salt, hash]
    if parts.len() != 4 {
        return Err(lang::ERR_HASH_MALFORMED());
    }
    let rounds: u32 = parts[1].parse().map_err(|_| lang::ERR_HASH_MALFORMED())?;
    let salt = b64_decode(parts[2])?;
    let expected = b64_decode(parts[3])?;
    let mut out = vec![0u8; expected.len()];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, rounds, &mut out);
    Ok(subtle_ct_eq(&out, &expected))
}

/// 常量时间字节比较（防时序侧信道；长度不等直接 false 不泄长）
fn subtle_ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 识别哈希算法（前缀识别；无法识别返回 None）
pub fn detect_hash_algorithm(hash: &str) -> Option<&'static str> {
    let h = hash.trim();
    if h.starts_with("$argon2id$") {
        Some("Argon2id")
    } else if h.starts_with("$argon2i$") {
        Some("Argon2i")
    } else if h.starts_with("$argon2d$") {
        Some("Argon2d")
    } else if h.starts_with("$2a$") || h.starts_with("$2b$") || h.starts_with("$2y$") {
        Some("Bcrypt")
    } else if h.starts_with("$pbkdf2-sha256$") {
        Some("PBKDF2-SHA256")
    } else if h.starts_with("$pbkdf2$") {
        Some("PBKDF2")
    } else if h.starts_with("{SSHA}") || h.starts_with("{SSHA256}") || h.starts_with("{PBKDF2}") {
        Some("PBKDF2 (LDAP)")
    } else if h.len() == 32 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some("MD5")
    } else if h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some("SHA1")
    } else if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        Some("SHA256")
    } else {
        None
    }
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

    #[test]
    fn verify_roundtrip_all_formats() {
        let pwd = "S3cret!pass";
        for algo in [
            HashAlgorithm::Argon2id,
            HashAlgorithm::Bcrypt,
            HashAlgorithm::Pbkdf2,
        ] {
            let h = hash_password(pwd, algo).unwrap();
            assert!(
                verify_password(pwd, &h).unwrap(),
                "{} 正例应通过",
                algo.label()
            );
            assert!(
                !verify_password("wrong", &h).unwrap(),
                "{} 反例应拒绝",
                algo.label()
            );
        }
        // MD5
        let h = hash_password("abc", HashAlgorithm::Md5).unwrap();
        assert!(verify_password("abc", &h).unwrap());
        assert!(!verify_password("abd", &h).unwrap());
    }

    #[test]
    fn verify_argon2_params_parsing() {
        // 参数解析路径：改小参数后哈希值必然不同 → 同密码校验应返回 Ok(false)
        // （参数参与计算，篡改参数即校验失败），不应报错
        let h = hash_password("x", HashAlgorithm::Argon2id).unwrap();
        let modified = h.replace("m=19456,t=2,p=1", "m=8192,t=3,p=2");
        assert_eq!(verify_password("x", &modified), Ok(false));
        assert_eq!(verify_password("y", &modified), Ok(false));
    }

    #[test]
    fn verify_rejects_unknown_format() {
        assert!(verify_password("x", "$1$xyz$abc").is_err());
        assert!(verify_password("x", "not-a-hash").is_err());
        assert!(verify_password("x", "").is_err());
    }

    #[test]
    fn detect_hash_algorithm_prefixes() {
        assert_eq!(detect_hash_algorithm("$argon2id$v=19$.."), Some("Argon2id"));
        assert_eq!(detect_hash_algorithm("$2b$12$.."), Some("Bcrypt"));
        assert_eq!(
            detect_hash_algorithm("$pbkdf2-sha256$600000$.."),
            Some("PBKDF2-SHA256")
        );
        assert_eq!(
            detect_hash_algorithm("900150983cd24fb0d6963f7d28e17f72"),
            Some("MD5")
        );
        assert_eq!(detect_hash_algorithm("deadbeef"), None);
    }
}
