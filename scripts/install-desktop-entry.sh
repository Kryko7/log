#!/usr/bin/env bash
set -euo pipefail

# Non-destructive desktop launcher installer for Log Personal Journal (Arch Linux / Wayland).
APP_DIR="/home/minindu/Personal/Log"
DEST_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
DEST_FILE="$DEST_DIR/log-journal.desktop"

mkdir -p "$DEST_DIR"

cat > "$DEST_FILE" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Log — Personal Journal
GenericName=Personal Journal
Comment=Tactile, local-first Markdown journal with WebGPU paper craft and encrypted backup
Exec=$APP_DIR/log-journal launch
Icon=$APP_DIR/assets/svg/app-icon.svg
Terminal=false
Categories=Office;TextEditor;Utility;
Keywords=journal;diary;markdown;notes;scrapbook;
StartupNotify=true
StartupWMClass=log-journal
EOF

chmod 0644 "$DEST_FILE"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DEST_DIR" >/dev/null 2>&1 || true
fi

echo "Installed desktop launcher at $DEST_FILE"
