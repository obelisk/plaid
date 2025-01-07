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
curl -d "get:some_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "insert:my_key:first_value" http://$PLAID_LOCATION/webhook/$URL
curl -d "get:some_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "get:my_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "insert:my_key:second_value" http://$PLAID_LOCATION/webhook/$URL
curl -d "get:some_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "get:my_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "delete:my_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "get:my_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "delete:another_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "get:another_key" http://$PLAID_LOCATION/webhook/$URL
# At this point the DB is empty
curl -d "insert:my_key:first_value" http://$PLAID_LOCATION/webhook/$URL
curl -d "insert:a_key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" http://$PLAID_LOCATION/webhook/$URL # too many bytes for the configured storage limit
curl -d "get:a_key" http://$PLAID_LOCATION/webhook/$URL # Empty because insertion failed
curl -d "insert:a_key:a" http://$PLAID_LOCATION/webhook/$URL # this is within the limit, so it's fine
curl -d "get:a_key" http://$PLAID_LOCATION/webhook/$URL # a
curl -d "delete:my_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "delete:a_key" http://$PLAID_LOCATION/webhook/$URL
# now the DB is empty, so we can insert the long key/value pair
curl -d "insert:a_key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" http://$PLAID_LOCATION/webhook/$URL
curl -d "get:a_key" http://$PLAID_LOCATION/webhook/$URL

sleep 2

kill $RH_PID 2>&1 > /dev/null

# The response from the webhook should be the following:
# Empty
# Empty
# first_value
# Empty
# second_value
# Empty
# Empty
# Empty
# a
# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa

echo -e "Empty\nEmpty\nfirst_value\nEmpty\nsecond_value\nEmpty\nEmpty\nEmpty\na\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
