#!/usr/bin/env bash
#
# Deprecated alias for stamp.sh, kept so existing invocations keep working.
# It forwards every argument unchanged. Switch to scripts/stamp.sh; this
# wrapper will be removed in a later release.
#
set -euo pipefail

echo "[enrich.sh] warning: renamed to stamp.sh. Use scripts/stamp.sh; this alias will be removed." >&2

exec "$(dirname "$0")/stamp.sh" "$@"
