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

# Wait for the request handler to bind its port before sending anything to
# Plaid: the rule makes a named request back to the handler, and if it isn't
# listening yet the rule fails with a connection refused error.
for _ in $(seq 1 100); do
  if (exec 3<>/dev/tcp/127.0.0.1/8998) 2>/dev/null; then exec 3>&-; break; fi
  sleep 0.1
done

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
