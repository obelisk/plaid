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

# The log back is what populates the cache with "1", so the second call can only
# return "1" once it has been delivered. The delayed-logback processor polls on
# an interval (10s by default), so the log back can arrive well after the 3s
# delay the rule asked for. Wait for the marker the rule emits when it handles
# the log back rather than guessing a duration.
wait_for_line "$FILE" "LOGBACK" 60

# Call the webhook
curl http://$PLAID_LOCATION/webhook/$URL
wait_for_lines $FILE 3 30

stop_request_handler $RH_PID

echo -e "0\nLOGBACK\n1" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
