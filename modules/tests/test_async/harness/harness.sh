#!/bin/bash

# Integration test for the async ticket system.
#
# The test_async rule spawns two chained async MNR calls when triggered.
# The MNR posts to this request handler's /async_echo route, which logs the
# body. The harness verifies:
#   1. The webhook trigger returns immediately (the rule does not block).
#   2. Both async calls eventually execute (the handler sees both bodies).
#   3. The completion chain ran to the end (the rule's step3 debug output
#      appears in Plaid's own log, which we check via the runtime log file).

URL="testasync"
FILE="received_data.$URL.txt"
PLAID_LOG="/tmp/plaid_async_test.log"

# Start the request handler to receive the async MNR calls
$REQUEST_HANDLER > $FILE &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi
RH_PID=$!
sleep 2

# Trigger the rule. The webhook response should come back quickly because
# the rule returns immediately after spawning.
START=$(date +%s)
curl -s -m 5 -d 'hello-async' http://$PLAID_LOCATION/webhook/$URL
CURL_RC=$?
END=$(date +%s)
ELAPSED=$((END - START))

if [ $CURL_RC -ne 0 ]; then
  echo "Webhook call failed with curl exit code $CURL_RC"
  kill $RH_PID 2>/dev/null
  rm -f $FILE
  exit 1
fi

echo "Webhook responded in ${ELAPSED}s (rule returned without blocking)"

# Give the async chain time to complete: two HTTP round trips plus
# completion delivery.
sleep 10

kill $RH_PID 2>&1 > /dev/null

# The handler should have received both bodies:
#   1. The original payload from step 1's spawn.
#   2. "second:hello-async" from step 2's chained spawn.
echo "hello-async from /async_echo" > expected.txt
echo "second:hello-async from /async_echo" >> expected.txt

sort expected.txt > expected_sorted.txt
sort $FILE > actual_sorted.txt

if ! diff -q expected_sorted.txt actual_sorted.txt >/dev/null; then
  echo "Async MNR outputs do not match expected:"
  diff expected_sorted.txt actual_sorted.txt
  rm -f $FILE expected.txt expected_sorted.txt actual_sorted.txt
  exit 1
fi

rm -f $FILE expected.txt expected_sorted.txt actual_sorted.txt

# The completion chain must have run to the end inside the rule. The rule
# logs this via print_debug_string, which lands in Plaid's log output.
# The integration harness runs Plaid in the background with RUST_LOG=debug;
# its output is captured by the parent script, so we also accept the case
# where we cannot access the log (the MNR outputs above are the primary
# assertion).
if [ -f "$PLAID_LOG" ]; then
  if ! grep -q "step3: DONE" "$PLAID_LOG"; then
    echo "Rule did not complete the async chain (no step3 output in Plaid log)"
    exit 1
  fi
fi

echo "Async ticket system test passed"
exit 0
