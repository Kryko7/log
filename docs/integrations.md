# External Integrations Discovery & Contracts

> **Privacy & Read-Only Guarantee**: All integration discovery steps documented here are strictly read-only. Zero Notion pages or databases, Google Drive files or folders, or Google Calendar events are created, modified, or deleted during discovery, and **zero personal journal content or private schedule text** is stored in this repository.

---

## 1. Notion MCP (`notion`)

### Connection & Tool Configuration
- **Plugin Location**: `~/.gemini/config/plugins/notion/`
- **MCP Endpoint**: `https://mcp.notion.com/mcp` (OAuth, managed by Antigravity)
- **Tool Schemas**:
  - `notion-search`: parameters `query` (string, required), `query_type` (`"internal" | "user"`), `data_source_url` (`collection://...`), `page_url`, `filters`, `sort`, `page_size` (1..50, default 10), `max_highlight_length` (0..500).
  - `notion-fetch`: parameters `id` (page/database URL, raw UUID, or `collection://...` data source URL, required), `include_discussions` (bool), `include_file_urls` (bool), `include_transcript` (bool).

### Discovered Journal Schema (Read-Only, Anonymized)
Read-only `notion-search` discovery (board #5) identified two Notion databases:

1. **Primary Journal Database**: `Journal`
   - **Database ID**: `00000000-0000-0000-0000-000000000001`
   - **Data Source URL**: `collection://00000000-0000-0000-0000-000000000002`
   - **Properties Schema & Mapping to `journal-types`**:

| Notion Property | Notion Type | Observed Values / Format | Target `Log` Mapping (`Entry` / `EntryFrontmatter`) |
|---|---|---|---|
| `Entry` | `title` | Entry headline string | `title` + Markdown `# Heading` |
| `Date` | `date` | `date:Date:start` (ISO-8601 date/datetime) | `date` (`DateTime<Utc>`) & `entry_date` |
| `Day` | `select` | `Monday` .. `Sunday` | Informational audit metadata |
| `Source` | `select` | `"Day One"` \| `"Notion"` | `source` (`"dayone"` / `"notion"`) used for primary Day One deduplication |
| `Starred` | `checkbox` | `boolean` | `starred` |
| `Photos` | `number` | integer photo count | Migration parity audit (`MigrationAuditItem`) |
| `Words` | `number` | integer word count | Migration parity audit (`MigrationAuditItem`) |
| `Daily Log` | `relation` | Related page IDs | Cross-reference / `links` backlink hint |

2. **Secondary Journal Database**: `Secondary Journal`
   - **Database ID**: `00000000-0000-0000-0000-000000000003`

### Importer & Delta Deduplication Contract (`crates/journal-import`)
- Support importing from **both**:
  1. Official Notion Markdown + CSV export folders/ZIPs (parsing CSV metadata alongside `.md` files and mapping `Entry`, `Date`, `Source`, `Starred`, `Photos`, `Words`).
  2. Read-only Notion MCP / API JSON snapshots matching the `Journal` schema above.
- **Day One Primary + Notion Delta Rule**:
  - When importing Notion entries after (or alongside) a Day One `.zip` export, if a Notion page has `Source == "Day One"` and an entry already exists on that date in the vault, or if `(YYYY-MM-DD, normalized_title_or_first_paragraph)` matches an existing entry, skip it as `MigrationStatus::SkippedDuplicate`.
  - Import only Notion-unique entries and record full counts and audit items in `MigrationReport`.

---

## 2. Google Drive Plugin (`gdrive`)

### Setup & Authentication
- **Skill Path**: `~/.gemini/config/plugins/gdrive/skills/gdrive/SKILL.md`
- **Binary**: `~/.gemini/config/plugins/gdrive/skills/gdrive/scripts/gdrive` (cached under `~/.gemini/antigravity/plugin_data/gdrive/bin/` and shimmed into `~/.gemini/antigravity/bin/gdrive`).
- **Auth State**: Antigravity OAuth2 (`provider: google`, scopes `https://www.googleapis.com/auth/drive`, `https://www.googleapis.com/auth/userinfo.email`). Prefix commands with `AGY_ACCOUNT=<account>` when multiple accounts are connected.

### Read-Only Listing Contract (`gdrive readonly ls --max 0 --json`)
- **CLI Pagination Note**: `gdrive readonly ls --json` defaults to 20 items when `--max` is omitted. Always pass `--max 0` explicitly when listing full folder contents:
  ```bash
  gdrive readonly ls --max 0 --json
  ```
- **Folder Search**:
  ```bash
  gdrive readonly search --name-exact "LogJournal" --json
  ```
  Filter client-side for `"mimeType": "application/vnd.google-apps.folder"`. (Live read-only check returned `[]`, confirming no `LogJournal` folder exists prior to user-initiated sync.)

### Output Schema (`gdrive readonly ls --max 0 --json` / `search --json`)
Returns a JSON array of file/folder metadata objects:
```json
[
  {
    "id": "1abcDEF...",
    "mimeType": "application/vnd.google-apps.folder",
    "modifiedTime": "2026-11-15T11:30:00.000Z",
    "name": "LogJournal",
    "size": "1024"
  }
]
```
- `id` (`string`): Drive file or folder ID.
- `mimeType` (`string`): `"application/vnd.google-apps.folder"` for folders, or MIME type (`"application/octet-stream"`, `"application/x-age"`, etc.) for files.
- `modifiedTime` (`string`): RFC3339 UTC timestamp.
- `name` (`string`): Filename or folder name.
- `size` (`string`, optional): Decimal byte count string; omitted on native Google Workspace items/folders.

### `age`-Encrypted Backup & Sync Contract (`crates/journal-sync`)
All remote backups **must** compress (`tar` + `gzip`) and encrypt client-side with **`age`** passphrase encryption (`*.tar.gz.age`) before leaving the machine. **Never** upload plaintext Markdown, unencrypted archives, or run `mutate empty-trash`.

- **Create backup folder on first user-initiated sync**:
  ```bash
  gdrive mutate mkdir "LogJournal"
  ```
- **List encrypted backups inside folder**:
  ```bash
  gdrive readonly ls <FOLDER_ID> --max 0 --json
  ```
- **Upload encrypted `.age` archive**:
  ```bash
  gdrive mutate upload /path/to/log-vault-YYYYMMDD-HHMMSS.tar.gz.age --parent <FOLDER_ID>
  ```
- **Download encrypted `.age` archive for local `age` decryption & restore**:
  ```bash
  gdrive readonly download <FILE_ID> --out /tmp/log-restore.tar.gz.age
  ```

---

## 3. Google Calendar Plugin (`gcalendar`)

### Setup & Authentication
- **Skill Path**: `~/.gemini/config/plugins/gcalendar/skills/gcalendar/SKILL.md`
- **Binary**: `~/.gemini/config/plugins/gcalendar/skills/gcalendar/scripts/gcalendar` (cached under `~/.gemini/antigravity/plugin_data/gcalendar/bin/`).
- **Auth State**: Antigravity OAuth2 (`provider: google`, scopes `https://www.googleapis.com/auth/calendar`, `https://www.googleapis.com/auth/userinfo.email`). Prefix with `AGY_ACCOUNT=<account>` when selecting among multiple accounts.

### Read-Only Daily Events Contract
Invoke strictly read-only to populate ephemeral `DailyContext` / `CalendarEventPrompt` suggestions:
```bash
gcalendar readonly events --date YYYY-MM-DD --max 50 --json
```
Never invoke `gcalendar mutate ...` from `Log`.

### Output Schema (`gcalendar readonly events --date YYYY-MM-DD --max 50 --json`)
Returns a JSON array of Google Calendar v3 event objects (`kind: "calendar#event"`):
```json
[
  {
    "kind": "calendar#event",
    "id": "event_instance_id",
    "etag": "\"3572284566470110\"",
    "status": "confirmed",
    "htmlLink": "https://www.google.com/calendar/event?eid=...",
    "created": "2026-08-07T22:38:03.000Z",
    "updated": "2026-08-07T22:38:03.235Z",
    "summary": "Event Title",
    "description": "Optional event notes",
    "location": "Optional location",
    "colorId": "5",
    "eventLabelId": "uuid",
    "eventType": "default",
    "visibility": "private",
    "transparency": "transparent",
    "iCalUID": "uid@google.com",
    "recurringEventId": "master_event_id",
    "creator": { "email": "user@example.com", "self": true },
    "organizer": { "email": "user@example.com", "self": true },
    "start": { "dateTime": "2026-11-15T18:00:00+05:30", "timeZone": "Asia/Colombo" },
    "end": { "dateTime": "2026-11-15T18:15:00+05:30", "timeZone": "Asia/Colombo" },
    "originalStartTime": { "dateTime": "2026-11-15T18:00:00+05:30", "timeZone": "Asia/Colombo" },
    "reminders": {
      "useDefault": false,
      "overrides": [{ "method": "popup", "minutes": 10 }]
    }
  }
]
```
- **All-Day Event Fallback**: All-day events populate `start.date` and `end.date` (`YYYY-MM-DD`, exclusive end date) instead of `dateTime`. Parsers in `journal-sync` / `src-tauri` must deserialize `start.dateTime.or(start.date)` cleanly.
- **Privacy Note**: Calendar events are converted in-memory to ephemeral `CalendarEventPrompt` items (`id`, `title`, `start_time`, `end_time`, `location`, `calendar_name`, `suggested_prompt`) and are **never written to disk** unless the user explicitly clicks a prompt to insert it into a journal entry.
