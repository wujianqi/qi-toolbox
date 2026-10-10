# 发布检查清单 / Release Checklist

每次发版前逐项核对。**元信息以本 README 为唯一真值**——版本号、功能描述以
`README.md` 为准，其余文件从它同步，避免多处描述漂移。

## 1. 版本号（以 README 为唯一真值）

新版本号先在 README（中英两区如有提及）确定，再同步以下位置：

- [ ] `Cargo.toml` → `version`（唯一编译期真值，`CARGO_PKG_VERSION` 由它派生）
- [ ] `Cargo.lock`（`cargo check` 后自动更新，勿手改）
- [ ] 关于页 / 侧栏版本号显示走 `env!("CARGO_PKG_VERSION")`，无需手改
- [ ] 如改动依赖或功能描述：`Cargo.toml` 的 `description` 与 README 描述一致

## 2. 功能与描述一致性

- [ ] README「功能 / Features」各节与实际界面一致（新增功能已补条目）
- [ ] README 截图区（`docs/screenshots/`）图片与当前界面一致（见该目录 README 约定）
- [ ] `src/i18n/zh-CN.toml` 与 `src/i18n/en.toml` 的 key 一一对应（无缺漏）

## 3. 质量验证

- [ ] `cargo check` 无警告
- [ ] `cargo test` 全部通过
- [ ] `cargo build --release` 成功，产物可正常启动
- [ ] （可选）macOS 真机冒烟：CI 只验证编译 + ad-hoc 签名，窗口/渲染/托盘等
      运行期行为需在 Mac 上确认（下载 CI artifact 或上一版发布包）
- [ ] 资源管理器中确认 exe 文件图标为品牌 logo（`src/logo.ico` 自动嵌入）；
      若改过 `src/logo.svg`，先跑 `cargo run --example gen-icon` 重新生成
- [ ] 手工冒烟：TOTP 生成/备份导入导出、四数据库页连接+SQL+导出（Turso / MySQL / PG / Redis）、SFTP 连接/书签、远程检测+报告导出、中英切换、深浅主题

## 4. 打标签与发布

- [ ] 提交信息用 Conventional Commits（`feat:` / `fix:` / `docs:` …）
- [ ] 打 tag：`git tag vX.Y.Z`（与 Cargo.toml 版本一致）
- [ ] GitHub Release：确认 CI 自动上传了四个产物（release.yml：windows-x64.exe /
      linux-x64.tar.gz / macos-arm64.tar.gz / macos-x64.tar.gz），Release Notes
      按模板填写（新增 / 改进 / 修复 / 校验和，双语）
