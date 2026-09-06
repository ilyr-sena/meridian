#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Building Meridian Hub (Linux)..."
"$ROOT/apps/hub/build.sh"
