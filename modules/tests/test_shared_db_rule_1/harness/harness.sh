#!/bin/bash

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL1="testshareddb_1"
FILE="received_data.$URL1.txt"
URL2="testshareddb_2"

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
curl -d "read and fail to write" http://$PLAID_LOCATION/webhook/$URL1
sleep 2
curl -d "write and check" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "read and check 2 bytes" http://$PLAID_LOCATION/webhook/$URL1
sleep 2
curl -d "delete and check" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "read after deletion" http://$PLAID_LOCATION/webhook/$URL1
sleep 2
curl -d "insert batch and check" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "read after batch insert" http://$PLAID_LOCATION/webhook/$URL1
sleep 2
curl -d "delete batch inserts" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "fill up the db" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "write to full db" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "write to non-existing db" http://$PLAID_LOCATION/webhook/$URL2
sleep 2

stop_request_handler $RH_PID

echo -e "OK\nOK\nOK\nOK\nOK\nOK\nOK\nOK\nOK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
