# Design & Privacy Rules — Log

## 1. Zero-Slop Editorial Craft

- **No Emoji in UI Chrome**: Never render Unicode emojis (`🕯️`, `🌧️`, `✨`, `🎙️`, etc.) in navigation, buttons, toolbars, badges, or weather/mood indicators. Every icon must be an inline `1.4px` stroke fountain-pen SVG from `crates/journal-ui/src/icons.rs`.
- **Physical Book-Spread Metaphor**:
  - Central spine gutter shadow separating left navigation/index and right folio/editor.
  - Subtle stacked page-edge layers along book edges.
  - Organic paper grain via SVG `feTurbulence` and `wgpu` paper/ink ambient shader (`crates/journal-ui/src/gpu_canvas.rs`).
  - Dates, coordinates, and section headers use tracked small-caps (`letter-spacing: 0.12em`).
  - Long-form prose uses literary serif typography (`Cormorant Garamond`, `Newsreader`, `Georgia`), `line-height: 1.78`, and a `64ch` reading measure.
- **Scrapbook Elements**:
  - Alpha-transparent die-cut botanical stickers (`assets/stamps/*.png`).
  - Engraved postal ink stamps & torn kozo washi tapes (`assets/svg/*.svg`).
  - 3D specular-lit Polaroid frames with handwritten caption margins.

## 2. Authoritative Color Tokens

### Day Mode (Cream Paper)
- Background / Paper: `#FBF7F0`
- Aged Margin / Secondary Paper: `#F3ECE0`
- Primary Ink (Warm Walnut): `#26201D`
- Secondary Ink (Muted Umber): `#6D6157`
- Primary Accent (Botanical Sage): `#546E5A`
- Secondary Accent (Terracotta Sealing Ink): `#B5563E`

### Night Mode (Candlelit Slate)
- Base Slate: `#161414`
- Elevated Folio Surface: `#1F1C1C`
- Primary Ink (Warm Parchment): `#EBE2D5`
- Lamp Glow Accent (Warm Amber): `#D99B52`

## 3. Storage, Disk & Privacy Guard

- **Vault Path**: Defaults strictly to `~/Personal/JournalVault` (overridable via `LOG_VAULT_DIR`). Never create, seed, or commit test vaults inside `/home/minindu/Personal/Log`.
- **Plaintext Local Ownership**: Entries are stored as plain UTF-8 Markdown files with YAML frontmatter. SQLite FTS5 (`index.db`) is a disposable, rebuildable index derived strictly from vault files.
- **Client-Side Encryption**: All remote backups (Google Drive `LogJournal` or exported archives) must be tarred, gzipped, and encrypted client-side using `age` (`age::Encryptor` with passphrase or recipient key) before leaving the machine.
- **Zero Telemetry**: No third-party analytics, crash beacons, or external font/CDN tracking scripts at runtime.
- **Cargo Disk Budget**: Keep `.cargo/config.toml` pointing `target-dir = "/tmp/log-cargo-target"` and `[profile.dev]` stripped (`debug = 0`, `strip = "debuginfo"`, `incremental = false`).
