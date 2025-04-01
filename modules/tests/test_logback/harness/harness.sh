#!/bin/bash

# Define what webhook within Plaid we're going to call
URL="testlogback"
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
curl http://$PLAID_LOCATION/webhook/$URL
sleep 1

# Wait for the log back to arrive
sleep 20

# Call the webhook
curl http://$PLAID_LOCATION/webhook/$URL
sleep 3

kill $RH_PID 2>&1 > /dev/null

echo -e "0\n1" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
