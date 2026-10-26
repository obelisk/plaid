#!/bin/bash

# Requires egress to api.github.com. The check is this client's request
# counter, not the quota gauge: earlier modules in the suite already call
# GitHub through the same client and set the gauges. Rate-limited and failed
# HTTP responses still increment the counter, so a shared-IP quota exhaust does
# not flake this check. Transport failures are excluded, because a blocked
# network must not satisfy a check about observed responses.

URL="test_github_rate_limit_metrics"
SERIES_PREFIX='plaid_github_api_requests_total{client="noauth",method="GET",'
NO_RESPONSE='outcome="transport_error"'

requests() {
  curl -s http://127.0.0.1:9464/metrics |
    awk -v prefix="$SERIES_PREFIX" -v no_response="$NO_RESPONSE" \
      'index($1, prefix) == 1 && !index($1, no_response) { total += $2 } END { print total + 0 }'
}

before=$(requests)
curl -s -d '{}' "http://$PLAID_LOCATION/webhook/$URL"

for _ in $(seq 1 15); do
  if [ "$(requests)" -gt "$before" ]; then
    exit 0
  fi
  sleep 2
done

echo "plaid_github_api_requests_total did not move for this client"
exit 1
