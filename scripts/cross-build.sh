#!/bin/bash
# ==============================================
# UA-Mask Rust 交叉编译脚本
# 用于在 Linux 环境编译可在 OpenWrt 运行的二进制
# ==============================================

set -e

# 目标架构
TARGET_ARCH="${1:-x86_64}"

# 架构映射
declare -A ARCH_MAP
ARCH_MAP["x86_64"]="x86_64-unknown-linux-musl"
ARCH_MAP["aarch64"]="aarch64-unknown-linux-musl"
ARCH_MAP["armv7"]="armv7-unknown-linux-musleabihf"
ARCH_MAP["mips"]="mips-unknown-linux-musl"
ARCH_MAP["mipsel"]="mipsel-unknown-linux-musl"
ARCH_MAP["mips64"]="mips64-unknown-linux-musl"

TARGET="${ARCH_MAP[$TARGET_ARCH]}"

echo "========================================"
echo "UA-Mask Rust 交叉编译"
echo "目标架构: $TARGET_ARCH"
echo "========================================"

# 检查 Rust
if ! command -v cargo >/dev/null 2>&1; then
    echo "错误: 未安装 Rust"
    echo "请运行: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# 添加目标
echo "添加交叉编译目标: $TARGET"
rustup target add "$TARGET" 2>/dev/null || true

# 编译
echo "开始编译..."
RUSTFLAGS="-C opt-level=3 -C lto" cargo build --release --target "$TARGET"

# 复制二进制
OUTPUT_DIR="files"
mkdir -p "$OUTPUT_DIR"
cp "target/$TARGET/release/UAmask" "$OUTPUT_DIR/UAmask-bin"
chmod +x "$OUTPUT_DIR/UAmask-bin"

echo "========================================"
echo "编译完成!"
echo "二进制文件: $OUTPUT_DIR/UAmask-bin"
echo ""
echo "将整个 UAmask-Rust 文件夹复制到 OpenWrt package/luci 目录即可编译"
echo "========================================"