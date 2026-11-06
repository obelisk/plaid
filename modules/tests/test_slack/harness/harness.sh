#!/bin/bash
set -e

source "$(dirname "$0")/../../common/harness_lib.sh"

# If GITHUB_ACTIONS is not set, skip because Plaid won't be running
# with the Slack API properly configured
if [ -z "$GITHUB_ACTIONS" ]; then
  echo "Not running in GitHub Actions, skipping Slack tests"
  exit 0
fi

if [ -z "$SLACK_TEST_WEBHOOK" ] || [ -z "$SLACK_TEST_BOT_TOKEN" ]; then
  echo "Slack secrets are not available, skipping Slack tests"
  exit 0
fi

URL="testslack"
FILE="received_data.$URL.txt"

# Start the webhook
$REQUEST_HANDLER > $FILE &
if [ $? -ne 0 ]; then
  echo "SlackTest: Failed to start request handler"
  rm $FILE
  exit 1
fi

RH_PID=$!
wait_for_request_handler $RH_PID

# Call the webhook
OUTPUT=$(curl -XPOST -d 'slack_input' http://$PLAID_LOCATION/webhook/$URL)
wait_for_lines $FILE 5 60
stop_request_handler $RH_PID

echo -e "OK\nOK\nOK\nOK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
