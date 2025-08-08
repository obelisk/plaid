#!/bin/bash

# Define what webhook within Plaid we're going to call
URL1="multilogtype1"
URL2="multilogtype2"
URL3="multilogtype3"
FILE="received_data.multilogtype.txt"

# Start the webhook
$REQUEST_HANDLER > $FILE &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi

RH_PID=$!

# Call the webhook
curl -d '{"type": "multilogtype1"}' http://$PLAID_LOCATION/webhook/$URL1
sleep 2
curl -d '{"type": "multilogtype2"}' http://$PLAID_LOCATION/webhook/$URL2
sleep 2
curl -d '{"type": "multilogtype3"}' http://$PLAID_LOCATION/webhook/$URL3

kill $RH_PID 2>&1 > /dev/null
sleep 2

# The rule we are testing handles types 1 and 2, but not 3
echo -e "multilogtype1\nmultilogtype2" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
