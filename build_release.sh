#!/usr/bin/env bash
# Qi Toolbox 发布构建脚本（单包：中英文内置，启动跟随系统语言，可在主面板侧栏切换）
# 用法: bash build_release.sh
# 输出: release/qi-toolbox

set -e
RELEASE_DIR="release"

echo "=== Qi Toolbox Release Build ==="

# 清理旧产物
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"

# 构建
echo -e "\n[1/1] Building..."
cargo build --release
cp target/release/qi-toolbox "$RELEASE_DIR/qi-toolbox"
echo "  -> $RELEASE_DIR/qi-toolbox"

# 复制发布说明
[ -f README.md ] && cp README.md "$RELEASE_DIR/"

# 输出摘要
echo -e "\n=== Done ==="
ls -lh "$RELEASE_DIR"/qi-toolbox
