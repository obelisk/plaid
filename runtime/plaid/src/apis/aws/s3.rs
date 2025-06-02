use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};

use aws_sdk_kms::error::SdkError;
use aws_sdk_s3::operation::get_object_attributes::GetObjectAttributesError;
use aws_sdk_s3::{presigning::PresigningConfig, primitives::ByteStream, Client};
use plaid_stl::aws::s3::{GetObjectReponse, GetObjectRequest, ObjectAttributes, PutObjectRequest};
use serde::Deserialize;

use crate::{apis::ApiError, get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::put_object::PutObjectError;

/// Errors that may occur while interacting with S3.
#[derive(Debug)]
pub enum S3Errors {
    PutObjectError(SdkError<PutObjectError>),
    GetObjectError(SdkError<GetObjectError>),
    GetObjectAttributesError(SdkError<GetObjectAttributesError>),
    BytesStreamError(aws_sdk_s3::primitives::ByteStreamError),
    PresignError(aws_sdk_s3::presigning::PresigningConfigError),
    NoContentLengthReturned,
    ObjectTooLarge,
}

/// Configuration for a single S3 bucket, including permissions and the allowed rule.
#[derive(Deserialize)]
struct BucketConfiguration {
    #[serde(default)]
    r: Vec<String>,
    #[serde(default)]
    rw: Vec<String>,
}

/// Configuration for initializing the S3 API wrapper.
#[derive(Deserialize)]
pub struct S3Config {
    /// Authentication method used to access S3.
    authentication: AwsAuthentication,
    /// The maximum object size that we'll read into memory. Defaults to 4 MiB
    /// if no value is provided
    #[serde(default = "default_max_object_size")]
    max_object_fetch_size: usize,
    /// Maps S3 bucket names to their associated configuration rules.
    bucket_configuration: HashMap<String, BucketConfiguration>,
}

fn default_max_object_size() -> usize {
    1024 * 1024 * 4
}

/// S3 wrapper for interacting with the AWS S3 service based on bucket/rule configuration.
pub struct S3 {
    /// AWS SDK S3 client.
    client: Client,
    /// Configured buckets and their associated access rules.
    bucket_configuration: HashMap<String, BucketConfiguration>,
    /// The maximum object size that we'll read into memory. Defaults to 4 MiB
    /// if no value is provided
    max_object_fetch_size: usize,
}

impl S3 {
    /// Creates a new `S3` instance using the provided configuration.
    pub async fn new(config: S3Config) -> Self {
        let sdk_config = get_aws_sdk_config(config.authentication).await;
        let client = aws_sdk_s3::Client::new(&sdk_config);

        Self {
            client,
            bucket_configuration: config.bucket_configuration,
            max_object_fetch_size: config.max_object_fetch_size,
        }
    }

    /// Retrieves all of the metadata from an object without returning the object itself
    pub async fn get_object_attributes(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetObjectRequest>(params).map_err(|_| ApiError::BadRequest)?;

        let module = module.to_string();
        let bucket_config = self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;
        if !bucket_config.r.contains(&module) && !bucket_config.rw.contains(&module) {
            error!(
                "{module} tried to use an S3 bucket which it's not allowed to: {}",
                request.bucket_id
            );
            return Err(ApiError::BadRequest);
        }

        let object_attributes = self
            .client
            .get_object_attributes()
            .bucket(request.bucket_id)
            .key(request.object_key)
            .send()
            .await
            .map_err(S3Errors::GetObjectAttributesError)?;

        let response = ObjectAttributes {
            object_size: object_attributes.object_size,
            last_modified: object_attributes.last_modified.map(|lm| lm.secs()),
        };

        let serialized = serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)?;

        Ok(serialized)
    }

    /// Handles a request to retrieve an object from S3 or generate a presigned URL.
    pub async fn get_object(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<GetObjectRequest>(params).map_err(|_| ApiError::BadRequest)?;

        let module = module.to_string();
        let bucket_config = self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;
        if !bucket_config.r.contains(&module) && !bucket_config.rw.contains(&module) {
            error!(
                "{module} tried to use an S3 bucket which it's not allowed to: {}",
                request.bucket_id
            );
            return Err(ApiError::BadRequest);
        }

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
                .map_err(S3Errors::GetObjectError)?;

            GetObjectReponse::PresignedUri(response.uri().to_string())
        } else {
            let response = request_builder
                .send()
                .await
                .map_err(S3Errors::GetObjectError)?;

            // Check that the object's length is less than our configured max before reading into memory.
            let length = response
                .content_length
                .ok_or(S3Errors::NoContentLengthReturned)?;

            if length as usize > self.max_object_fetch_size {
                return Err(S3Errors::ObjectTooLarge)?;
            }

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
        if !bucket_config.rw.contains(&module.to_string()) {
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
            .map_err(S3Errors::PutObjectError)?;

        Ok(String::new())
    }

    /// Verifies that the module is authorized to access the specified bucket
    fn fetch_bucket_configuration<T: Display>(
        &self,
        module: T,
        bucket: &str,
    ) -> Result<&BucketConfiguration, ApiError> {
        match self.bucket_configuration.get(bucket) {
            Some(config) => Ok(config),
            None => {
                error!("{module} tried to use a S3 bucket that is not configured: {bucket}");
                Err(ApiError::BadRequest)
            }
        }
    }
}
