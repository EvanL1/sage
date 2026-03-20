#!/bin/bash
set -e

# 关闭正在运行的 Sage
osascript -e 'quit app "Sage"' 2>/dev/null || true
sleep 1

# 构建
cd "$(dirname "$0")"
cargo tauri build --bundles app 2>&1 | tail -3

# 安装
rm -rf /Applications/Sage.app
cp -R target/release/bundle/macos/Sage.app /Applications/Sage.app
xattr -cr /Applications/Sage.app  # 移除 Gatekeeper 隔离属性
mkdir -p ~/.sage/bin
cp target/release/sage-desktop ~/.sage/bin/sage

# 启动
open /Applications/Sage.app
echo "✓ Sage deployed and launched"
