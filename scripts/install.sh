#!/usr/bin/env bash
# Build (unless --skip-build is passed and a release binary already exists) and
# install Log to the user's XDG application launcher and taskbar/dock.
#
# Usage:
#   ./scripts/install.sh [--skip-build] [--bin-dir <DIR>] [--data-dir <DIR>]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/log-cargo-target}"

SKIP_BUILD=0
PASS_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        *)
            PASS_ARGS+=("$1")
            shift
            ;;
    esac
done

BIN=""
if [[ "$SKIP_BUILD" -eq 1 ]]; then
    if [[ -x "$REPO_ROOT/log-journal" ]]; then
        BIN="$REPO_ROOT/log-journal"
    elif [[ -x "$TARGET_DIR/release/log-journal" ]]; then
        BIN="$TARGET_DIR/release/log-journal"
    fi
fi

if [[ -z "$BIN" ]]; then
    echo "Building Log Tauri v2 release binary..."
    CARGO_TARGET_DIR="$TARGET_DIR" cargo build --manifest-path "$REPO_ROOT/src-tauri/Cargo.toml" --release
    cp "$TARGET_DIR/release/log-journal" "$REPO_ROOT/log-journal"
    chmod +x "$REPO_ROOT/log-journal"
    BIN="$REPO_ROOT/log-journal"
fi

exec "$BIN" install-desktop "${PASS_ARGS[@]}"
