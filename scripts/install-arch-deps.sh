#!/usr/bin/env bash
# Optional Arch Linux system dependencies for building the native WebKitGTK/Tauri v2 desktop wrapper.
# Note: The local Axum + Chrome/Wayland launcher (`log-journal`) works without webkit2gtk-4.1.

set -euo pipefail

echo "Installing optional native Tauri v2 WebKitGTK dependencies on Arch Linux..."
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg
echo "Done. You can now build src-tauri with: cargo build --manifest-path src-tauri/Cargo.toml"
