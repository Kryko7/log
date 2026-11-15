# Log — Personal Journal

A tactile, local-first personal journal desktop application built completely with **Tauri v2**, Rust (`journal-types`, `journal-core`, `journal-import`, `journal-sync`), SQLite FTS5, client-side `age` encryption, and a zero-framework ES-module frontend (`ui/`).

## Design Philosophy

**Tactile Editorial Stationery × Playful Scrapbook**
- **Spacious Single-Sheet Page**: Warm Day Cream paper (`#FBF7F0`), Walnut ink (`#26201D`), Botanical Sage (`#546E5A`), Terracotta sealing ink (`#B5563E`), and Candlelit Night mode (`#161414` / `#D99B52`) paired with a WebGL2 paper-grain & desk-lamp shader.
- **Downward-Scrolling Prose**: Auto-expanding Markdown editor with zero internal textarea scrollbar, smart Day One auto-title deduplication, and below-the-fold Photographs, Voice Notes, Entry Details ledger, and Connections.
- **Physical Scrapbook Craft**: Die-cut alpha-transparent botanical stickers, rotary postal ink stamps, 3D tilt-reactive Polaroid photographs with washi tape, and 64-bar interactive voice memo waveforms.
- **Zero-Slop Craft**: Fine `1.4px` stroke fountain-pen SVG iconography throughout the UI chrome.

## Local-First Storage & Privacy

- **Plain Markdown Vault**: Every entry is stored as a human-readable UTF-8 Markdown file with YAML frontmatter under `~/Personal/JournalVault` (overridable via `--vault <PATH>` or `LOG_VAULT_DIR`).
- **Rebuildable Search Index**: SQLite FTS5 index (`.index/index.sqlite`) and thumbnails (`.index/thumbs/`) are derived solely from your vault files and can be rebuilt at any time.
- **Client-Side `age` Encryption**: Remote backups are packed as `.tar.gz` and encrypted client-side with `age` passphrases before leaving your machine for Google Drive (`LogJournal` folder).
- **Zero Telemetry**: Pure native Tauri v2 desktop window with local `vault://` media streaming and zero cloud tracking.

## Architecture

| Component | Purpose |
| --- | --- |
| `crates/journal-types` | Shared domain models (`Entry`, `Notebook`, `PolaroidMeta`, `VoiceMemoMeta`, `MigrationReport`, `BackupInfo`, etc.). |
| `crates/journal-core` | Vault persistence, Markdown + YAML frontmatter serializer, SQLite FTS5 search, `[[wikilink]]` backlinks, calendar summaries, On This Day, EXIF photo ingestion, and WAV peak extraction. |
| `crates/journal-import` | Primary Day One (`.zip` / `.json`) importer and Notion delta importer with automatic Day One deduplication and audit reports. |
| `crates/journal-sync` | Pure-Rust `age` backup encryption/decryption, Google Drive sync, and daily weather + read-only Google Calendar prompt context. |
| `ui/` | Zero-framework modular ES-module frontend (`index.html`, `styles/main.css`, `src/*.js`, `assets/`) bundled into Tauri's `frontendDist`. |
| `src-tauri/` | Pure Tauri v2 desktop application & CLI (`log-journal` binary), `vault://` custom URI media protocol, and 23 `#[tauri::command]` IPC handlers. |

## Quick Start (Arch Linux)

### 1. Install System Dependencies (`webkit2gtk-4.1`)

```bash
./scripts/install-arch-deps.sh
```

### 2. Build the Native Tauri v2 Binary

```bash
# Build artifacts are routed to /tmp/log-cargo-target via .cargo/config.toml
cargo build --manifest-path src-tauri/Cargo.toml --release
cp /tmp/log-cargo-target/release/log-journal ./log-journal
chmod +x ./log-journal
```

### 3. Install to App Launcher & Pin to Taskbar

```bash
# Build (or pass --skip-build if ./log-journal exists) and install:
#   - ~/.local/bin/log-journal
#   - ~/.local/share/applications/dev.logjournal.app.desktop
#   - ~/.local/share/icons/hicolor/{32x32,128x128,256x256,512x512}/apps/dev.logjournal.app.png
./scripts/install.sh
```

