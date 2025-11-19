#!/bin/bash

# Define what webhook within Plaid we're going to call
URL="test_mnr_return_certs"
FILE="received_data.$URL.txt"

# Start the webhook
$REQUEST_HANDLER > $FILE &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi

RH_PID=$!
sleep 5  # give time to the warp server to start

# Call the webhook
curl -d '{}' http://$PLAID_LOCATION/webhook/$URL
sleep 5

kill $RH_PID 2>&1 > /dev/null

echo -e "OK from /testmnr\nOK from /testmnr/my_variable\nOK from /testmnr/headers" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt
exit $RESULT
