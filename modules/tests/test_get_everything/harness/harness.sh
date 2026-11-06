#!/bin/bash

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL="testgeteverything"
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
curl -d "{}" -H "Authorization: Some Authorization Header" -H "my_secret: Secret from a header" http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 1
curl "http://$PLAID_LOCATION/webhook/$URL?q=queryParameter&my_secret=secretFromQueryParam"
wait_for_lines $FILE 2

stop_request_handler $RH_PID

echo -e "OK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
