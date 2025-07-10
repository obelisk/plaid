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
curl -d "get:some_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "insert:my_key:first_value" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "get:some_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "get:my_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "insert:my_key:second_value" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "get:some_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "get:my_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete:my_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "get:my_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete:another_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "get:another_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# At this point the DB is empty
curl -d "insert:my_key:first_value" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# too many bytes for the configured storage limit
curl -d "insert:a_key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# empty because insertion failed
curl -d "get:a_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# this is within the limit, so it's fine
curl -d "insert:a_key:a" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# returns "a"
curl -d "get:a_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete:my_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete:a_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# now the DB is empty, so we can insert the long key/value pair
curl -d "insert:a_key:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "get:a_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete:a_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# the DB is empty
curl -d "insert:my_key:a" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "insert:my_new_key:b" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "insert:a_key:c" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "list_keys:all:my_key|my_new_key|a_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "list_keys:prefix:my:my_key|my_new_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete:my_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "delete:my_new_key" http://$PLAID_LOCATION/webhook/$URL
curl -d "delete:a_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# the DB is empty
curl -d "insert:some_key:a" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "insert_check_returned_data:some_key:a" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete:some_key" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
# the DB is empty
curl -d "insert:some_key:a" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete_check_returned_data:some_key:a" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "insert:some_key:some_value" http://$PLAID_LOCATION/webhook/$URL; sleep 0.5
curl -d "delete_check_returned_data:some_key:some_value" http://$PLAID_LOCATION/webhook/$URL
# the DB is empty

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
# OK
# OK
# OK
# OK
# OK

echo -e "Empty\nEmpty\nfirst_value\nEmpty\nsecond_value\nEmpty\nEmpty\nEmpty\na\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nOK\nOK\nOK\nOK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
