#!/usr/bin/env bash
# Arch Linux system dependencies required to build and run the pure Tauri v2 desktop app.

set -euo pipefail

echo "Installing Tauri v2 WebKitGTK dependencies on Arch Linux..."
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg
echo "Done. Build the Tauri v2 binary with:"
echo "  CARGO_TARGET_DIR=/tmp/log-cargo-target cargo build --manifest-path src-tauri/Cargo.toml --release"
