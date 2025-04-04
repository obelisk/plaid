#!/bin/bash

set -e

# Define what webhook within Plaid we're going to call
URL="testregex"
DATA="apr3tty-standard_email@gmail.com"

# Call the webhook
OUTPUT=$(curl -XPOST http://$PLAID_LOCATION/webhook/$URL?signature=$SIGNATURE\&data=$DATA -d "$DATA")

exit 0