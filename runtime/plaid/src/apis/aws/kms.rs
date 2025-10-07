use crate::{apis::ApiError, get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};
use aws_sdk_kms::{
    operation::{get_public_key::GetPublicKeyOutput, sign::SignOutput},
    primitives::Blob,
    types::{MessageType, SigningAlgorithmSpec},
    Client,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display, sync::Arc};

/// A request to sign a given message with a KMS key.
#[derive(Deserialize)]
struct SignRequestRequest {
    /// To specify a KMS key, use its key ID, key ARN, alias name, or alias ARN.
    /// When using an alias name, prefix it with "alias/".
    /// To specify a KMS key in a different Amazon Web Services account, you must use the key ARN or alias ARN.
    /// For example:
    /// - Key ID: 1234abcd-12ab-34cd-56ef-1234567890ab
    /// - Key ARN: arn:aws:kms:us-east-2:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab
    /// - Alias name: alias/ExampleAlias
    /// - Alias ARN: arn:aws:kms:us-east-2:111122223333:alias/ExampleAlias
    key_id: String,
    /// Specifies the message or message digest to sign. Messages can be 0-4096 bytes. To sign a larger message, provide a message digest.
    message: Vec<u8>,
    /// Tells KMS whether the value of the Message parameter should be hashed as part of the signing algorithm.
    /// Use RAW for unhashed messages; use DIGEST for message digests, which are already hashed.
    #[serde(deserialize_with = "parse_message_type")]
    message_type: MessageType,
    #[serde(deserialize_with = "parse_signing_algorithm")]
    /// The signing algorithm to use in signing this request
    signing_algorithm: SigningAlgorithmSpec,
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

/// Defines configuration for the KMS API
#[derive(Deserialize)]
pub struct KmsConfig {
    /// Specifies the authentication method for accessing the KMS API.
    ///
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
    /// Configured keys - maps a KMS key ID to a list of rules that are allowed to use it
    key_configuration: HashMap<String, Vec<String>>,
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

/// Represents the response from `get_public_key`. There are more fields that KMS sends us
/// but we won't include them for now.
/// See https://docs.rs/aws-sdk-kms/1.42.0/aws_sdk_kms/operation/get_public_key/struct.GetPublicKeyOutput.html
/// for entire output.
#[derive(Serialize)]
struct PublicKey {
    /// The Amazon Resource Name (key ARN) of the asymmetric KMS key that was used to sign the message.
    key_id: Option<String>,
    /// The exported public key.
    /// The value is a DER-encoded X.509 public key, also known as SubjectPublicKeyInfo (SPKI), as defined in RFC 5280.
    /// When you use the HTTP API or the Amazon Web Services CLI, the value is Base64-encoded.
    /// Otherwise, it is not Base64-encoded.
    pub public_key: Option<Vec<u8>>,
}

impl PublicKey {
    /// Creates a `PublicKey` instance from a `GetPublicKeyOutput` object,
    /// extracting the key ID and public key
    fn from_aws_response(pub_key_output: GetPublicKeyOutput) -> Self {
        let public_key: Option<Vec<u8>> = pub_key_output.public_key.map(|sig| sig.into_inner());

        Self {
            key_id: pub_key_output.key_id,
            public_key,
        }
    }
}

/// Represents the KMS API that handles all requests to KMS
pub struct Kms {
    /// The underlying KMS client used to interact with the KMS API.
    client: Client,
    /// A collection of KMS key IDs and the rules that are allowed to interact with them
    key_configuration: HashMap<String, Vec<String>>,
}

impl Kms {
    /// Creates a new instance of `Kms`
    pub async fn new(config: KmsConfig) -> Self {
        let sdk_config = get_aws_sdk_config(&config.authentication).await;
        let client = aws_sdk_kms::Client::new(&sdk_config);

        Self {
            client,
            key_configuration: config.key_configuration,
        }
    }

    /// Signs an arbitrary message using the provided KMS key
    ///
    /// This function:
    /// - Parses the request parameters from a JSON string.
    /// - Fetches the configuration settings for the parsed out KMS key ID
    /// - Verifies that the calling module is allowed to use this KMS key
    /// - Sends the signing request to the KMS API and returns the signed result as a JSON string.
    ///
    /// Returns a `Result` containing the signed output as a `String` or an `ApiError` if any step fails.
    pub async fn sign_arbitrary_message(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the information needed to make the request
        let request: SignRequestRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Fetch rules that are allowd to use this key
        let allowed_rules = self.fetch_key_configuration(module.to_string(), &request.key_id)?;

        // Verify that caller is allowed to use this key
        if !allowed_rules.contains(&module.to_string()) {
            error!(
                "{module} tried to use KMS key which it's not allowed to: {}",
                request.key_id
            );
            return Err(ApiError::BadRequest);
        }

        let output = self
            .client
            .sign()
            .key_id(&request.key_id)
            .message_type(request.message_type)
            .signing_algorithm(request.signing_algorithm.clone())
            .message(Blob::new(request.message))
            .send()
            .await
            .map_err(|e| ApiError::KmsSignError(e))?;

        let output = SignRequestResponse::from_sign_output(output);

        serde_json::to_string(&output).map_err(|_| ApiError::BadRequest)
    }

    /// Returns the public key of an asymmetric KMS key. Unlike the private key of a asymmetric KMS key,
    /// which never leaves KMS unencrypted, callers with `kms:GetPublicKey` permission can download the public key of an asymmetric KMS key.
    pub async fn get_public_key(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the information needed to make the request
        let request: HashMap<String, String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let key_id = request
            .get("key_id")
            .ok_or(ApiError::MissingParameter("key_id".to_string()))?
            .to_string();

        // Fetch rules that are allowd to use this key
        let allowed_rules = self.fetch_key_configuration(module.clone(), &key_id)?;

        // Verify that caller is allowed to use this key
        if !allowed_rules.contains(&module.to_string()) {
            error!("{module} tried to use KMS key which it's not allowed to: {key_id}",);
            return Err(ApiError::BadRequest);
        }

        let output = self
            .client
            .get_public_key()
            .key_id(key_id)
            .send()
            .await
            .map_err(|e| ApiError::KmsGetPublicKeyError(e))?;

        let output = PublicKey::from_aws_response(output);

        serde_json::to_string(&output).map_err(|_| ApiError::BadRequest)
    }

    fn fetch_key_configuration<T: Display>(
        &self,
        module: T,
        key_id: &str,
    ) -> Result<Vec<String>, ApiError> {
        match self.key_configuration.get(key_id) {
            Some(config) => Ok(config.to_vec()),
            None => {
                error!("{module} tried to use a KMS key that is not configured: {key_id}",);
                return Err(ApiError::BadRequest);
            }
        }
    }
}
