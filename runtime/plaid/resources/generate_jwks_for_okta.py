# This script generates a key pair that can be used for authenticating an Okta app to the Okta service.
#
# The script creates a directory `keys` and three (3) files in this directory:
# 1. The private key in JSON format
# 2. The private key in PEM format
# 3. The public key in JSON format
#
# The content of file n.3 (pub key in JSON format) should be pasted into Okta admin console when registering
# a new application.
#
# The content of file n.2 (private key in PEM format) should be pasted in Plaid's config to allow Plaid to
# authenticate to the Okta service.
#
# In the end, Plaid's config should look like this:
# [apis.okta.authentication]
# client_id = "OKTA APP'S CLIENT ID"
# private_key = """
# -----BEGIN PRIVATE KEY-----
# ...
# -----END PRIVATE KEY-----
# """
#
#
# Recommended way to use the script:
#
# python -m venv venv
# . ./venv/bin/activate
# pip install -U pip jwcrypto
# python <this_file>.py


import os
from jwcrypto import jwk

key_name = "plaid_service_app_keys"  # change as required
key_type = "RSA"
alg = "RSA256"
size = 4096
use = "sig"


def create_keys(key_name):
    """Create all of the keys and save in keys directory"""
    key = jwk.JWK.generate(kty=key_type, size=size, kid=key_name, use=use, alg=alg)

    with open(f"keys/{key_name}_private.json", "w") as writer:
        writer.write(key.export_private())

    with open(f"keys/{key_name}_public.json", "w") as writer:
        writer.write(key.export_public())

    with open(f"keys/{key_name}.pem", "w") as writer:
        writer.write(key.export_to_pem(private_key=True, password=None).decode("utf-8"))


if not os.path.exists("keys"):
    os.makedirs("keys")
    create_keys(key_name=key_name)
    print("Keys created. Please move to secure storage and remove the keys directory.")
else:
    print(
        "Please remove existing keys directory - make sure you have the existing keys stored securely because this will generate new ones!"
    )
