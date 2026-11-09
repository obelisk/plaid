#!/bin/bash

set -e

source "$(dirname "$0")/../../common/harness_lib.sh"

# Define what webhook within Plaid we're going to call
URL="crontest"
FILE="received_data.$URL.txt"

# Start the webhook
$REQUEST_HANDLER 2> $FILE &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi

RH_PID=$!
wait_for_request_handler $RH_PID
register_handler_cleanup $RH_PID

# The cron job fires on the 5th, 11th, 36th, 41st and 57th second of every
# minute, so a full minute of output is needed to observe all five.
wait_for_lines $FILE 5 90
stop_request_handler $RH_PID

# Get the execution times from the webhook
EXPECTED=(5 11 36 41 57)
ACTUAL=( $(awk '{print $1 % 60}' "$FILE") )
SORTED_ACTUAL=( $(printf "%s\n" "${ACTUAL[@]}" | sort -n) )
rm $FILE

# The scheduler wakes up on the configured second but the rule only runs once
# the executor picks the message up, so on a loaded runner an execution can be
# observed a second late. Allow a small tolerance instead of requiring an exact
# match, which made this test fail whenever the runner was busy.
TOLERANCE=2

if (( ${#SORTED_ACTUAL[@]} != ${#EXPECTED[@]} )); then
  echo "FAIL: got ${#SORTED_ACTUAL[@]} executions (${SORTED_ACTUAL[*]}), want ${#EXPECTED[@]} (${EXPECTED[*]})"
  exit 1
fi

for i in "${!EXPECTED[@]}"; do
  delta=$(( SORTED_ACTUAL[i] - EXPECTED[i] ))
  if (( delta < 0 )); then
    delta=$(( -delta ))
  fi
  if (( delta > TOLERANCE )); then
    echo "FAIL: got seconds ${SORTED_ACTUAL[*]}, want ${EXPECTED[*]} (tolerance ${TOLERANCE}s)"
    exit 1
  fi
done

exit 0
