#!/bin/bash
set -e

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL="test_aes_enc_dec"
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
curl -d "{}" http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 4

stop_request_handler $RH_PID

echo -e "OK\nOK\nOK\nOK" > expected.txt
diff expected.txt $FILE || RESULT=$?
RESULT=${RESULT:-0}

rm -f $FILE expected.txt

exit $RESULT
