#!/usr/bin/env bash
# Qi Toolbox 多语言发布构建脚本
# 用法: bash build_release.sh
# 输出: release/qi-toolbox_cn, release/qi-toolbox_en

set -e
RELEASE_DIR="release"

echo "=== Qi Toolbox Release Build ==="

# 清理旧产物
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"

# 构建中文版 (默认)
echo -e "\n[1/2] Building Chinese version..."
cargo build --release
cp target/release/qi-toolbox "$RELEASE_DIR/qi-toolbox_cn"
echo "  -> $RELEASE_DIR/qi-toolbox_cn"

# 构建英文版
echo -e "\n[2/2] Building English version..."
cargo build --release --features english
cp target/release/qi-toolbox "$RELEASE_DIR/qi-toolbox_en"
echo "  -> $RELEASE_DIR/qi-toolbox_en"

# 复制发布说明
[ -f README.md ] && cp README.md "$RELEASE_DIR/"

# 输出摘要
echo -e "\n=== Done ==="
ls -lh "$RELEASE_DIR"/qi-toolbox_*
