#!/usr/bin/env bash
# Watch a directory for .wasm file changes and restart the plaid container.
# Runs on the host (not inside Docker).
#
# Usage:
#   ./scripts/watch-modules.sh [directory]
#
# Default directory: ./compiled_modules
#
# Requires: inotifywait (from inotify-tools) or falls back to polling.

set -euo pipefail

WATCH_DIR="${1:-./compiled_modules}"
COMPOSE_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if [ ! -d "$WATCH_DIR" ]; then
    echo "Directory $WATCH_DIR does not exist. Create it and add .wasm files first."
    exit 1
fi

restart_plaid() {
    echo "[$(date '+%H:%M:%S')] Change detected — restarting plaid..."
    docker compose -C "$COMPOSE_DIR" restart plaid
}

# Try inotifywait first (efficient, event-based)
if command -v inotifywait &>/dev/null; then
    echo "Watching $WATCH_DIR for .wasm changes (inotifywait)..."
    echo "Press Ctrl+C to stop."
    while inotifywait -q -e close_write,moved_to,create --include '\.wasm$' "$WATCH_DIR"; do
        restart_plaid
    done
else
    # Fallback: poll every 2 seconds
    echo "inotifywait not found — falling back to polling (install inotify-tools for efficiency)."
    echo "Watching $WATCH_DIR for .wasm changes (polling every 2s)..."
    echo "Press Ctrl+C to stop."

    last_hash=""
    while true; do
        current_hash=$(find "$WATCH_DIR" -name '*.wasm' -exec stat -c '%Y %n' {} + 2>/dev/null | sort | md5sum)
        if [ "$current_hash" != "$last_hash" ] && [ -n "$last_hash" ]; then
            restart_plaid
        fi
        last_hash="$current_hash"
        sleep 2
    done
fi
