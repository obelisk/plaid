use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};

use aws_sdk_kms::error::SdkError;
use aws_sdk_s3::operation::delete_object::DeleteObjectError;
use aws_sdk_s3::operation::get_object_attributes::GetObjectAttributesError;
use aws_sdk_s3::operation::list_object_versions::ListObjectVersionsError;
use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error;
use aws_sdk_s3::operation::put_object_tagging::PutObjectTaggingError;
use aws_sdk_s3::types::{Tag, Tagging};
use aws_sdk_s3::{presigning::PresigningConfig, primitives::ByteStream, Client};
use plaid_stl::aws::s3::{
    DeleteObjectRequest, GetObjectRequest, GetObjectResponse, ListObjectVersionsRequest,
    ListObjectVersionsResponse, ListObjectsRequest, ListObjectsResponse, ObjectAttributes,
    ObjectVersion, PutObjectRequest, PutObjectTagRequest,
};
use serde::Deserialize;

use crate::{apis::ApiError, get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::put_object::PutObjectError;

/// Errors that may occur while interacting with S3.
#[derive(Debug)]
pub enum S3Errors {
    TooManyTagsProvided,
    BuildError(aws_sdk_s3::error::BuildError),
    TagObjectError(SdkError<PutObjectTaggingError>),
    PutObjectError(SdkError<PutObjectError>),
    GetObjectError(SdkError<GetObjectError>),
    ListObjectsError(SdkError<ListObjectsV2Error>),
    DeleteObjectError(SdkError<DeleteObjectError>),
    ListObjectVersionsError(SdkError<ListObjectVersionsError>),
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

    /// Removes an object from a bucket.
    pub async fn delete_object(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request = serde_json::from_str::<DeleteObjectRequest>(params)
            .map_err(|_| ApiError::BadRequest)?;

        let module = module.to_string();
        let bucket_config = self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;
        if !bucket_config.rw.contains(&module) {
            error!(
                "{module} tried to use an S3 bucket which it's not allowed to: {}",
                request.bucket_id
            );
            return Err(ApiError::BadRequest);
        }

        let mut delete_request = self
            .client
            .delete_object()
            .bucket(request.bucket_id)
            .key(request.object_key);

        if let Some(version) = request.version_id {
            delete_request = delete_request.version_id(version)
        }

        delete_request
            .send()
            .await
            .map_err(S3Errors::DeleteObjectError)?;

        Ok(0)
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
            .object_attributes(aws_sdk_s3::types::ObjectAttributes::ObjectSize)
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

    /// Returns metadata about all versions of the objects in a bucket.
    pub async fn list_object_versions(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request = serde_json::from_str::<ListObjectVersionsRequest>(params)
            .map_err(|_| ApiError::BadRequest)?;

        let module = module.to_string();
        let bucket_config = self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;
        if !bucket_config.r.contains(&module) && !bucket_config.rw.contains(&module) {
            error!(
                "{module} tried to use an S3 bucket which it's not allowed to: {}",
                request.bucket_id
            );
            return Err(ApiError::BadRequest);
        }

        let object_versions = self
            .client
            .list_object_versions()
            .bucket(request.bucket_id)
            .prefix(request.object_key)
            .send()
            .await
            .map_err(S3Errors::ListObjectVersionsError)?;

        let versions = object_versions
            .versions()
            .iter()
            .filter_map(|ver| {
                Some(ObjectVersion {
                    key: ver.key.clone()?,
                    is_latest: ver.is_latest?,
                    last_modified: ver.last_modified?.secs(),
                    version_id: ver.version_id.clone()?,
                })
            })
            .collect::<Vec<_>>();

        let response = ListObjectVersionsResponse { versions };

        let serialized = serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)?;

        Ok(serialized)
    }

    /// Lists objects in the specified S3 bucket up to 1,000 per request.
    pub async fn list_objects(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<ListObjectsRequest>(params).map_err(|_| ApiError::BadRequest)?;

        let module = module.to_string();
        let bucket_config = self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;
        if !bucket_config.r.contains(&module) && !bucket_config.rw.contains(&module) {
            error!(
                "{module} tried to use an S3 bucket which it's not allowed to: {}",
                request.bucket_id
            );
            return Err(ApiError::BadRequest);
        }

        let mut list = self
            .client
            .list_objects_v2()
            .bucket(request.bucket_id)
            .prefix(request.prefix);

        if let Some(token) = request.continuation_key {
            list = list.continuation_token(token);
        }

        let response = list.send().await.map_err(S3Errors::ListObjectsError)?;
        let keys = response.contents.map(|contents| {
            contents
                .into_iter()
                .filter_map(|obj| obj.key)
                .collect::<Vec<_>>()
        });

        let response = ListObjectsResponse {
            continuation_token: response.next_continuation_token,
            keys,
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

            GetObjectResponse::PresignedUri(response.uri().to_string())
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

            GetObjectResponse::Object(object_bytes.to_vec())
        };

        let serialized = serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)?;

        Ok(serialized)
    }

    /// Handles a request to upload an object to S3.
    pub async fn put_object(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
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

        Ok(0)
    }

    /// Sets the supplied tag-set to an object that already exists in a bucket. A tag is a key-value pair.
    pub async fn put_object_tag(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        // Parse the information needed to make the request
        let request = serde_json::from_str::<PutObjectTagRequest>(params)
            .map_err(|_| ApiError::BadRequest)?;

        // Check that caller is allowed to write to this bucket
        let bucket_config = self.fetch_bucket_configuration(module.clone(), &request.bucket_id)?;
        if !bucket_config.rw.contains(&module.to_string()) {
            error!("{module} tried to write to a bucket but is not permitted to");
            return Err(ApiError::BadRequest);
        }

        // S3 allows up to 10 tags per object
        if request.tags.len() > 10 {
            return Err(S3Errors::TooManyTagsProvided)?;
        }

        // Parse and filter tags
        let tags = request
            .tags
            .into_iter()
            .filter_map(|(key, value)| {
                let key_len = key.len();
                let val_len = value.len();

                // Length checks (key: 1–128; value: 0–256)
                if key_len < 1 || key_len > 128 || val_len > 256 {
                    return None;
                }

                // Reserved prefix
                if key.starts_with("aws:") || value.starts_with("aws:") {
                    return None;
                }

                // Restricted characters
                if !key.chars().all(is_safe_tag_char) || !value.chars().all(is_safe_tag_char) {
                    return None;
                }

                Tag::builder().key(key).value(value).build().ok()
            })
            .collect::<Vec<_>>();

        let tag_set = Tagging::builder()
            .set_tag_set(Some(tags))
            .build()
            .map_err(S3Errors::BuildError)?;

        self.client
            .put_object_tagging()
            .bucket(request.bucket_id)
            .key(request.object_key)
            .tagging(tag_set)
            .send()
            .await
            .map_err(S3Errors::TagObjectError)?;

        Ok(0)
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

// Define the "safe" character predicate
fn is_safe_tag_char(c: char) -> bool {
    c.is_alphanumeric()
        || c.is_whitespace()
        || matches!(c, '+' | '-' | '=' | '.' | '_' | ':' | '/' | '@')
}
