#!/bin/bash

set -e

# Define what webhook within Plaid we're going to call
URL="dynamodbtest"
FILE="received_data.$URL.txt"

# Start the webhook
$REQUEST_HANDLER > $FILE &
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

