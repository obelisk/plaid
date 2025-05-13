#!/bin/bash

set -e

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

sleep 60
kill $RH_PID 2>&1 > /dev/null

# Get the execution times from the webhook
EXPECTED=(5 11 36 41 57)
ACTUAL=( $(awk '{print $1 % 60}' "$FILE") )
SORTED_ACTUAL=( $(printf "%s\n" "${ACTUAL[@]}" | sort -n) )
rm $FILE

if [[ "${SORTED_ACTUAL[*]}" != "${EXPECTED[*]}" ]]; then
  echo "FAIL: got seconds ${SORTED_ACTUAL[*]}, want ${EXPECTED[*]}"
  exit 1
fi

exit 0