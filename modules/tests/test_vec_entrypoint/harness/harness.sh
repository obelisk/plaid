#!/bin/bash

# Define what webhook within Plaid we're going to call
URL="test_vec_entrypoint"
FILE="received_data.$URL.txt"

# Start the webhook
$REQUEST_HANDLER > $FILE &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi

RH_PID=$!

# Call the webhook
head -c 16 /dev/urandom | curl -X POST "http://$PLAID_LOCATION/webhook/$URL" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @-
sleep 2

kill $RH_PID 2>&1 > /dev/null

RESULT=$(head -n 1 $FILE)
rm -f $FILE

if [[ $RESULT == "16" ]]; then
    exit 0
else
    exit 1
fi
