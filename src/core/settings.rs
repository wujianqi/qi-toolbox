//! 跨会话记忆用户输入（上次输入回填 / 提交时持久化）
//!
//! - 位置：Windows 取 `%APPDATA%\qi-toolbox\inputs.txt`；目录解析已预留
//!   XDG_CONFIG_HOME / `~/.config` 分支，未来跨平台只换目录解析与加密后端。
//! - 格式：每行 `key<TAB>base64(value)`；值一律 base64（base64ct 编码），
//!   避免输入中的换行 / 制表符破坏行结构。
//! - 敏感项（键在 [`is_secret_key`] 注册表内）用平台加密后端落盘：
//!   Windows = DPAPI（CryptProtectData，仅当前 Windows 用户可解密）；
//!   其它平台暂无加密后端 → **宁可跳过不落盘，也不明文保存**（安全回退）。
//! - 写入采用「同目录临时文件 + 原子改名」，避免中途崩溃留下半截文件。
//! - 本模块是尽力而为的记忆缓存：读写失败一律静默忽略，不打断任何用户操作。

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64ct::{Base64, Encoding};

/// 配置目录（`save` 时自动创建）。Windows 走 APPDATA；其它平台预留
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

fn file_path() -> PathBuf {
    config_dir().join("inputs.txt")
}

/// 敏感键注册表：命中者写入时走加密后端（不落明文）。
/// 约定：`<模块>.<字段>`；新增需记忆的敏感输入在此登记。
pub fn is_secret_key(key: &str) -> bool {
    matches!(
        key,
        "sftp.pass" | "turso.token" | "totp.key" | "pwd.input"
    )
}

/// 读取全部记忆输入（自动解密敏感项；解密失败/损坏的行跳过）。
pub fn load() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let text = match std::fs::read_to_string(file_path()) {
        Ok(t) => t,
        Err(e) => {
            // 首次运行文件不存在属常态；其余读失败（权限/占用等）记日志便于排查
            if e.kind() != std::io::ErrorKind::NotFound {
                crate::core::log::warn("settings", &format!("load failed: {}", e));
            }
            return map;
        }
    };
    for line in text.lines() {
        let Some((key, val_b64)) = line.split_once('\t') else {
            continue;
        };
        // 兼容旧写法：以 `!` 开头的键名即密文标记（新版本以 is_secret_key 判定，
        // 标记仅作兜底，双向兼容）
        let key = key.strip_prefix('!').unwrap_or(key);
        let mut buf = vec![0u8; val_b64.len()];
        let Ok(bytes) = Base64::decode(val_b64.trim(), &mut buf) else {
            continue;
        };
        let bytes = bytes.to_vec();
        let value = if is_secret_key(key) {
            match unprotect(&bytes) {
                Ok(p) => String::from_utf8_lossy(&p).into_owned(),
                // 无法解密（换用户/换机器/跨平台文件）：整条丢弃
                Err(_) => continue,
            }
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        map.insert(key.to_string(), value);
    }
    map
}

/// 合并写入若干键值：`Some(v)` 记入（敏感键自动加密），`None` 删除该键。
/// 内部流程：读现文件 → 应用变更 → 整文件原子重写；失败静默（尽力而为）。
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

/// 把整份设置写回文件（敏感键加密、非敏感明文；加密失败则跳过该键）。
fn save(map: &BTreeMap<String, String>) {
    let mut lines = Vec::new();
    for (key, value) in map {
        if is_secret_key(key) {
            let Ok(cipher) = protect(value.as_bytes()) else {
                continue; // 无加密后端 / 加密失败：宁可丢失也不明文落盘
            };
            lines.push(format!("{}\t{}", key, Base64::encode_string(&cipher)));
        } else {
            lines.push(format!("{}\t{}", key, Base64::encode_string(value.as_bytes())));
        }
    }
    let dir = config_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        crate::core::log::warn("settings", "save: create config dir failed");
        return;
    }
    let path = file_path();
    let tmp = dir.join(format!("inputs.tmp.{}", std::process::id()));
    let ok = std::fs::write(&tmp, lines.join("\n")).is_ok()
        && std::fs::rename(&tmp, &path).is_ok();
    // rename 失败时清掉残留临时文件（尽力而为）
    if !ok {
        crate::core::log::warn("settings", "save: write/rename failed (inputs.tmp cleaned)");
        let _ = std::fs::remove_file(&tmp);
    }
}

// ────────────────────── 平台加密后端 ──────────────────────

/// 加密一段明文（返回密文字节）。Windows 用 DPAPI（当前用户作用域）。
#[cfg(windows)]
fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_PROMPTSTRUCT,
    };

    // CRYPTPROTECT_UI_FORBIDDEN：进程内无 UI 提示（免弹窗）
    const UI_FORBIDDEN: u32 = 0x1;

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // 入参类型均为指针；null 表示不用描述串/熵/提示框
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
        return Err("DPAPI protect failed".to_string());
    }
    // 拷贝出密文后释放 DPAPI 分配的内存
    let cipher = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(out.pbData as *mut core::ffi::c_void);
    }
    Ok(cipher)
}

/// 解密 DPAPI 密文。
#[cfg(windows)]
fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
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
        return Err("DPAPI unprotect failed".to_string());
    }
    let plain = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(out.pbData as *mut core::ffi::c_void);
    }
    Ok(plain)
}

/// 非 Windows 平台暂无加密后端：敏感项一律拒绝存储（调用方跳过落盘）。
#[cfg(not(windows))]
fn protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
    Err("secret backend not available on this platform".to_string())
}

#[cfg(not(windows))]
fn unprotect(_cipher: &[u8]) -> Result<Vec<u8>, String> {
    Err("secret backend not available on this platform".to_string())
}
