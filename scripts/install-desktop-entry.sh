#!/usr/bin/env bash
set -euo pipefail

# Convenience wrapper delegating to scripts/install.sh --skip-build.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/install.sh" --skip-build "$@"
