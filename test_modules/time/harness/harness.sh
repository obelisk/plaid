#!/bin/bash

set -e

# Define what webhook within Plaid we're going to call
URL="timetest"
FILE="received_data.$URL.txt"

# Start the webhook
./target/debug/request_handler > $FILE &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi

RH_PID=$!

sleep 2
# Call the webhook
curl -X POST -H "Content-Type: application/json" -d '' http://$PLAID_LOCATION/webhook/$URL
sleep 1
kill $RH_PID 2>&1 > /dev/null

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