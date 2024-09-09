use std::collections::HashMap;

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

/// Contains metadata about a KMS key
#[derive(Deserialize)]
pub struct GetPublicKeyResponse {
    /// The Amazon Resource Name (key ARN) of the asymmetric KMS key that was used to sign the message.
    pub key_id: Option<String>,
    /// The exported public key.
    ///
    /// The value is a DER-encoded X.509 public key, also known as SubjectPublicKeyInfo (SPKI), as defined in RFC 5280.
    /// When you use the HTTP API or the Amazon Web Services CLI, the value is Base64-encoded. Otherwise, it is not Base64-encoded.
    pub public_key: Option<Vec<u8>>,
}

/// Tells KMS whether the value of the Message parameter should be hashed as part of the signing algorithm.
/// Use `Raw` for unhashed messages; use `Digest` for message digests, which are already hashed.
#[derive(Serialize)]
pub enum MessageType {
    Raw,
    Digest,
}

/// The signing algorithm to use in signing this request
pub enum SigningAlgorithm {
    EcdsaSha256,
    EcdsaSha384,
    EcdsaSha512,
    RsassaPkcs1V15Sha256,
    RsassaPkcs1V15Sha384,
    RsassaPkcs1V15Sha512,
    RsassaPssSha256,
    RsassaPssSha384,
    RsassaPssSha512,
}

impl Serialize for SigningAlgorithm {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::EcdsaSha256 => serializer.serialize_str("ECDSA_SHA_256"),
            Self::EcdsaSha384 => serializer.serialize_str("ECDSA_SHA_384"),
            Self::EcdsaSha512 => serializer.serialize_str("ECDSA_SHA_512"),
            Self::RsassaPkcs1V15Sha256 => serializer.serialize_str("RSASSA_PKCS1_V1_5_SHA_256"),
            Self::RsassaPkcs1V15Sha384 => serializer.serialize_str("RSASSA_PKCS1_V1_5_SHA_384"),
            Self::RsassaPkcs1V15Sha512 => serializer.serialize_str("RSASSA_PKCS1_V1_5_SHA_512"),
            Self::RsassaPssSha256 => serializer.serialize_str("RSASSA_PSS_SHA_256"),
            Self::RsassaPssSha384 => serializer.serialize_str("RSASSA_PSS_SHA_384"),
            Self::RsassaPssSha512 => serializer.serialize_str("RSASSA_PSS_SHA_512"),
        }
    }
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
/// - `key_id`:  To specify a KMS key, use its key ID, key ARN, alias name, or alias ARN.
/// When using an alias name, prefix it with "alias/".
///     To specify a KMS key in a different Amazon Web Services account, you must use the key ARN or alias ARN.
///     For example:
///     - Key ID: 1234abcd-12ab-34cd-56ef-1234567890ab
///     - Key ARN: arn:aws:kms:us-east-2:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab
///     - Alias name: alias/ExampleAlias
///     - Alias ARN: arn:aws:kms:us-east-2:111122223333:alias/ExampleAlias
/// - `message`: The message or message digest to be signed by KMS.
/// - `message_type`: Specifies whether the message should be hashed (use `Raw` for unhashed, `Digest` for pre-hashed).
/// - `signing_algorithm`: The signing algorithm to use in signing this request
///
/// # Returns
///
/// A `Result` containing either the `SignRequestResponse` with the signing details or a `PlaidFunctionError` if something went wrong.
pub fn sign_arbitrary_message(
    key_id: &str,
    message: Vec<u8>,
    message_type: MessageType,
    signing_algorithm: SigningAlgorithm,
) -> Result<SignRequestResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(kms, sign_arbitrary_message);
    }

    #[derive(Serialize)]
    struct SignRequestRequest {
        key_id: String,
        message: Vec<u8>,
        message_type: MessageType,
        signing_algorithm: SigningAlgorithm,
    }

    let request = SignRequestRequest {
        key_id: key_id.to_string(),
        message,
        message_type,
        signing_algorithm,
    };

    let request = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        kms_sign_arbitrary_message(
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

/// Returns the public key of an asymmetric KMS key. Unlike the private key of a asymmetric KMS key,
/// which never leaves KMS unencrypted, callers with `kms:GetPublicKey` permission can download the public key of an asymmetric KMS key.
///
/// # Parameters
/// - `key_id`:  To specify a KMS key, use its key ID, key ARN, alias name, or alias ARN.
/// When using an alias name, prefix it with "alias/".
///     To specify a KMS key in a different Amazon Web Services account, you must use the key ARN or alias ARN.
///     For example:
///     - Key ID: 1234abcd-12ab-34cd-56ef-1234567890ab
///     - Key ARN: arn:aws:kms:us-east-2:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab
///     - Alias name: alias/ExampleAlias
///     - Alias ARN: arn:aws:kms:us-east-2:111122223333:alias/ExampleAlias
/// - `message`: The message or message digest to be signed by KMS.
pub fn get_public_key(key_id: &str) -> Result<GetPublicKeyResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(kms, get_public_key);
    }

    let mut request = HashMap::new();
    request.insert("key_id", key_id);

    let request = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        kms_get_public_key(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    return_buffer.truncate(res as usize);

    match serde_json::from_slice(&return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}
