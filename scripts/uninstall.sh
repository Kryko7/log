#!/usr/bin/env bash
# Remove Log from the user's XDG application launcher, icons, and binary directory.
#
# Usage:
#   ./scripts/uninstall.sh [--bin-dir <DIR>] [--data-dir <DIR>]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/log-cargo-target}"

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
PASS_ARGS=("$@")

i=0
args=("$@")
while [[ $i -lt ${#args[@]} ]]; do
    case "${args[$i]}" in
        --bin-dir)
            if [[ $((i + 1)) -lt ${#args[@]} ]]; then
                BIN_DIR="${args[$((i + 1))]}"
            fi
            i=$((i + 2))
            ;;
        --data-dir)
            if [[ $((i + 1)) -lt ${#args[@]} ]]; then
                DATA_DIR="${args[$((i + 1))]}"
            fi
            i=$((i + 2))
            ;;
        *)
            i=$((i + 1))
            ;;
    esac
done

BIN=""
for candidate in \
    "$BIN_DIR/log-journal" \
    "$REPO_ROOT/log-journal" \
    "$TARGET_DIR/release/log-journal" \
    "$(command -v log-journal 2>/dev/null || true)"; do
    if [[ -n "$candidate" && -x "$candidate" ]]; then
        BIN="$candidate"
        break
    fi
done

if [[ -n "$BIN" ]]; then
    exec "$BIN" uninstall-desktop "${PASS_ARGS[@]}"
fi

rm -f "$BIN_DIR/log-journal"
rm -f "$DATA_DIR/applications/dev.logjournal.app.desktop"
for size in 32x32 128x128 256x256 512x512; do
    rm -f "$DATA_DIR/icons/hicolor/$size/apps/dev.logjournal.app.png"
done

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DATA_DIR/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f "$DATA_DIR/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Removed Log desktop entry, icons, and binary from $BIN_DIR and $DATA_DIR"
