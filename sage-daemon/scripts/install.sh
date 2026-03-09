#!/bin/bash
set -euo pipefail

SAGE_HOME="$HOME/.sage"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PLIST_SRC="$PROJECT_DIR/launchd/com.sage.daemon.plist"
PLIST_DST="$HOME/Library/LaunchAgents/com.sage.daemon.plist"

echo "=== Sage Daemon Installer ==="

# Build
echo "[1/4] Building sage-daemon (release)..."
cd "$PROJECT_DIR"
cargo build --release

# Install binary + config
echo "[2/4] Installing to $SAGE_HOME..."
mkdir -p "$SAGE_HOME/bin" "$SAGE_HOME/logs" "$SAGE_HOME/memory"
cp target/release/sage-daemon "$SAGE_HOME/bin/"
cp config.toml "$SAGE_HOME/config.toml"

# Install LaunchAgent
echo "[3/4] Installing LaunchAgent..."
if launchctl list | grep -q "com.sage.daemon"; then
    launchctl unload "$PLIST_DST" 2>/dev/null || true
fi
cp "$PLIST_SRC" "$PLIST_DST"
launchctl load "$PLIST_DST"

echo "[4/4] Done!"
echo ""
echo "  Binary:  $SAGE_HOME/bin/sage-daemon"
echo "  Config:  $SAGE_HOME/config.toml"
echo "  Logs:    $SAGE_HOME/logs/"
echo "  Memory:  $SAGE_HOME/memory/"
echo ""
echo "Commands:"
echo "  sage-daemon --heartbeat-once    # Run once"
echo "  launchctl stop com.sage.daemon  # Stop"
echo "  launchctl start com.sage.daemon # Start"
echo "  tail -f ~/.sage/logs/sage.*.log # Watch logs"
