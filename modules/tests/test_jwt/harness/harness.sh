#!/bin/bash

# Define what webhook within Plaid we're going to call
URL="test_jwt"
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

expected="OK - simple request (es256)
OK - simple request (rs256)
OK - add a field which is allowlisted (es256)
OK - add aud which is an enforced claim but doesn't match enforced value (rs256)
OK - add aud which is an enforced claim but matches enforced value (rs256)
OK - add a field which is an enforced claim, and is not allowlisted (rs256)
OK - add a field which is NOT allowlisted (es256)
OK - add a header which is allowlisted (es256)
OK - add a header which is NOT allowlisted (es256)
OK - use a key ID that we are not allowed to use (es256)
OK - use a key ID that does not exist (es256)
OK - make a request without exp (es256)"
echo -e "$expected" > expected.txt

if [ "$DEBUG" = "true" ]; then
  echo "Expected"
  echo "--------"
  cat expected.txt
  echo ""
  echo "Received"
  echo "--------"
  cat $FILE
  echo ""
fi

diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
