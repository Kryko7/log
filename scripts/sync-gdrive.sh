#!/usr/bin/env bash
# Upload the latest client-side age-encrypted (.tar.gz.age) vault backup
# to a single "LogJournal" folder in Google Drive.
#
# Supported transports (tried in order):
#   1. log-journal sync (built-in OAuth / saved token / rclone)
#   2. rclone Drive remote
#   3. LOG_GDRIVE_TOKEN via Google Drive v3 REST API (curl)
#   4. gdrive CLI (optional fallback if installed)
#
# Usage:
#   scripts/sync-gdrive.sh [path/to/backup.tar.gz.age] [folder_name]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
VAULT_DIR="${LOG_VAULT_DIR:-$HOME/Personal/JournalVault}"
FOLDER_NAME="${2:-${LOG_GDRIVE_FOLDER:-LogJournal}}"
ARCHIVE_PATH="${1:-}"

if [[ -z "$ARCHIVE_PATH" ]]; then
    BACKUPS_DIR="$VAULT_DIR/backups"
    if [[ ! -d "$BACKUPS_DIR" ]]; then
        echo "Error: backups directory not found at $BACKUPS_DIR" >&2
        exit 1
    fi
    ARCHIVE_PATH="$(find "$BACKUPS_DIR" -maxdepth 1 -type f -name '*.tar.gz.age' -printf '%T@\t%p\n' 2>/dev/null | sort -nr | head -n 1 | cut -f2- || true)"
fi

if [[ -z "$ARCHIVE_PATH" || ! -f "$ARCHIVE_PATH" ]]; then
    echo "Error: no .tar.gz.age encrypted backup archive found." >&2
    exit 1
fi

if [[ "$ARCHIVE_PATH" != *.age ]]; then
    echo "Refusing to upload unencrypted archive: $ARCHIVE_PATH" >&2
    exit 1
fi

ARCHIVE_NAME="$(basename "$ARCHIVE_PATH")"

# 1. Built-in log-journal sync (uses saved Google OAuth / token / rclone).
LOG_BIN=""
if command -v log-journal >/dev/null 2>&1; then
    LOG_BIN="$(command -v log-journal)"
elif [[ -x "$HOME/.local/bin/log-journal" ]]; then
    LOG_BIN="$HOME/.local/bin/log-journal"
elif [[ -x "$REPO_DIR/log-journal" ]]; then
    LOG_BIN="$REPO_DIR/log-journal"
fi

if [[ -n "$LOG_BIN" && -z "${1:-}" ]]; then
    if "$LOG_BIN" --vault "$VAULT_DIR" sync --folder "$FOLDER_NAME"; then
        exit 0
    fi
fi

# 2. rclone Google Drive remote.
if command -v rclone >/dev/null 2>&1; then
    RCLONE_REMOTE="${LOG_RCLONE_REMOTE:-}"
    if [[ -z "$RCLONE_REMOTE" ]]; then
        RCLONE_REMOTE="$(rclone listremotes --long 2>/dev/null | awk -F: '/:[[:space:]]*drive([[:space:]]|$)/ { gsub(/^[[:space:]]+|[[:space:]]+$/, "", $1); print $1; exit }')"
    fi
    RCLONE_REMOTE="${RCLONE_REMOTE%:}"
    if [[ -n "$RCLONE_REMOTE" ]]; then
        echo "Uploading $ARCHIVE_NAME to $RCLONE_REMOTE:$FOLDER_NAME/$ARCHIVE_NAME via rclone..."
        rclone copyto "$ARCHIVE_PATH" "$RCLONE_REMOTE:$FOLDER_NAME/$ARCHIVE_NAME"
        echo "Uploaded $ARCHIVE_NAME to $RCLONE_REMOTE:$FOLDER_NAME/$ARCHIVE_NAME."
        exit 0
    fi
fi

