#!/bin/bash

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

# Call the webhook
curl -d "{}" http://$PLAID_LOCATION/webhook/$URL
sleep 2

kill $RH_PID 2>&1 > /dev/null

echo -e "OK\nOK\nOK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
