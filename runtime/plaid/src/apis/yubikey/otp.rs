use super::{Yubikey, YubikeyError};

use crate::apis::ApiError;

use std::collections::BTreeMap;

use ring::{hmac, rand};

const YUBICLOUD_VERIFY: &str = "https://api.yubico.com/wsapi/2.0/verify";

fn hex_encode<T: AsRef<[u8]>>(data: T) -> String {
    data.as_ref()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

impl Yubikey {
    pub async fn verify_otp(&self, otp: &str, _: &str) -> Result<String, ApiError> {
        // Generate a random nonce to validate the OTP with
        let nonce: [u8; 16] = rand::generate(&self.rng)
            .map_err(|_| ApiError::YubikeyError(YubikeyError::RandError))?
            .expose();
        let nonce = hex_encode(nonce);

        // Build the request string
        let request = format!("id={}&nonce={nonce}&otp={otp}", &self.config.client_id);

        // Generate the signature over the request and base64 encode it
        let signature = hmac::sign(&self.key, request.as_bytes());
        let signature = base64::encode_config(signature.as_ref(), base64::STANDARD);

        // Finish the request by appending the signature
        let signed_request = format!("{request}&h={signature}");

        // Make the request to the server and wait for the response
        let res = self
            .client
            .get(format!("{YUBICLOUD_VERIFY}?{signed_request}"))
            .send()
            .await
            .map_err(|_| ApiError::YubikeyError(YubikeyError::NetworkError))?;

        // Fetch the response data
        let data = res
            .text()
            .await
            .map_err(|_| ApiError::YubikeyError(YubikeyError::NoData))?;

        // Sort the returned parts using a BTree because a signature is over the
        // components in alphabetical order
        let mut response_items = BTreeMap::new();
        for line in data.lines() {
            let param: Vec<&str> = line.splitn(2, '=').collect();
            if param.len() > 1 {
                response_items.insert(param[0].to_string(), param[1].to_string());
            }
        }

        // Remove the signature field
        let signature = response_items
            .remove("h")
            .ok_or(ApiError::YubikeyError(YubikeyError::NoSignature))?;

        // Rebuild the signed response
        let mut signed_data = String::new();
        for (key, value) in response_items.iter() {
            let param = format!("{}={}&", key, value);
            signed_data.push_str(param.as_ref());
        }
        signed_data.pop();

        // Validate the signature matches what we calculated
        if let Err(_) = hmac::verify(
            &self.key,
            signed_data.as_bytes(),
            &base64::decode(signature)
                .map_err(|_| ApiError::YubikeyError(YubikeyError::BadData))?,
        ) {
            error!("Could not verify the signature from Yubico!");
            return Err(ApiError::YubikeyError(YubikeyError::BadSignature));
        };

        // Finally look at the status
        let status = response_items
            .get("status")
            .ok_or(ApiError::YubikeyError(YubikeyError::NoStatus))?;

        // If the response is ok, we don't return anything, otherwise we will provide
        // limited data as to why it failed.
        match status.as_str() {
            "OK" => Ok("OK".to_string()),
            "BAD_OTP" => Ok("BAD_OTP".to_string()),
            "REPLAYED_OTP" => Ok("REPLAYED_OTP".to_string()),
            other => {
                error!("Received {other} error from Yubico");
                Ok(format!("{other}"))
            }
        }
    }
}
