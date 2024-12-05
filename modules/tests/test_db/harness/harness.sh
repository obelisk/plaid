#!/bin/bash

# Define what webhook within Plaid we're going to call
URL="testdb"
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
curl http://$PLAID_LOCATION/webhook/$URL?key=some_key
curl -d "first_value" http://$PLAID_LOCATION/webhook/$URL
curl http://$PLAID_LOCATION/webhook/$URL?key=some_key
curl http://$PLAID_LOCATION/webhook/$URL?key=my_key
curl -d "second_value" http://$PLAID_LOCATION/webhook/$URL
curl http://$PLAID_LOCATION/webhook/$URL?key=some_key
curl http://$PLAID_LOCATION/webhook/$URL?key=my_key

kill $RH_PID 2>&1 > /dev/null

# The response from the webhook should be the following:
# Empty
# Empty
# first_value
# Empty
# second_value

echo -e "Empty\nEmpty\nfirst_value\nEmpty\nsecond_value" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
