#!/bin/bash

set -e

# Define what webhook within Plaid we're going to call
URL="persistentresponsetest"
NEEDLE="quereydatatestneedle"

# Call the webhook
OUTPUT=$(curl -XGET http://$PLAID_LOCATION/webhook/$URL?querydata=$NEEDLE)

if [[ $OUTPUT == *"$NEEDLE"* ]]; then
  exit 0
fi

exit 1