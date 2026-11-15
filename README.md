# Log — Personal Journal

A tactile, local-first personal journal built with Rust, Leptos CSR, WebGPU (`wgpu`), Axum, SQLite FTS5, and Tauri v2 desktop packaging.

## Design Philosophy

**Tactile Editorial Sanctuary × Playful Scrapbook**
- **Book-Spread Typography**: Warm Day Cream paper (`#FBF7F0`), Walnut ink (`#26201D`), Botanical Sage (`#546E5A`), Terracotta sealing ink (`#B5563E`), and a Candlelit Night mode (`#161414` / `#D99B52`) paired with an interactive `wgpu` paper-grain & desk-lamp shader.
- **Physical Scrapbook Craft**: Die-cut alpha-transparent botanical stickers, rotary postal ink stamps, 3D tilt-reactive Polaroid photographs with washi tape, and 64-bar interactive voice memo waveforms.
- **Zero-Slop Craft**: Fine `1.4px` stroke fountain-pen SVG iconography throughout the UI chrome.

## Local-First Storage & Privacy

- **Plain Markdown Vault**: Every entry is stored as a human-readable UTF-8 Markdown file with YAML frontmatter under `~/Personal/JournalVault` (overridable via `--vault <PATH>` or `LOG_VAULT_DIR`).
- **Rebuildable Search Index**: SQLite FTS5 index (`.index/index.sqlite`) and thumbnails (`.index/thumbs/`) are derived solely from your vault files and can be rebuilt at any time.
- **Client-Side `age` Encryption**: Remote backups are packed as `.tar.gz` and encrypted client-side with `age` passphrases before leaving your machine for Google Drive (`LogJournal` folder).
- **Zero Telemetry**: No analytics, tracking beacons, or unencrypted cloud uploads.

## Workspace Architecture

| Crate | Purpose |
| --- | --- |
| `crates/journal-types` | Pure Rust domain models (`Entry`, `Notebook`, `PolaroidMeta`, `VoiceMemoMeta`, `MigrationReport`, `BackupInfo`, etc.) shared between native and `wasm32`. |
| `crates/journal-core` | Vault persistence, Markdown + YAML frontmatter serializer, SQLite FTS5 search, `[[wikilink]]` backlinks, calendar summaries, On This Day, EXIF photo ingestion, and WAV peak extraction. |
| `crates/journal-import` | Primary Day One (`.zip` / `.json`) importer and Notion delta importer with automatic Day One deduplication and audit reports. |
| `crates/journal-sync` | Pure-Rust `age` backup encryption/decryption, Google Drive sync, and daily weather + read-only Google Calendar prompt context. |
| `crates/journal-ui` | Leptos CSR frontend compiled to WASM via Trunk, including the `wgpu` paper shader, `/` slash-command editor, voice recorder/player, timeline, calendar mosaic, and On This Day views. |
| `crates/journal-server` | `log-journal` CLI, showcase vault seeder, Axum `/api/invoke/{cmd}` + `/api/upload/*` server, and Arch/Wayland chromeless launcher. |
| `src-tauri` | Tauri v2 desktop wrapper (`log-desktop`) exposing all 21 IPC commands and Wayland WebKit DMABUF guard. |

## Quick Start

### 1. Build Frontend & Server

```bash
# Build the Leptos + WASM frontend
trunk build --release --config crates/journal-ui/Trunk.toml

# Build the release server binary (artifacts routed to /tmp/log-cargo-target)
cargo build --release -p journal-server
cp /tmp/log-cargo-target/release/log-journal ./log-journal
chmod +x ./log-journal
```

### 2. Seed a Demo Vault & Launch

```bash
# Populate a rich showcase vault (or omit --vault to use ~/Personal/JournalVault)
./log-journal seed-demo --vault /tmp/log-demo-vault

# Launch local server on 127.0.0.1:4377 and open a chromeless GPU browser window
./log-journal --vault /tmp/log-demo-vault
```

## CLI Reference (`log-journal`)

Global flag: `--vault <PATH>` (overrides `LOG_VAULT_DIR`; defaults to `~/Personal/JournalVault`).

- `log-journal` or `log-journal launch [--port 4377]` — Start the local server on `127.0.0.1:<port>` and launch `google-chrome-stable --app=http://127.0.0.1:<port>` (falling back to `xdg-open`).
- `log-journal serve [--port 4377]` — Run the headless HTTP API & static asset server.
- `log-journal seed-demo [--vault <PATH>]` — Populate an illustrative showcase vault with notebooks, Polaroid photo + washi tape, 3-second synthesized acoustic chord voice memo, bidirectional `[[wikilinks]]`, and an "On This Day" entry from one year ago.
- `log-journal import-dayone --zip <PATH> [--notebook <ID>] [--dry-run]` — Import a Day One `.zip` or `.json` export.
- `log-journal import-notion --path <PATH> [--notebook <ID>] [--dry-run]` — Import a Notion folder/ZIP or MCP JSON snapshot, skipping entries already present from Day One.
- `log-journal backup --passphrase-env <ENV> [--upload-gdrive]` — Create an `age`-encrypted `.tar.gz.age` vault snapshot (and optionally upload to Google Drive).
- `log-journal restore --file <PATH> --target <DIR> --passphrase-env <ENV> [--overwrite]` — Decrypt and restore an `.age` archive into `--target`.

## HTTP API & Multipart Routes

- `POST /api/invoke/{cmd}` — JSON RPC for all 21 commands: `get_vault_stats`, `list_notebooks`, `create_notebook`, `list_entries`, `get_entry`, `save_entry`, `delete_entry`, `search_entries`, `list_tags`, `get_calendar_month`, `get_on_this_day`, `get_backlinks`, `list_templates`, `get_daily_context`, `get_asset_manifest`, `import_dayone`, `import_notion`, `list_backups`, `create_backup`, `restore_backup`, `sync_gdrive`.
- `POST /api/upload/photo` — Multipart (`field = "file"`) -> `PolaroidMeta`
- `POST /api/upload/audio` — Multipart (`field = "file"`) -> `VoiceMemoMeta`
- `POST /api/upload/dayone` — Multipart (`field = "file"`) -> `MigrationReport`
- `POST /api/upload/notion` — Multipart (`field = "file"`) -> `MigrationReport`
- Static assets: `/assets`, `/vault-media`, `/vault-thumbs`, and `/` SPA fallback (`crates/journal-ui/dist`).

## Desktop Integration (Arch Linux / Wayland)

Install the desktop entry for application launchers:

```bash
cp dist/log-journal.desktop ~/.local/share/applications/
```

For native Tauri v2 builds (requires `webkit2gtk-4.1`), `src-tauri/src/main.rs` automatically sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Wayland sessions when unset to ensure crisp rendering.

## Verification

```bash
cargo check --workspace
cargo test --workspace
cargo check -p journal-ui --target wasm32-unknown-unknown
```
