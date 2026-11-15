# Log — Personal Journal Application

A tactile, local-first personal journal built with Rust, Leptos + WebGPU (`wgpu`), Axum, SQLite FTS5, and Tauri-ready packaging.

## Design Aesthetic: Tactile Editorial Sanctuary × Playful Scrapbook

- **Zero-Slop Editorial Craft**:
  - **NO emoji icons anywhere in UI chrome** (`🕯️`, `🌧️`, `📔`, etc. are strictly banned). Use fine `1.4px` stroke fountain-pen SVG icons (`crates/journal-ui/src/icons.rs`) exclusively.
  - **Physical Book-Spread Metaphors**: Central spine gutter shadow, stacked page-edge layers, `feTurbulence` / `wgpu` organic paper fibers, small-caps tracking (`0.12em`) for dates and locations, literary serif prose (`Cormorant Garamond` / `Newsreader` / `Georgia`, `line-height: 1.78`, `64ch` measure).
  - **Tactile Scrapbook Artifacts**: Real alpha-transparent die-cut botanical stickers, rotary postal ink stamps, and 3D specular-lit Polaroid frames (`assets/` + `assets/manifest.json`).
- **Color Palette**:
  - **Day Cream Paper**: `#FBF7F0`
  - **Aged Margin**: `#F3ECE0`
  - **Warm Walnut Ink**: `#26201D`
  - **Muted Umber**: `#6D6157`
  - **Botanical Sage**: `#546E5A`
  - **Terracotta Sealing Ink**: `#B5563E`
  - **Candlelit Night Mode**: `#161414` (base) / `#1F1C1C` (surface) / `#EBE2D5` (parchment ink) / `#D99B52` (Warm Amber Lamp)

## Storage & Privacy Architecture

- **Vault Location**: Plain UTF-8 Markdown files with YAML frontmatter stored in `~/Personal/JournalVault` (overridable via `LOG_VAULT_DIR`). **Never** store user journal data or temporary vaults inside `/home/minindu/Personal/Log`.
- **Search Index**: Rebuildable SQLite FTS5 index derived solely from the vault's Markdown files and sidecar metadata.
- **Encrypted Sync**: All remote backups (e.g., Google Drive `LogJournal`) are compressed and encrypted client-side using **`age`** before leaving the machine.
- **Zero Telemetry**: No analytics, tracking beacons, or unencrypted cloud uploads.

## Workspace Layout

- `crates/journal-types`: Pure Rust shared domain types (`serde`/`chrono`/`uuid`, compiles on both `x86_64` and `wasm32-unknown-unknown`).
- `crates/journal-core`: Vault filesystem storage, Markdown + YAML frontmatter parser/serializer, SQLite FTS5 index, templates, media thumbnails, backlinks, and On This Day queries.
- `crates/journal-import`: Importers and migration audit tools (Day One ZIP, Notion export/MCP snapshots, Markdown folders).
- `crates/journal-sync`: Client-side `age` archive encryption/decryption and Google Drive / local backup orchestration.
- `crates/journal-ui`: Leptos CSR frontend + `wgpu` WebGL/WebGPU paper & ink ambient shader canvas, built with Trunk.
- `crates/journal-server`: Local Axum HTTP API & static asset server (`log-journal` binary).
- `src-tauri`: Desktop wrapper skeleton (excluded from workspace Cargo checks when `webkit2gtk-4.1` is unavailable).

## Build & Environment Rules

- **Target Directory**: Always route build artifacts to `/tmp/log-cargo-target` (configured in `.cargo/config.toml` and `CARGO_TARGET_DIR=/tmp/log-cargo-target`) to preserve root disk space.
- **Dev Profile**: Root `Cargo.toml` sets `debug = 0`, `strip = "debuginfo"`, `incremental = false` under `[profile.dev]`.
- **Verification Commands**:
  - `cargo check --workspace`
  - `cargo test -p journal-types`
  - `cargo check -p journal-ui --target wasm32-unknown-unknown`
