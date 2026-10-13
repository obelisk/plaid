#!/bin/bash
set -e
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

# Wait for the request handler to bind its port before sending anything to
# Plaid: the rule makes a named request back to the handler, and if it isn't
# listening yet the rule fails with a connection refused error.
for _ in $(seq 1 100); do
  if (exec 3<>/dev/tcp/127.0.0.1/8998) 2>/dev/null; then exec 3>&-; break; fi
  sleep 0.1
done



# Call the webhook
OUTPUT=$(curl -XPOST -d 'slack_input' http://$PLAID_LOCATION/webhook/$URL)
sleep 2
kill $RH_PID 2>&1 > /dev/null

echo -e "OK\nOK\nOK\nOK\nOK" > expected.txt
diff expected.txt $FILE
RESULT=$?

rm -f $FILE expected.txt

exit $RESULT
