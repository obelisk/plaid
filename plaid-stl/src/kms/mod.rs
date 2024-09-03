use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

/// Represents the response from a sign request, including the key ID, signature, and signing algorithm used.
#[derive(Deserialize)]
pub struct SignRequestResponse {
    /// The Amazon Resource Name (key ARN) of the asymmetric KMS key that was used to sign the message.
    pub key_id: Option<String>,
    /// The cryptographic signature that was generated for the message.
    ///
    /// - When used with the supported RSA signing algorithms, the encoding of this value is defined by PKCS #1 in [RFC 8017](https://tools.ietf.org/html/rfc8017).
    /// - When used with the `ECDSA_SHA_256`, `ECDSA_SHA_384`, or `ECDSA_SHA_512` signing algorithms, this value is a DER-encoded object as defined by ANSI X9.62–2005 and [RFC 3279 Section 2.2.3](https://tools.ietf.org/html/rfc3279#section-2.2.3). This is the most commonly used signature format and is appropriate for most uses.
    ///
    /// When you use the HTTP API or the Amazon Web Services CLI, the value is Base64-encoded. Otherwise, it is not Base64-encoded.
    pub signature: Option<Vec<u8>>,
    /// The signing algorithm that was used to sign the message.
    pub signing_algorithm: Option<String>,
}

/// Tells KMS whether the value of the Message parameter should be hashed as part of the signing algorithm.
/// Use `Raw` for unhashed messages; use `Digest` for message digests, which are already hashed.
#[derive(Serialize)]
pub enum MessageType {
    Raw,
    Digest,
}

/// Creates and sends a named signing request to the KMS API.
///
/// This function:
/// - Constructs a `SignRequestRequest` with the provided request name, message, and message type.
/// - Serializes the request into a JSON string.
/// - Sends the request to the KMS API using a host function, handling the response in a buffer.
/// - Deserializes the response into a `SignRequestResponse`, which includes details like the key ID, signature, and signing algorithm used.
///
/// # Parameters
/// - `request_name`: The name of the signing request as defined in the configuration (e.g., `plaid.toml`).
/// - `message`: The message or message digest to be signed by KMS.
/// - `message_type`: Specifies whether the message should be hashed (use `Raw` for unhashed, `Digest` for pre-hashed).
///
/// # Returns
/// A `Result` containing either the `SignRequestResponse` with the signing details or a `PlaidFunctionError` if something went wrong.
pub fn make_named_signing_request(
    request_name: &str,
    message: &str,
    message_type: MessageType,
) -> Result<SignRequestResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(kms, make_named_signing_request);
    }

    #[derive(Serialize)]
    struct SignRequestRequest {
        /// Specifies the message or message digest to sign. Messages can be 0-4096 bytes. To sign a larger message, provide a message digest.
        message: String,
        /// Name of the request - defined in plaid.toml
        request_name: String,
        /// Tells KMS whether the value of the Message parameter should be hashed as part of the signing algorithm.
        /// Use RAW for unhashed messages; use DIGEST for message digests, which are already hashed.
        message_type: MessageType,
    }

    let request = SignRequestRequest {
        request_name: request_name.to_owned(),
        message: message.to_owned(),
        message_type,
    };

    let request = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        kms_make_named_signing_request(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match serde_json::from_slice(&return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}
