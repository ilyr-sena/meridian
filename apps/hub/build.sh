#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=================================================="
echo "🚀 Meridian Hub — One-Click Linux Builder"
echo "=================================================="

# Check uv or python3
if command -v uv >/dev/null 2>&1; then
    echo "✓ Using uv for fast environment setup"
    if [ ! -d ".venv" ]; then
        uv venv .venv
    fi
    uv pip install -e . pyinstaller srp
    PYTHON=".venv/bin/python"
elif [ -f ".venv/bin/python" ]; then
    PYTHON=".venv/bin/python"
else
    echo "Creating python venv..."
    python3 -m venv .venv
    .venv/bin/pip install --upgrade pip
    .venv/bin/pip install -e . pyinstaller srp
    PYTHON=".venv/bin/python"
fi

echo "==> Building standalone Linux binary..."
"$PYTHON" packaging/build_binary.py

echo "==> Verifying binary..."
dist/meridian --help >/dev/null

echo "=================================================="
echo "✓ Linux build complete: apps/hub/dist/meridian"
echo "=================================================="
