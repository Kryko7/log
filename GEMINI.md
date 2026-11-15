# Log — Personal Journal Application

A tactile, local-first personal journal desktop application built completely with **Tauri v2**, Rust (`journal-types`, `journal-core`, `journal-import`, `journal-sync`), SQLite FTS5, client-side `age` encryption, and a zero-framework modular ES-module frontend (`ui/`) with WebGL2 paper & ink ambient shading.

## Design Aesthetic: Tactile Editorial Stationery × Playful Scrapbook

- **Zero-Slop Editorial Craft**:
  - **NO emoji icons anywhere in UI chrome** (`🕯️`, `🌧️`, `📔`, etc. are strictly banned). Use fine `1.4px` stroke fountain-pen SVG icons (`ui/src/icons.js`) exclusively.
  - **Quiet Single-Sheet Page Layout**: Generous centered downward-scrolling writing & reading sheet (`max-width: 960px`, inner prose measure `68ch–72ch`, `Newsreader` / `Cormorant Garamond` / `Georgia`, `line-height: 1.85`), auto-growing prose `<textarea>` with zero internal scrollbar, optional cover artwork, and a quiet below-the-fold gallery & archival ledger.
  - **Tactile Scrapbook Artifacts**: Real alpha-transparent die-cut botanical stickers, rotary postal ink stamps, 3D specular-lit Polaroid frames (`ui/assets/` + `ui/assets/manifest.json`), and a WebGL2 + `feTurbulence` paper fiber surface (`ui/src/shader.js`).
- **Color Palette**:
  - **Day Cream Paper**: `#FBF7F0`
  - **Aged Margin**: `#F3ECE0`
  - **Warm Walnut Ink**: `#26201D`
  - **Muted Umber**: `#6D6157`
  - **Botanical Sage**: `#546E5A`
  - **Terracotta Sealing Ink**: `#B5563E`
  - **Candlelit Night Mode**: `#161414` (base) / `#1F1C1C` (surface) / `#EBE2D5` (parchment ink) / `#D99B52` (Warm Amber Lamp)

## Storage & Privacy Architecture

- **Vault Location**: Plain UTF-8 Markdown files with YAML frontmatter stored in `~/Personal/JournalVault` (overridable via `LOG_VAULT_DIR` or `--vault <PATH>`). **Never** store user journal data or temporary vaults inside the source repository.
- **Search Index**: Rebuildable SQLite FTS5 index derived solely from the vault's Markdown files and sidecar metadata.
- **Encrypted Sync**: All remote backups (e.g., Google Drive `LogJournal`) are compressed and encrypted client-side using **`age`** before leaving the machine.
- **Zero Telemetry**: No analytics, tracking beacons, or unencrypted cloud uploads.

## Workspace Layout

- `crates/journal-types`: Pure Rust shared domain types (`serde`/`chrono`/`uuid`).
- `crates/journal-core`: Vault filesystem storage, Markdown + YAML frontmatter parser/serializer, SQLite FTS5 index, templates, media thumbnails, backlinks, and On This Day queries.
- `crates/journal-import`: Importers and migration audit tools (Day One ZIP/JSON, Notion export/MCP snapshots, Markdown folders).
- `crates/journal-sync`: Client-side `age` archive encryption/decryption and Google Drive / local backup orchestration.
- `ui`: Zero-framework ES-module Tauri `frontendDist` (`index.html`, `styles/main.css`, `src/*.js`, `assets/`).
- `src-tauri`: Pure Tauri v2 desktop application & CLI (`log-journal` binary), custom `vault://` media protocol, and 23 typed IPC commands.

## Build & Environment Rules

- **Target Directory**: Always route build artifacts to `/tmp/log-cargo-target` (configured in `.cargo/config.toml` and `CARGO_TARGET_DIR=/tmp/log-cargo-target`) to preserve root disk space.
- **Dev Profile**: Root `Cargo.toml` sets `debug = 0`, `strip = "debuginfo"`, `incremental = false` under `[profile.dev]`.
- **Verification Commands**:
  - `cargo test --workspace` (runs all tests across `journal-types`, `journal-core`, `journal-import`, `journal-sync`)
  - `node --check ui/src/*.js` (verifies ES module syntax)
  - `cargo build --manifest-path src-tauri/Cargo.toml --release` (requires `webkit2gtk-4.1` on Arch Linux; builds `/tmp/log-cargo-target/release/log-journal`)
