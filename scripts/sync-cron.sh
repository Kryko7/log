#!/usr/bin/env bash
# Cron / systemd timer wrapper: refresh calendar context, encrypt vault with age,
# and optionally sync to Google Drive ("LogJournal" folder).
#
# Usage:
#   LOG_BACKUP_PASSPHRASE_FILE=~/.config/log/backup.pass scripts/sync-cron.sh [--upload-gdrive]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

"$SCRIPT_DIR/refresh-calendar-context.sh" || true
exec "$SCRIPT_DIR/nightly-backup.sh" "$@"
