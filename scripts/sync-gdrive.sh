#!/usr/bin/env bash
# Upload the latest client-side age-encrypted (.tar.gz.age) vault backup
# to a single "LogJournal" folder in Google Drive using the `gdrive` CLI.
#
# Usage:
#   scripts/sync-gdrive.sh [path/to/backup.tar.gz.age] [folder_name]

set -euo pipefail

VAULT_DIR="${LOG_VAULT_DIR:-$HOME/Personal/JournalVault}"
FOLDER_NAME="${2:-${LOG_GDRIVE_FOLDER:-LogJournal}}"
ARCHIVE_PATH="${1:-}"

if [[ -z "$ARCHIVE_PATH" ]]; then
    BACKUPS_DIR="$VAULT_DIR/backups"
    if [[ ! -d "$BACKUPS_DIR" ]]; then
        echo "Error: backups directory not found at $BACKUPS_DIR" >&2
        exit 1
    fi
    ARCHIVE_PATH="$(ls -1t "$BACKUPS_DIR"/*.tar.gz.age 2>/dev/null | head -n 1 || true)"
fi

if [[ -z "$ARCHIVE_PATH" || ! -f "$ARCHIVE_PATH" ]]; then
    echo "Error: no .tar.gz.age encrypted backup archive found." >&2
    exit 1
fi

if [[ "$ARCHIVE_PATH" != *.age ]]; then
    echo "Refusing to upload unencrypted archive: $ARCHIVE_PATH" >&2
    exit 1
fi

if ! command -v gdrive >/dev/null 2>&1; then
    echo "Error: gdrive CLI not found in PATH." >&2
    exit 1
fi

if ! gdrive auth status >/dev/null 2>&1; then
    echo "Error: gdrive is not authenticated. Run 'gdrive auth login' first." >&2
    exit 1
fi

FOLDER_JSON="$(gdrive readonly search --name-exact "$FOLDER_NAME" --type folder --json)"
FOLDER_ID="$(python3 -c '
import json, sys
data = json.load(sys.stdin)
items = data if isinstance(data, list) else data.get("files", [])
print(items[0]["id"] if items else "")
' <<< "$FOLDER_JSON")"

if [[ -z "$FOLDER_ID" ]]; then
    CREATED_JSON="$(gdrive mutate mkdir "$FOLDER_NAME" --json)"
    FOLDER_ID="$(python3 -c '
import json, sys
data = json.load(sys.stdin)
print(data.get("id", ""))
' <<< "$CREATED_JSON")"
fi

if [[ -z "$FOLDER_ID" ]]; then
    echo "Error: unable to resolve or create Google Drive folder '$FOLDER_NAME'." >&2
    exit 1
fi

ARCHIVE_NAME="$(basename "$ARCHIVE_PATH")"
EXISTING_JSON="$(gdrive readonly search --parent-id "$FOLDER_ID" --name-exact "$ARCHIVE_NAME" --json)"
EXISTING_ID="$(python3 -c '
import json, sys
data = json.load(sys.stdin)
items = data if isinstance(data, list) else data.get("files", [])
print(items[0]["id"] if items else "")
' <<< "$EXISTING_JSON")"

if [[ -n "$EXISTING_ID" ]]; then
    echo "Encrypted archive '$ARCHIVE_NAME' already present in Google Drive '$FOLDER_NAME' (id: $EXISTING_ID)."
    exit 0
fi

UPLOAD_JSON="$(gdrive mutate upload "$ARCHIVE_PATH" --parent "$FOLDER_ID" --mime-type application/octet-stream --json)"
echo "$UPLOAD_JSON"
