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

# Install the app icon + desktop entry so the taskbar/dock shows the Meridian
# icon. Linux window managers source the taskbar icon from the .desktop entry
# + icon theme (NOT from the window's embedded _NET_WM_ICON) — without an
# entry they fall back to the default "gears" icon. The entry is matched to
# the running window via StartupWMClass, which winit sets from argv[0]'s
# basename ("meridian" for dist/meridian).
ICON_SRC="$DIR/assets/meridian-icon.png"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
ICON_DIR="$DATA_HOME/icons/hicolor/256x256/apps"
DESKTOP_DIR="$DATA_HOME/applications"

if [ -f "$ICON_SRC" ]; then
  mkdir -p "$ICON_DIR" "$DESKTOP_DIR"
  cp --remove-destination -f "$ICON_SRC" "$ICON_DIR/meridian-hub.png"

  cat > "$DESKTOP_DIR/meridian-hub.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Meridian Hub
Comment=Meridian device hub (pure Rust)
Exec="$DIR/dist/meridian"
Icon=meridian-hub
Terminal=false
Categories=Development;Utility;
StartupWMClass=meridian
EOF

  update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
  gtk-update-icon-cache -f -t "$DATA_HOME/icons/hicolor" 2>/dev/null || true
  echo "  ✓ App icon + desktop entry installed:"
  echo "    $ICON_DIR/meridian-hub.png"
  echo "    $DESKTOP_DIR/meridian-hub.desktop"
fi

echo "=================================================="
echo "  SUCCESS! Standalone binary created:"
echo "  $DIR/dist/meridian"
echo "=================================================="
