#!/bin/bash

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL="testmnr"
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
curl -d '{}' http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 3

stop_request_handler $RH_PID

echo -e "OK from /testmnr\nOK from /testmnr/my_variable\nOK from /testmnr/headers" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt
exit $RESULT
