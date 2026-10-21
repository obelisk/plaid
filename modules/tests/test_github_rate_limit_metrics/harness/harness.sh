#!/bin/bash

URL="test_github_rate_limit_metrics"

curl -s -d '{}' "http://$PLAID_LOCATION/webhook/$URL"
sleep 3

if ! curl -s http://127.0.0.1:9464/metrics | grep -F 'plaid_github_api_rate_limit_remaining{client="noauth",resource="core"}'; then
  echo "Missing metric for the octocrab client"
  exit 1
fi

exit 0
