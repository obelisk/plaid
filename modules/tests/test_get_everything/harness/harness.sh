#!/bin/bash

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

sleep 2
# Call the webhook
curl -d "{}" -H "Authorization: Some Authorization Header" -H "my_secret: Secret from a header" http://$PLAID_LOCATION/webhook/$URL
sleep 2
curl "http://$PLAID_LOCATION/webhook/$URL?q=queryParameter&my_secret=secretFromQueryParam"
sleep 2

kill $RH_PID 2>&1 > /dev/null

echo -e "OK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