# 3. LOG_GDRIVE_TOKEN via Google Drive v3 REST API (curl + python3).
if [[ -n "${LOG_GDRIVE_TOKEN:-}" ]] && command -v curl >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    export FOLDER_NAME ARCHIVE_NAME
    FOLDER_QUERY="$(python3 -c 'import os, urllib.parse; n=os.environ["FOLDER_NAME"].replace("\\","\\\\").replace("'\''","\\'\''"); print(urllib.parse.quote(f"name = '\''{n}'\'' and mimeType = '\''application/vnd.google-apps.folder'\'' and trashed = false"))')"
    SEARCH_JSON="$(curl -fsSL -H "Authorization: Bearer $LOG_GDRIVE_TOKEN" "https://www.googleapis.com/drive/v3/files?q=${FOLDER_QUERY}&fields=files(id,name)&pageSize=10")"
    FOLDER_ID="$(python3 -c 'import json, os, sys; name=os.environ["FOLDER_NAME"]; files=json.load(sys.stdin).get("files",[]); print(next((f["id"] for f in files if f.get("name")==name), ""))' <<< "$SEARCH_JSON")"

    if [[ -z "$FOLDER_ID" ]]; then
        CREATE_PAYLOAD="$(python3 -c 'import json, os; print(json.dumps({"name": os.environ["FOLDER_NAME"], "mimeType": "application/vnd.google-apps.folder"}))')"
        CREATED_JSON="$(curl -fsSL -X POST -H "Authorization: Bearer $LOG_GDRIVE_TOKEN" -H "Content-Type: application/json" -d "$CREATE_PAYLOAD" "https://www.googleapis.com/drive/v3/files?fields=id,name")"
        FOLDER_ID="$(python3 -c 'import json, sys; print(json.load(sys.stdin).get("id",""))' <<< "$CREATED_JSON")"
    fi

    if [[ -z "$FOLDER_ID" ]]; then
        echo "Error: failed to resolve or create Google Drive folder '$FOLDER_NAME'." >&2
        exit 1
    fi

    export FOLDER_ID
    META_JSON="$(python3 -c 'import json, os; print(json.dumps({"name": os.environ["ARCHIVE_NAME"], "parents": [os.environ["FOLDER_ID"]]}))')"
    INIT_HEADERS="$(mktemp)"
    trap 'rm -f "$INIT_HEADERS"' EXIT
    FILE_SIZE="$(stat -c%s "$ARCHIVE_PATH")"
    curl -fsSL -D "$INIT_HEADERS" -o /dev/null -X POST \
        -H "Authorization: Bearer $LOG_GDRIVE_TOKEN" \
        -H "Content-Type: application/json; charset=UTF-8" \
        -H "X-Upload-Content-Type: application/octet-stream" \
        -H "X-Upload-Content-Length: $FILE_SIZE" \
        -d "$META_JSON" \
        "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id,name"
    UPLOAD_URL="$(awk 'BEGIN{IGNORECASE=1} /^Location:/ { sub(/\r$/, "", $2); print $2; exit }' "$INIT_HEADERS")"
    if [[ -z "$UPLOAD_URL" ]]; then
        echo "Error: Drive resumable upload did not return Location URL." >&2
        exit 1
    fi
    curl -fsSL -X PUT \
        -H "Content-Type: application/octet-stream" \
        --upload-file "$ARCHIVE_PATH" \
        "$UPLOAD_URL"
    echo ""
    exit 0
fi

# 4. Optional gdrive CLI fallback (without broken auth-status gate).
if [[ "${LOG_GDRIVE_DISABLE_CLI:-0}" != "1" ]] && command -v gdrive >/dev/null 2>&1; then
    FOLDER_JSON="$(gdrive readonly search --name-exact "$FOLDER_NAME" --type folder --json)"
    FOLDER_ID="$(python3 -c '
import json, sys
data = json.load(sys.stdin)
items = data if isinstance(data, list) else data.get("files", [])
print(items[0]["id"] if items else "")
' <<< "$FOLDER_JSON")"

    if [[ -z "$FOLDER_ID" ]]; then
        gdrive mutate mkdir "$FOLDER_NAME" >/dev/null
        FOLDER_JSON="$(gdrive readonly search --name-exact "$FOLDER_NAME" --type folder --json)"
        FOLDER_ID="$(python3 -c '
import json, sys
data = json.load(sys.stdin)
items = data if isinstance(data, list) else data.get("files", [])
print(items[0]["id"] if items else "")
' <<< "$FOLDER_JSON")"
    fi

    if [[ -z "$FOLDER_ID" ]]; then
        echo "Error: unable to resolve or create Google Drive folder '$FOLDER_NAME'." >&2
        exit 1
    fi

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

    gdrive mutate upload "$ARCHIVE_PATH" --parent "$FOLDER_ID" --mime-type application/octet-stream
    exit 0
fi

echo "No Google Drive transport configured." >&2
echo "Choose one:" >&2
echo "  1. Open Log -> Backup & Sync -> Connect Google Drive (OAuth, rclone, or token)." >&2
echo "  2. On Arch Linux: sudo pacman -S rclone && rclone config (add a Google Drive remote named 'logjournal')." >&2
echo "  3. Export LOG_GDRIVE_TOKEN with an OAuth bearer token and re-run this script." >&2
exit 1
