#!/bin/bash

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL="testlogback"
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

# Call the webhook. The rule replies with the current cache value ("0") and then
# schedules a log back with a 3 second delay.
curl http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 1 30

# Wait for the log back to be delivered. It is not observable from here, so this
# has to be a fixed wait, and it must be long enough that the log back is
# consumed before this harness exits: the delayed-logback processor polls on an
# interval (10s by default), so a 3s delay can take up to ~13s to be processed.
# If the harness exits first, the log back fires during a later test and its
# output lands in that test's file, failing it.
sleep 20

# Call the webhook
curl http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 2 30

stop_request_handler $RH_PID

echo -e "0\n1" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
