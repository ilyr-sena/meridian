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
cp "$CARGO_TARGET_DIR/release/meridian-hub" "$DIR/dist/meridian"
chmod +x "$DIR/dist/meridian"

# Bundle sidecar binaries alongside the executable
cp -r "$DIR/bin/"* "$DIR/dist/bin/"

echo "=================================================="
echo "  SUCCESS! Standalone binary created:"
echo "  $DIR/dist/meridian"
echo "=================================================="
