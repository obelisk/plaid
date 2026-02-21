#!/usr/bin/env bash
# Send a test webhook to the local plaid instance.
set -euo pipefail

HOST="${PLAID_HOST:-localhost:8080}"
ENDPOINT="${1:-hello}"
PAYLOAD="${2:-{"message": "hello from local dev"}}"

echo "POST http://$HOST/webhook/$ENDPOINT"
echo "Payload: $PAYLOAD"
echo "---"

curl -s -w "\nHTTP %{http_code}\n" \
    -X POST \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD" \
    "http://$HOST/webhook/$ENDPOINT"
