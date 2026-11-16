#!/bin/bash
set -e

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL="test_redis_cache_eviction"
FILE="received_data.$URL.txt"

# Start the webhook
$REQUEST_HANDLER > $FILE &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi

RH_PID=$!
wait_for_request_handler $RH_PID

# Call the webhook
curl -d "{}" http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 1

stop_request_handler $RH_PID

RESULT=$(head -n 1 $FILE)
rm -f $FILE

if [[ $RESULT == "OK" ]]; then
    exit 0
else
    exit 1
fi
