#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Building Meridian Hub (Pure Rust - Linux)..."
"$ROOT/apps/hub-rust/build.sh"