Or install directly from the compiled binary (`./log-journal install-desktop`), or build a system package on Arch Linux via `cd packaging/arch && makepkg -si`.

Because `Log` registers `enableGTKAppId` (`dev.logjournal.app`) on Wayland/X11 matching `dev.logjournal.app.desktop` (`StartupWMClass=dev.logjournal.app`), it appears in your app launcher, keeps crisp taskbar icons, and supports pinning to the dock/taskbar.

To uninstall:

```bash
./scripts/uninstall.sh
# or: log-journal uninstall-desktop
```

### 4. Launch Desktop App (or Seed a Demo Vault)

```bash
# Optional: populate an isolated demo vault first
./log-journal --vault /tmp/log-demo-vault seed-demo

# Launch the native Tauri v2 desktop window (defaults to ~/Personal/JournalVault)
./scripts/launch-wayland.sh
```

## CLI Reference (`log-journal`)

Global flag: `--vault <PATH>` (overrides `LOG_VAULT_DIR`; defaults to `~/Personal/JournalVault`).

- `log-journal` or `log-journal launch` — Launch the native Tauri v2 desktop window.
- `log-journal install-desktop [--bin-dir <DIR>] [--data-dir <DIR>]` — Install binary, XDG `.desktop` launcher (`dev.logjournal.app.desktop`), and hicolor icons.
- `log-journal uninstall-desktop [--bin-dir <DIR>] [--data-dir <DIR>]` — Remove installed binary, `.desktop` file, and hicolor icons.
- `log-journal seed-demo` — Populate an illustrative vault with notebooks, Polaroid photo + washi tape, 3-second synthesized voice memo, bidirectional `[[wikilinks]]`, and an "On This Day" entry from one year ago.
- `log-journal import-dayone --zip <PATH> [--notebook <ID>] [--dry-run]` — Import a Day One `.zip` or `.json` export.
- `log-journal import-notion --path <PATH> [--notebook <ID>] [--dry-run]` — Import a Notion folder/ZIP or MCP JSON snapshot, skipping entries already present from Day One.
- `log-journal backup --passphrase-env <ENV> [--upload-gdrive]` — Create an `age`-encrypted `.tar.gz.age` vault snapshot (and optionally upload to Google Drive).
- `log-journal restore --file <PATH> --target <DIR> --passphrase-env <ENV> [--overwrite]` — Decrypt and restore an `.age` archive into `--target`.

## Tauri IPC & `vault://` Media Protocol

- **23 IPC Commands**: `get_vault_stats`, `list_notebooks`, `create_notebook`, `list_entries`, `get_entry`, `save_entry`, `delete_entry`, `search_entries`, `list_tags`, `get_calendar_month`, `get_on_this_day`, `get_backlinks`, `list_templates`, `get_daily_context`, `get_asset_manifest`, `import_dayone`, `import_notion`, `list_backups`, `create_backup`, `restore_backup`, `sync_gdrive`, `upload_photo`, `upload_audio`.
- **`vault://` Custom URI Protocol**: Streams `/vault-media/...` and `/vault-thumbs/...` directly from disk with path-traversal protection and proper `Content-Type` headers.

## Desktop Integration & Scripts

- `scripts/install-arch-deps.sh` — Install `webkit2gtk-4.1` and GTK build dependencies on Arch Linux.
- `scripts/install.sh` / `scripts/uninstall.sh` — Build and install/remove the XDG launcher (`dev.logjournal.app.desktop`), icons, and `~/.local/bin/log-journal`.
- `scripts/launch-wayland.sh` — Launch `log-journal` with Wayland WebKitGTK DMABUF safety guard (`WEBKIT_DISABLE_DMABUF_RENDERER=1`).
- `packaging/arch/PKGBUILD` — Arch Linux `makepkg` recipe for system-wide installation.
- `scripts/import-all.sh` — Batch Day One + Notion migration helper.
- `scripts/sync-cron.sh` / `scripts/nightly-backup.sh` — Automated `age`-encrypted vault backups and Google Drive sync.

## Verification

```bash
cargo test --workspace
node scripts/check-ui.mjs
```
