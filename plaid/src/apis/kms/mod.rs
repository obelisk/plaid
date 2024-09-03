use super::ApiError;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_kms::{
    config::Credentials,
    operation::sign::SignOutput,
    primitives::Blob,
    types::{MessageType, SigningAlgorithmSpec},
    Client,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct SignRequestRequest {
    /// Specifies the message or message digest to sign. Messages can be 0-4096 bytes. To sign a larger message, provide a message digest.
    #[serde(deserialize_with = "parse_message")]
    message: Blob,
    /// Name of the request - defined in plaid.toml
    request_name: String,
    /// Tells KMS whether the value of the Message parameter should be hashed as part of the signing algorithm.
    /// Use RAW for unhashed messages; use DIGEST for message digests, which are already hashed.
    #[serde(deserialize_with = "parse_message_type")]
    message_type: MessageType,
}

/// Custom parser for message_type. Returns an error if an invalid message type is provided.
fn parse_message_type<'de, D>(deserializer: D) -> Result<MessageType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let message_type = String::deserialize(deserializer)?;

    match message_type.to_uppercase().as_str() {
        "RAW" => Ok(MessageType::Raw),
        "DIGEST" => Ok(MessageType::Digest),
        _ => Err(serde::de::Error::custom(
            "Invalid message type value provided",
        )),
    }
}

/// Custom parser for message_type. Returns an error if an invalid message type is provided.
fn parse_message<'de, D>(deserializer: D) -> Result<Blob, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let message = String::deserialize(deserializer)?;

    Ok(Blob::new(message))
}

/// Defines configuration for the KMS API
#[derive(Deserialize)]
pub struct KmsConfig {
    /// Specifies the authentication method for accessing the KMS API.
    ///
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: Authentication,
    sign_requests: HashMap<String, Request>,
}

/// Defines methods to authenticate to KMS with
#[derive(Deserialize)]
#[serde(untagged)]
enum Authentication {
    Iam {},
    ApiKey {
        access_key_id: String,
        secret_access_key: String,
        region: String,
    },
}

/// Defines the configuration of a signing request to KMS.
#[derive(Deserialize)]
pub struct Request {
    /// To specify a KMS key, use its key ID, key ARN, alias name, or alias ARN.
    /// When using an alias name, prefix it with "alias/".
    /// To specify a KMS key in a different Amazon Web Services account, you must use the key ARN or alias ARN.
    /// For example:
    /// - Key ID: 1234abcd-12ab-34cd-56ef-1234567890ab
    /// - Key ARN: arn:aws:kms:us-east-2:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab
    /// - Alias name: alias/ExampleAlias
    /// - Alias ARN: arn:aws:kms:us-east-2:111122223333:alias/ExampleAlias
    key_id: String,
    /// The modules allowed to use this request
    allowed_rules: Vec<String>,
    /// The signing algorithm to use in signing this request
    #[serde(deserialize_with = "parse_signing_algorithm")]
    signing_algorithm: SigningAlgorithmSpec,
}

/// Custom parser for signing_algorithm. Returns an error if an invalid message type is provided.
fn parse_signing_algorithm<'de, D>(deserializer: D) -> Result<SigningAlgorithmSpec, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let signing_algorithm = String::deserialize(deserializer)?;

    match signing_algorithm.to_uppercase().as_str() {
        "ECDSA_SHA_256" => Ok(SigningAlgorithmSpec::EcdsaSha256),
        "ECDSA_SHA_384" => Ok(SigningAlgorithmSpec::EcdsaSha384),
        "ECDSA_SHA_512" => Ok(SigningAlgorithmSpec::EcdsaSha512),
        "RSASSA_PKCS1_V1_5_SHA_256" => Ok(SigningAlgorithmSpec::RsassaPkcs1V15Sha256),
        "RSASSA_PKCS1_V1_5_SHA_384" => Ok(SigningAlgorithmSpec::RsassaPkcs1V15Sha384),
        "RSASSA_PKCS1_V1_5_SHA_512" => Ok(SigningAlgorithmSpec::RsassaPkcs1V15Sha512),
        "RSASSA_PSS_SHA_256" => Ok(SigningAlgorithmSpec::RsassaPssSha256),
        "RSASSA_PSS_SHA_384" => Ok(SigningAlgorithmSpec::RsassaPssSha384),
        "RSASSA_PSS_SHA_512" => Ok(SigningAlgorithmSpec::RsassaPssSha512),
        "SM2DSA" => Ok(SigningAlgorithmSpec::Sm2Dsa),
        _ => Err(serde::de::Error::custom(
            "Invalid signing algorithm provided. Accepted values are ECDSA_SHA_256, ECDSA_SHA_384, ECDSA_SHA_512, RSASSA_PKCS1_V1_5_SHA_256, RSASSA_PKCS1_V1_5_SHA_384, RSASSA_PKCS1_V1_5_SHA_512, RSASSA_PSS_SHA_256, RSASSA_PSS_SHA_384, RSASSA_PSS_SHA_512, and SM2DSA.",
        )),
    }
}

