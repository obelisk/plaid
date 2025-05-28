use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};

use aws_sdk_kms::error::SdkError;
use aws_sdk_s3::{presigning::PresigningConfig, primitives::ByteStream, Client};
use serde::{Deserialize, Serialize};

use crate::{apis::ApiError, get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::put_object::PutObjectError;

#[derive(Debug)]
pub enum S3Errors {
    S3PutObjectError(SdkError<PutObjectError>),
    S3GetObjectError(SdkError<GetObjectError>),
    BytesStreamError(aws_sdk_s3::primitives::ByteStreamError),
    PresignError(aws_sdk_s3::presigning::PresigningConfigError),
}

#[derive(Deserialize, PartialEq)]
enum BucketPermission {
    Read,
    Write,
}

impl Default for BucketPermission {
    fn default() -> Self {
        Self::Read
    }
}

#[derive(Deserialize)]
struct BucketConfiguration {
    #[serde(default)]
    permission: BucketPermission,
    rule: String,
}

/// Defines configuration for the KMS API
#[derive(Deserialize)]
pub struct S3Config {
    /// Specifies the authentication method for accessing the KMS API.
    ///
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
    /// Configured bucket - maps an S3 bucket ID to a list of rules that are allowed to use it
    bucket_configuration: HashMap<String, Vec<BucketConfiguration>>,
}

/// Represents the KMS API that handles all requests to KMS
pub struct S3 {
    /// The underlying KMS client used to interact with the KMS API.
    client: Client,
    /// Configured bucket - maps an S3 bucket ID to a list of rules that are allowed to use it
    bucket_configuration: HashMap<String, Vec<BucketConfiguration>>,
}

#[derive(Deserialize)]
struct PutObjectRequest {
    /// The bucket name to which the `PUT` action was initiated.
    bucket_id: String,
    /// Object data.
    object: Vec<u8>,
    /// Object key for which the `PUT` action was initiated.
    object_key: String,
}

#[derive(Deserialize)]
struct GetObjectRequest {
    /// The bucket name containing the object.
    bucket_id: String,
    /// Key of the object to get.
    object_key: String,
    expires_in: Option<u64>,
}

#[derive(Serialize)]
enum GetObjectReponse {
    Object(Vec<u8>),
    PresignedUri(String),
}

impl S3 {
    /// Creates a new instance of `S3`
    pub async fn new(config: S3Config) -> Self {
        let sdk_config = get_aws_sdk_config(config.authentication).await;
        let client = aws_sdk_s3::Client::new(&sdk_config);

        Self {
            client,
            bucket_configuration: config.bucket_configuration,
        }
    }

    pub async fn get_object(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the information needed to make the request
        let request =
            serde_json::from_str::<GetObjectRequest>(params).map_err(|_| ApiError::BadRequest)?;

        // Check that caller is allowed to read from this bucket
        self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;

        let request_builder = self
            .client
            .get_object()
            .bucket(request.bucket_id)
            .key(request.object_key);

        let response = if let Some(expiration) = request.expires_in {
            let presigned = PresigningConfig::expires_in(Duration::from_secs(expiration))
                .map_err(|e| S3Errors::PresignError(e))?;

            let response = request_builder
                .presigned(presigned)
                .await
                .map_err(|e| S3Errors::S3GetObjectError(e))?;

            GetObjectReponse::PresignedUri(response.uri().to_string())
        } else {
            let response = request_builder
                .send()
                .await
                .map_err(|e| S3Errors::S3GetObjectError(e))?;

            let object_bytes = response
                .body
                .collect()
                .await
                .map_err(|e| S3Errors::BytesStreamError(e))?
                .into_bytes();

            GetObjectReponse::Object(object_bytes.to_vec())
        };

        let serialized = serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)?;

        Ok(serialized)
    }

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
            .map_err(|e| S3Errors::S3PutObjectError(e))?;

        Ok(String::new())
    }

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
                error!("{module} tried to use a S3 bucket that is not configured: {bucket}",);
                return Err(ApiError::BadRequest);
            }
        }
    }
}
