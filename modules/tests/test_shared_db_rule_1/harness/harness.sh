#!/bin/bash

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

# Call the webhook
sleep 2
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
curl -d "fill up the db" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "write to full db" http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d "write to non-existing db" http://$PLAID_LOCATION/webhook/$URL2
sleep 2

kill $RH_PID 2>&1 > /dev/null

echo -e "OK\nOK\nOK\nOK\nOK\nOK\nOK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
