use bcrypt::{hash, DEFAULT_COST};

pub fn run(password: Option<String>) -> String {
    if let Some(pwd) = password {
        // 生成密码哈希
        match hash(&pwd, DEFAULT_COST) {
            Ok(hash) => hash,
            Err(e) => format!("Hash generation failed: {}", e),
        }
    } else {
        String::from("No input provided")
    }
}