/// Represents the response from a sign request, including the key ID, signature, and signing algorithm used.
#[derive(Serialize)]
struct SignRequestResponse {
    /// The Amazon Resource Name (key ARN) of the asymmetric KMS key that was used to sign the message.
    key_id: Option<String>,
    /// The cryptographic signature that was generated for the message.
    ///
    /// - When used with the supported RSA signing algorithms, the encoding of this value is defined by PKCS #1 in [RFC 8017](https://tools.ietf.org/html/rfc8017).
    /// - When used with the `ECDSA_SHA_256`, `ECDSA_SHA_384`, or `ECDSA_SHA_512` signing algorithms, this value is a DER-encoded object as defined by ANSI X9.62–2005 and [RFC 3279 Section 2.2.3](https://tools.ietf.org/html/rfc3279#section-2.2.3). This is the most commonly used signature format and is appropriate for most uses.
    ///
    /// When you use the HTTP API or the Amazon Web Services CLI, the value is Base64-encoded. Otherwise, it is not Base64-encoded.
    signature: Option<Vec<u8>>,
    /// The signing algorithm that was used to sign the message.
    signing_algorithm: Option<String>,
}

impl SignRequestResponse {
    /// Creates a `SignRequestResponse` instance from a `SignOutput` object,
    /// extracting the key ID, signature, and signing algorithm.
    fn from_sign_output(sign_output: SignOutput) -> Self {
        let signature: Option<Vec<u8>> = sign_output.signature.map(|sig| sig.into_inner());

        let signing_algorithm = sign_output
            .signing_algorithm
            .map(|signing_algorithm| signing_algorithm.to_string());

        Self {
            key_id: sign_output.key_id,
            signature,
            signing_algorithm,
        }
    }
}

/// Represents the KMS API that handles all requests to KMS
pub struct Kms {
    /// The underlying KMS client used to interact with the KMS API.
    client: Client,
    /// A collection of pre-configured signing requests, keyed by their names.
    sign_requests: HashMap<String, Request>,
}

impl Kms {
    /// Creates a new instance of `Kms`
    pub async fn new(config: KmsConfig) -> Self {
        let sdk_config = match config.authentication {
            Authentication::ApiKey {
                access_key_id,
                secret_access_key,
                region,
            } => {
                info!("Using API keys to authenticate to KMS");
                let credentials =
                    Credentials::new(access_key_id, secret_access_key, None, None, "Plaid");

                aws_config::defaults(BehaviorVersion::latest())
                    .region(Region::new(region.clone()))
                    .credentials_provider(credentials)
                    .load()
                    .await
            }
            Authentication::Iam {} => {
                info!("Using IAM role assigned to environment for KMS authentication");
                aws_config::load_defaults(BehaviorVersion::latest()).await
            }
        };

        let client = aws_sdk_kms::Client::new(&sdk_config);

        Self {
            client,
            sign_requests: config.sign_requests,
        }
    }

    /// Makes a named signing request to the KMS API using the specified parameters.
    ///
    /// This function:
    /// - Parses the request parameters from a JSON string.
    /// - Fetches the corresponding signing request specification from the configuration.
    /// - Verifies that the calling module is allowed to use this request.
    /// - Sends the signing request to the KMS API and returns the signed result as a JSON string.
    ///
    /// Returns a `Result` containing the signed output as a `String` or an `ApiError` if any step fails.
    pub async fn make_named_signing_request(
        &self,
        params: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        // Parse the information needed to make the request
        let request: SignRequestRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let request_name = &request.request_name;

        // Attempt to fetch signing request specificaation from config
        let request_specification = match self.sign_requests.get(request_name) {
            Some(x) => x,
            None => {
                error!("{module} tried to use sign-request which doesn't exist: {request_name}");
                return Err(ApiError::BadRequest);
            }
        };

        // Verify that caller is allowed to use this request
        if !request_specification
            .allowed_rules
            .contains(&module.to_string())
        {
            error!("{module} tried to use sign-request which it's not allowed to: {request_name}");
            return Err(ApiError::BadRequest);
        }

        let output = self
            .client
            .sign()
            .key_id(&request_specification.key_id)
            .message_type(request.message_type)
            .signing_algorithm(request_specification.signing_algorithm.clone())
            .message(request.message)
            .send()
            .await
            .map_err(|e| ApiError::KmsSignError(e))?;

        let output = SignRequestResponse::from_sign_output(output);

        serde_json::to_string(&output).map_err(|_| ApiError::BadRequest)
    }
}
