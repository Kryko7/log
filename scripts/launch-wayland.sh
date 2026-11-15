#!/usr/bin/env bash
# Launch the pure Tauri v2 Log desktop application safely on Arch Linux / Wayland.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN="${LOG_JOURNAL_BIN:-}"
if [[ -z "$BIN" ]]; then
    if [[ -x "$REPO_ROOT/log-journal" ]]; then
        BIN="$REPO_ROOT/log-journal"
    elif [[ -x "/tmp/log-cargo-target/release/log-journal" ]]; then
        BIN="/tmp/log-cargo-target/release/log-journal"
    elif command -v log-journal >/dev/null 2>&1; then
        BIN="$(command -v log-journal)"
    else
        echo "Error: Tauri log-journal binary not found." >&2
        echo "Build it with: CARGO_TARGET_DIR=/tmp/log-cargo-target cargo build --manifest-path src-tauri/Cargo.toml --release" >&2
        exit 1
    fi
fi

# WebKitGTK DMABUF renderer guard on Wayland compositors
if [[ -n "${WAYLAND_DISPLAY:-}" && -z "${WEBKIT_DISABLE_DMABUF_RENDERER:-}" ]]; then
    export WEBKIT_DISABLE_DMABUF_RENDERER=1
fi

exec "$BIN" "$@"
