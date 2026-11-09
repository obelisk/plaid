#!/bin/bash

set -e

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL="timetest"
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
register_handler_cleanup $RH_PID

# Call the webhook
curl -X POST -H "Content-Type: application/json" -d '' http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 1
stop_request_handler $RH_PID

# Get the response time from the webhook
RETURNED_TIME=$(cat $FILE | tail -1)
CURRENT_TIME=$(date +%s)
DIFF="$(($CURRENT_TIME-$RETURNED_TIME))"

rm $FILE

# Check if the response contains the received data
if [[ $DIFF == 0 || $DIFF == 1 || $DIFF == 2 ]]; then
  exit 0
else
  echo "Time test failed because time is off by too much"
  exit 1
fi
