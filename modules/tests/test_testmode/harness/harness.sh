#!/bin/bash

set -e

# Define what webhook within Plaid we're going to call
URL="testmode"

# Call the webhook
OUTPUT=$(curl -XPOST -d 'crash' http://$PLAID_LOCATION/webhook/$URL)
OUTPUT=$(curl -XPOST -d 'memory_pressure' http://$PLAID_LOCATION/webhook/$URL)

# There is no defined success critieria here but it's important
# to keep track of what Plaid does as the compiler's optimizations
# change with time
exit 0