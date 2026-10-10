//! 新版本检查：GitHub Releases 最新 tag 与当前版本比对（尽力而为，失败静默）。
//!
//! 用既有 ureq 依赖发起请求（不新增库）；网络失败/超时一律返回 Err，
//! 由调用方决定是否提示（不打扰用户）。

/// 仓库 releases API（与 Cargo.toml repository 一致）
const API_URL: &str = "https://api.github.com/repos/wujianqi/qi-toolbox/releases/latest";

/// 查询最新发布版本号（tag 形如 "v0.3.0"，返回 "0.3.0"；无 v 前缀原样返回）。
pub fn check_latest() -> Result<String, String> {
    let resp = ureq::get(API_URL)
        .set("User-Agent", "qi-toolbox")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| e.to_string())?;
    let text = resp.into_string().map_err(|e| e.to_string())?;
    parse_tag_name(&text)
}

/// 极简 JSON 解析（不引 serde）：从 releases 响应中提取 `"tag_name":"vX.Y.Z"`。
fn parse_tag_name(text: &str) -> Result<String, String> {
    let key = "\"tag_name\"";
    let pos = text
        .find(key)
        .ok_or_else(|| "tag_name not found".to_string())?;
    let rest = &text[pos + key.len()..];
    let colon = rest.find(':').ok_or("bad json")?;
    let after = rest[colon + 1..].trim_start();
    let quote = after.strip_prefix('"').ok_or("bad tag")?;
    let end = quote.find('"').ok_or("bad tag")?;
    let tag = &quote[..end];
    Ok(tag.trim_start_matches('v').to_string())
}

/// 比较 a 与 b（语义化版本 a > b 返回 true；解析失败按字典序）。
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(latest), parse(current));
    for i in 0..3 {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.2.1", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        // 解析失败的段按 0 处理
        assert!(is_newer("0.3", "0.2.5"));
        assert!(!is_newer("x.y.z", "0.0.1"));
        // 带空白的段容错
        assert!(is_newer("0. 3.0 ", "0.2.0"));
    }

    #[test]
    fn tag_parse() {
        // GitHub releases API 实际形态
        assert_eq!(
            parse_tag_name(r#"{"tag_name":"v0.3.0","name":"x"}"#).unwrap(),
            "0.3.0"
        );
        // 无 v 前缀
        assert_eq!(parse_tag_name(r#"{"tag_name": "1.2.3"}"#).unwrap(), "1.2.3");
        // 字段在 JSON 中间
        assert_eq!(
            parse_tag_name(r#"{"url":"…","tag_name":"v10.0.1","x":1}"#).unwrap(),
            "10.0.1"
        );
        // 缺字段 / 非法 JSON
        assert!(parse_tag_name("{}").is_err());
        assert!(parse_tag_name("not json").is_err());
        assert!(parse_tag_name(r#"{"tag_name":}"#).is_err());
    }
}
