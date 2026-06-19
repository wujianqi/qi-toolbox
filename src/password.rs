use argon2::Argon2;
use base64ct::{Base64, Encoding};
use sha2::Sha256;
use crate::strings::lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Argon2id,
    Bcrypt,
    Pbkdf2,
}

impl HashAlgorithm {
    pub fn all() -> &'static [HashAlgorithm] {
        &[HashAlgorithm::Argon2id, HashAlgorithm::Bcrypt, HashAlgorithm::Pbkdf2]
    }

    pub fn label(&self) -> &'static str {
        match self {
            HashAlgorithm::Argon2id => "Argon2id",
            HashAlgorithm::Bcrypt => "Bcrypt",
            HashAlgorithm::Pbkdf2 => "PBKDF2",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            HashAlgorithm::Argon2id => lang::ALGO_ARGON2,
            HashAlgorithm::Bcrypt => lang::ALGO_BCRYPT,
            HashAlgorithm::Pbkdf2 => lang::ALGO_PBKDF2,
        }
    }
}

/// 平台预设 — 选择平台后自动匹配对应的密码算法
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformPreset {
    None,       // 自定义（不绑定平台）
    Laravel,    // PHP — Bcrypt / Argon2id
    Django,     // Python — PBKDF2-SHA256
    Spring,     // Java — Bcrypt
    Express,    // Node.js — Bcrypt / Argon2id
    Dotnet,     // ASP.NET — PBKDF2-SHA256
    Rails,      // Ruby — Bcrypt
    WordPress,  // PHP — Argon2id
    Go,         // Golang — Bcrypt
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

    pub fn label(&self) -> &'static str {
        match self {
            PlatformPreset::None => lang::PLAT_NONE,
            PlatformPreset::Laravel => lang::PLAT_LARAVEL,
            PlatformPreset::Django => lang::PLAT_DJANGO,
            PlatformPreset::Spring => lang::PLAT_SPRING,
            PlatformPreset::Express => lang::PLAT_EXPRESS,
            PlatformPreset::Dotnet => lang::PLAT_DOTNET,
            PlatformPreset::Rails => lang::PLAT_RAILS,
            PlatformPreset::WordPress => lang::PLAT_WORDPRESS,
            PlatformPreset::Go => lang::PLAT_GO,
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

    /// 该平台的补充说明
    pub fn note(&self) -> &'static str {
        match self {
            PlatformPreset::None => lang::NOTE_NONE,
            PlatformPreset::Laravel => lang::NOTE_LARAVEL,
            PlatformPreset::Django => lang::NOTE_DJANGO,
            PlatformPreset::Spring => lang::NOTE_SPRING,
            PlatformPreset::Express => lang::NOTE_EXPRESS,
            PlatformPreset::Dotnet => lang::NOTE_DOTNET,
            PlatformPreset::Rails => lang::NOTE_RAILS,
            PlatformPreset::WordPress => lang::NOTE_WORDPRESS,
            PlatformPreset::Go => lang::NOTE_GO,
        }
    }
}

pub fn hash_password(password: &str, algorithm: HashAlgorithm) -> Result<String, String> {
    match algorithm {
        HashAlgorithm::Argon2id => hash_argon2id(password),
        HashAlgorithm::Bcrypt => hash_bcrypt(password),
        HashAlgorithm::Pbkdf2 => hash_pbkdf2(password),
    }
}

/// Argon2id — PHC 格式，兼容 PHP password_hash / Python argon2-cffi / Go golang.org/x/crypto
///
/// 格式: $argon2id$v=19$m=<memory>,t=<time>,p=<parallelism>$<salt_b64>$<hash_b64>
/// 参数: m=19456 (19 MiB), t=2, p=1 — OWASP 2024 最低推荐值
fn hash_argon2id(password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| lang::ERR_SALT.replace("{}", &e.to_string()))?;
    let salt_b64 = Base64::encode_string(&salt);

    let params = argon2::Params::new(19456, 2, 1, None)
        .map_err(|e| lang::ERR_ARGON2_PARAM.replace("{}", &e.to_string()))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut output = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut output)
        .map_err(|e| lang::ERR_ARGON2_HASH.replace("{}", &e.to_string()))?;

    // PHC 标准: hash 用 base64 不带 padding
    let hash_b64 = Base64::encode_string(&output).trim_end_matches('=').to_string();

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
    bcrypt::hash(password, 12).map_err(|e| lang::ERR_BCRYPT_HASH.replace("{}", &e.to_string()))
}

/// PBKDF2-SHA256 — 兼容 Django / Laravel / Python passlib
///
/// 格式: $pbkdf2-sha256$<rounds>$<salt_b64>$<hash_b64>
/// 600,000 轮 — OWASP 2024 推荐值
fn hash_pbkdf2(password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| lang::ERR_SALT.replace("{}", &e.to_string()))?;
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
