#!/usr/bin/env bash
# Refresh cached read-only Google Calendar events for Log's Daily Context Ribbon.
# Writes atomically to ~/.cache/log-journal/today-events.json.
#
# Usage:
#   scripts/refresh-calendar-context.sh [YYYY-MM-DD]

set -euo pipefail

TARGET_DATE="${1:-$(date +%Y-%m-%d)}"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/log-journal"
CACHE_FILE="${LOG_CALENDAR_CACHE_PATH:-$CACHE_DIR/today-events.json}"

mkdir -p "$(dirname "$CACHE_FILE")"

if ! command -v gcalendar >/dev/null 2>&1; then
    echo "Warning: gcalendar CLI not found in PATH; skipping calendar cache refresh." >&2
    exit 0
fi

TMP_FILE="$(mktemp "$(dirname "$CACHE_FILE")/.today-events.XXXXXX.json")"
cleanup() {
    rm -f "$TMP_FILE"
}
trap cleanup EXIT

if gcalendar readonly events --date "$TARGET_DATE" --max 50 --json > "$TMP_FILE"; then
    if python3 -c 'import json, sys; json.load(open(sys.argv[1]))' "$TMP_FILE" >/dev/null 2>&1; then
        mv "$TMP_FILE" "$CACHE_FILE"
        echo "Updated calendar context cache for $TARGET_DATE -> $CACHE_FILE"
        exit 0
    fi
fi

echo "Warning: gcalendar query failed or returned invalid JSON; leaving existing cache untouched." >&2
exit 0
