//! 构建脚本：Windows 目标上把 `src/logo.ico` 作为 exe 资源嵌入（资源管理器/
//! 任务栏/快捷方式显示的文件图标）。ico 由 `cargo run --example gen-icon` 从
//! `src/logo.svg` 生成，logo 更新后重新生成即可，无需改这里。

fn main() {
    println!("cargo:rerun-if-changed=src/logo.ico");
    #[cfg(windows)]
    {
        // winresource 依赖无条件编译，但只在 Windows 目标调用（cross 编译时
        // host 可能是 Linux，须按 CARGO_CFG_TARGET_OS 判定而非 host 系统）
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            winresource::WindowsResource::new()
                .set_icon("src/logo.ico")
                .compile()
                .expect("嵌入 Windows 资源（exe 图标）失败");
        }
    }
}
