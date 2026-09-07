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

# Bundle sidecar binaries and sideload engine alongside the executable
if [ -d "$DIR/../../apps/hub/src/meridian_py" ]; then
    mkdir -p "$DIR/bin/meridian_py"
    cp -r "$DIR/../../apps/hub/src/meridian_py/"* "$DIR/bin/meridian_py/"
fi
cp --remove-destination -rf "$DIR/bin/"* "$DIR/dist/bin/"
chmod +x "$DIR/dist/bin/meridian-mesh"* 2>/dev/null || true
chmod +x "$DIR/dist/bin/zsign"* 2>/dev/null || true
chmod +x "$DIR/dist/bin/sideload-engine.py"* 2>/dev/null || true

echo "=================================================="
echo "  SUCCESS! Standalone binary created:"
echo "  $DIR/dist/meridian"
echo "=================================================="
