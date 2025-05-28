use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};

use aws_sdk_kms::error::SdkError;
use aws_sdk_s3::{presigning::PresigningConfig, primitives::ByteStream, Client};
use serde::{Deserialize, Serialize};

use crate::{apis::ApiError, get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::put_object::PutObjectError;

/// Errors that may occur while interacting with S3.
#[derive(Debug)]
pub enum S3Errors {
    S3PutObjectError(SdkError<PutObjectError>),
    S3GetObjectError(SdkError<GetObjectError>),
    BytesStreamError(aws_sdk_s3::primitives::ByteStreamError),
    PresignError(aws_sdk_s3::presigning::PresigningConfigError),
}

/// Permissions for an S3 bucket.
#[derive(Deserialize, PartialEq)]
enum BucketPermission {
    /// Read-only access.
    Read,
    /// Write access.
    Write,
}

impl Default for BucketPermission {
    fn default() -> Self {
        Self::Read
    }
}

/// Configuration for a single S3 bucket, including permissions and the allowed rule.
#[derive(Deserialize)]
struct BucketConfiguration {
    /// Access permission (Read or Write). Defaults to `Read` if a value isn't provided
    #[serde(default)]
    permission: BucketPermission,
    /// Rule name that is allowed to access this bucket.
    rule: String,
}

/// Configuration for initializing the S3 API wrapper.
#[derive(Deserialize)]
pub struct S3Config {
    /// Authentication method used to access S3.
    authentication: AwsAuthentication,
    /// Maps S3 bucket names to their associated configuration rules.
    bucket_configuration: HashMap<String, Vec<BucketConfiguration>>,
}

/// S3 wrapper for interacting with the AWS S3 service based on bucket/rule configuration.
pub struct S3 {
    /// AWS SDK S3 client.
    client: Client,
    /// Configured buckets and their associated access rules.
    bucket_configuration: HashMap<String, Vec<BucketConfiguration>>,
}

/// Request payload for uploading an object to S3.
#[derive(Deserialize)]
struct PutObjectRequest {
    /// Target bucket ID.
    bucket_id: String,
    /// Object data as a byte vector.
    object: Vec<u8>,
    /// Key to store the object under.
    object_key: String,
}

/// Request payload for retrieving an object from S3.
#[derive(Deserialize)]
struct GetObjectRequest {
    /// Bucket ID to fetch the object from.
    bucket_id: String,
    /// Key of the object to fetch.
    object_key: String,
    /// Optional duration in seconds for generating a presigned URL.
    expires_in: Option<u64>,
}

/// Response returned from `get_object`.
#[derive(Serialize)]
enum GetObjectReponse {
    /// Raw object data.
    Object(Vec<u8>),
    /// Presigned URL to access the object.
    PresignedUri(String),
}

impl S3 {
    /// Creates a new `S3` instance using the provided configuration.
    pub async fn new(config: S3Config) -> Self {
        let sdk_config = get_aws_sdk_config(config.authentication).await;
        let client = aws_sdk_s3::Client::new(&sdk_config);

        Self {
            client,
            bucket_configuration: config.bucket_configuration,
        }
    }

    /// Handles a request to retrieve an object from S3 or generate a presigned URL.
    pub async fn get_object(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetObjectRequest>(params).map_err(|_| ApiError::BadRequest)?;

        self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;

        let request_builder = self
            .client
            .get_object()
            .bucket(request.bucket_id)
            .key(request.object_key);

        let response = if let Some(expiration) = request.expires_in {
            let presigned = PresigningConfig::expires_in(Duration::from_secs(expiration))
                .map_err(S3Errors::PresignError)?;

            let response = request_builder
                .presigned(presigned)
                .await
                .map_err(S3Errors::S3GetObjectError)?;

            GetObjectReponse::PresignedUri(response.uri().to_string())
        } else {
            let response = request_builder
                .send()
                .await
                .map_err(S3Errors::S3GetObjectError)?;

            let object_bytes = response
                .body
                .collect()
                .await
                .map_err(S3Errors::BytesStreamError)?
                .into_bytes();

            GetObjectReponse::Object(object_bytes.to_vec())
        };

        let serialized = serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)?;

        Ok(serialized)
    }

    /// Handles a request to upload an object to S3.
    pub async fn put_object(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the information needed to make the request
        let request =
            serde_json::from_str::<PutObjectRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // Check that caller is allowed to write to this bucket
        let bucket_config = self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;
        if bucket_config.permission != BucketPermission::Write {
            error!("{module} tried to write to a bucket but is not permitted to");
            return Err(ApiError::BadRequest);
        }

        self.client
            .put_object()
            .bucket(request.bucket_id)
            .body(ByteStream::from(request.object))
            .key(request.object_key)
            .send()
            .await
            .map_err(S3Errors::S3PutObjectError)?;

        Ok(String::new())
    }

    /// Verifies that the module is authorized to access the specified bucket
    fn fetch_bucket_configuration<T: Display>(
        &self,
        module: T,
        bucket: &str,
    ) -> Result<&BucketConfiguration, ApiError> {
        match self.bucket_configuration.get(bucket) {
            Some(config) => {
                if let Some(bucket) = config.iter().find(|conf| conf.rule == module.to_string()) {
                    Ok(bucket)
                } else {
                    error!(
                        "{module} tried to use an S3 bucket which it's not allowed to: {bucket}"
                    );
                    Err(ApiError::BadRequest)
                }
            }
            None => {
                error!("{module} tried to use a S3 bucket that is not configured: {bucket}");
                Err(ApiError::BadRequest)
            }
        }
    }
}
