#!/usr/bin/env bash
# Exercise the local PostgreSQL API from webhook to database and back.
set -euo pipefail

HOST="${PLAID_HOST:-localhost:8080}"
PAYLOAD='{"minimum_id":1,"active":true}'
EXPECTED='{"columns":[{"name":"id","postgres_type":"int8"},{"name":"name","postgres_type":"text"},{"name":"active","postgres_type":"bool"},{"name":"profile","postgres_type":"jsonb"}],"rows":[[1,"Ada Lovelace",true,{"team":"security"}],[3,"Linus Torvalds",true,{"team":"infrastructure"}]]}'

echo "POST http://$HOST/webhook/postgres"
curl \
    --silent \
    --show-error \
    --fail-with-body \
    --retry 30 \
    --retry-connrefused \
    --retry-delay 1 \
    -X POST \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD" \
    --output /dev/null \
    "http://$HOST/webhook/postgres"

response=""
for _ in $(seq 1 30); do
    response="$(curl --silent --show-error --fail-with-body "http://$HOST/webhook/postgres")"
    if [[ "$response" == "$EXPECTED" ]]; then
        break
    fi
    sleep 0.2
done

if [[ "$response" != "$EXPECTED" ]]; then
    echo "Unexpected PostgreSQL example response" >&2
    echo "Actual:   $response" >&2
    echo "Expected: $EXPECTED" >&2
    exit 1
fi

echo "$response"
echo "PostgreSQL end-to-end smoke test passed"
