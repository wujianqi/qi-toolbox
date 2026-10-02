//! 跨会话记忆用户输入（上次输入回填 / 提交时持久化）
//!
//! - 存储后端：本地 libSQL 数据库 `%APPDATA%\qi-toolbox\store.db`（core::store 的 kv 表），
//!   不再使用 txt 文件；首次运行时若检测到旧 `inputs.txt` 会一次性导入并删除。
//! - 敏感项（键在 [`is_secret_key`] 注册表内）写入前用应用主口令派生密钥加密
//!   （core::master，AES-256-GCM）；未解锁时拒绝写入（宁可跳过不落盘，也不明文保存）。
//! - 本模块是尽力而为的记忆缓存：读写失败一律静默忽略，不打断任何用户操作。

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64ct::{Base64, Encoding};

/// 配置目录（store 初始化时自动创建）。Windows 走 APPDATA；其它平台预留
/// XDG_CONFIG_HOME / ~/.config，供未来跨平台版本使用。
pub fn config_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                // APPDATA 缺失（极少见）：退回用户主目录下的 Roaming
                std::env::var_os("USERPROFILE")
                    .map(|p| PathBuf::from(p).join("AppData").join("Roaming"))
                    .unwrap_or_default()
            })
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_default()
    };
    base.join("qi-toolbox")
}

/// 敏感键注册表：命中者写入时走加密后端（不落明文）。
/// 约定：`<模块>.<字段>`；新增需记忆的敏感输入在此登记。
pub fn is_secret_key(key: &str) -> bool {
    matches!(
        key,
        "sftp.pass" | "turso.token" | "totp.key" | "pwd.input" | "mysql.pass" | "pg.pass"
    )
}

/// 读取全部记忆输入（自动解密敏感项；解密失败/损坏的行跳过）。
/// 首次调用时把旧版 `inputs.txt` 的内容一次性导入数据库，随后删除旧文件。
pub fn load() -> BTreeMap<String, String> {
    migrate_legacy_file();
    let mut map = BTreeMap::new();
    match crate::core::store::kv_all() {
        Ok(rows) => {
            for (key, enc) in rows {
                let mut buf = vec![0u8; enc.len()];
                let Ok(bytes) = Base64::decode(enc.trim(), &mut buf) else {
                    continue;
                };
                let bytes = bytes.to_vec();
                let value = if is_secret_key(&key) {
                    match unprotect(&bytes) {
                        Ok(p) => String::from_utf8_lossy(&p).into_owned(),
                        // 无法解密（换用户/换机器/跨平台）：整条丢弃
                        Err(_) => continue,
                    }
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                };
                map.insert(key, value);
            }
        }
        Err(e) => {
            crate::core::log::warn("settings", &format!("load failed: {}", e));
        }
    }
    map
}

/// 旧版 txt 记忆文件一次性迁移：解析行 → 写入 kv 表 → 删除旧文件（尽力而为）。
fn migrate_legacy_file() {
    let legacy = config_dir().join("inputs.txt");
    let text = match std::fs::read_to_string(&legacy) {
        Ok(t) => t,
        Err(_) => return, // 首次运行 / 已迁移：常态
    };
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Some((key, val_b64)) = line.split_once('\t') else {
            continue;
        };
        // 兼容旧写法：以 `!` 开头的键名即密文标记
        let key = key.strip_prefix('!').unwrap_or(key).to_string();
        let val = val_b64.trim().to_string();
        if !key.is_empty() {
            entries.push((key, val));
        }
    }
    if entries.is_empty() {
        let _ = std::fs::remove_file(&legacy);
        return;
    }
    let refs: Vec<(&str, &str)> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    if crate::core::store::kv_set(&refs).is_ok() {
        let _ = std::fs::remove_file(&legacy);
    }
}

/// 合并写入若干键值：`Some(v)` 记入（敏感键自动加密），`None` 删除该键。
/// 内部流程：读现库 → 应用变更 → 整表重写；失败静默（尽力而为）。
pub fn commit(entries: &[(&str, Option<&str>)]) {
    let mut map = load();
    for (key, val) in entries {
        match val {
            Some(v) => {
                map.insert(key.to_string(), v.to_string());
            }
            None => {
                map.remove(*key);
            }
        }
    }
    save(&map);
}

/// 把整份设置写回数据库（敏感键加密、非敏感明文；加密失败则跳过该键）。
fn save(map: &BTreeMap<String, String>) {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut dels: Vec<String> = Vec::new();
    for (key, value) in map {
        let encoded = if is_secret_key(key) {
            match protect(value.as_bytes()) {
                Ok(cipher) => Base64::encode_string(&cipher), // 无加密后端/加密失败：宁可丢失也不明文
                Err(_) => continue,
            }
        } else {
            Base64::encode_string(value.as_bytes())
        };
        rows.push((key.clone(), encoded));
    }
    // 删除库里已不存在于 map 的键（None 删除语义）
    if let Ok(existing) = crate::core::store::kv_all() {
        for (k, _) in existing {
            if !map.contains_key(&k) {
                dels.push(k);
            }
        }
    }
    let refs: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    if crate::core::store::kv_set(&refs).is_err() {
        crate::core::log::warn("settings", "save: kv write failed");
    }
    for k in &dels {
        let _ = crate::core::store::kv_del(k);
    }
}

// ────────────────────── 加密后端（应用主口令） ──────────────────────

/// 加密一段明文：应用主口令派生密钥（AES-256-GCM，`v2:` 前缀）。
/// 主口令未解锁时返回 Err，调用方跳过落盘（宁可丢失也不落明文）。
pub(crate) fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    crate::core::master::protect(plain)
}

/// 解密一段明文：主口令派生密钥；口令错误 / 密文损坏返回 Err。
pub(crate) fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    crate::core::master::unprotect(cipher)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 敏感键注册表：登记的键命中、大小写敏感、未登记的键不误伤
    #[test]
    fn secret_key_registry() {
        for k in [
            "sftp.pass",
            "turso.token",
            "totp.key",
            "pwd.input",
            "mysql.pass",
            "pg.pass",
        ] {
            assert!(is_secret_key(k), "{} 应命中敏感键", k);
        }
        // 未登记的键与变体不得误伤（否则明文/加密路径判断错乱）
        assert!(!is_secret_key("sftp.host"));
        assert!(!is_secret_key("SFTP.PASS"));
        assert!(!is_secret_key("sftp.password"));
        assert!(!is_secret_key(""));
    }
}
