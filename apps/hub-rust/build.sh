#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "=================================================="
echo "  Building Meridian Hub (Pure Rust - Linux)       "
echo "=================================================="

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo/target/meridian-hub}"

cargo build --release

mkdir -p "$DIR/dist/bin"
cp --remove-destination -f "$CARGO_TARGET_DIR/release/meridian-hub" "$DIR/dist/meridian"
chmod +x "$DIR/dist/meridian"

# Bundle native sidecar binaries alongside the executable
cp --remove-destination -rf "$DIR/bin/"* "$DIR/dist/bin/"
chmod +x "$DIR/dist/bin/meridian-mesh"* 2>/dev/null || true
chmod +x "$DIR/dist/bin/zsign"* 2>/dev/null || true

echo "=================================================="
echo "  SUCCESS! Standalone binary created:"
echo "  $DIR/dist/meridian"
echo "=================================================="
