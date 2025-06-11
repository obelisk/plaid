use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 4;

/// Specifies how an object should be fetched from S3.
pub enum ObjectFetchMode {
    /// Fetch a presigned URL that is valid for the specified number of seconds.
    Presigned(u64),
    /// Fetch the full object contents.
    FullObject,
}

/// Represents the response returned from the `get_object` function.
#[derive(Deserialize, Serialize, Debug)]
pub enum GetObjectReponse {
    /// The full object data.
    Object(Vec<u8>),
    /// A presigned URI for accessing the object.
    PresignedUri(String),
}

/// Request payload for retrieving an object from S3.
#[derive(Deserialize, Serialize)]
pub struct GetObjectRequest {
    /// The bucket name from which the object is requested.
    pub bucket_id: String,
    /// The key identifying the object to fetch.
    pub object_key: String,
    /// Optional expiration time for the presigned URL (in seconds).
    pub expires_in: Option<u64>,
}

/// Request payload for retrieving an object from S3.
#[derive(Deserialize, Serialize)]
pub struct ListObjectsRequest {
    /// The bucket name from which the object is requested.
    pub bucket_id: String,
    /// The key identifying the object to fetch.
    pub prefix: String,
    pub continuation_key: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct ListObjectsResponse {
    /// `continuation_token` is sent when `isTruncated` is true, which means there are more keys in the bucket
    /// that can be listed. The next list requests to Amazon S3 can be continued with this token
    pub continuation_token: Option<String>,
    /// List of keys matching the provided prefix
    pub keys: Option<Vec<String>>,
}

/// Request payload for uploading an object to S3.
#[derive(Deserialize, Serialize)]
pub struct PutObjectRequest {
    /// The bucket name to which the `PUT` action was initiated.
    pub bucket_id: String,
    /// Object data.
    pub object: Vec<u8>,
    /// Object key for which the `PUT` action was initiated.
    pub object_key: String,
}

/// Request payload for tagging an object in S3.
#[derive(Deserialize, Serialize)]
pub struct PutObjectTagRequest {
    /// The bucket name containing the object.
    pub bucket_id: String,
    /// Name of the object key.
    pub object_key: String,
    /// Tags to apply to the object
    pub tags: HashMap<String, String>,
}

/// Represents an S3 object's metadata
#[derive(Deserialize, Serialize)]
pub struct ObjectAttributes {
    /// The size of the object in bytes.
    pub object_size: Option<i64>,
    /// Unix timestamp of when the object was last modified.
    pub last_modified: Option<i64>,
}

/// Request payload for listing an object's versions in S3
#[derive(Serialize, Deserialize)]
pub struct ListObjectVersionsRequest {
    /// The bucket name containing the object.
    pub bucket_id: String,
    /// Name of the object key.
    pub object_key: String,
}

/// Represents the versions of an S3 object
#[derive(Serialize, Deserialize)]
pub struct ListObjectVersionsResponse {
    /// Container for version information.
    pub versions: Vec<ObjectVersion>,
}

/// Metadata about an object's version
#[derive(Serialize, Deserialize)]
pub struct ObjectVersion {
    /// The object key
    pub key: String,
    /// Specifies whether the object is (`true`) or is not (`false`) the latest version of an object.
    pub is_latest: bool,
    /// Date and time when the object was last modified.
    pub last_modified: i64,
    /// Version ID of an object.
    pub version_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct DeleteObjectRequest {
    /// The bucket name containing the object.
    pub bucket_id: String,
    /// Key name of the object to delete.
    pub object_key: String,
    /// Version ID used to reference a specific version of the object.
    pub version_id: Option<String>,
}

/// Removes an object from a bucket. The behavior depends on the bucket's versioning state:
///
/// - If bucket versioning is not enabled, the operation permanently deletes the object.
/// - If bucket versioning is enabled, the operation inserts a delete marker, which becomes the current version of the object.
///   To permanently delete an object in a versioned bucket, you must include the object’s `versionId` in the request.
///   For more information about versioning-enabled buckets, see [Deleting object versions from a versioning-enabled bucket](https://docs.aws.amazon.com/AmazonS3/latest/userguide/DeletingObjectVersions.html)
/// - If bucket versioning is suspended, the operation removes the object that has a null `versionId`, if there is one, and
///   inserts a delete marker that becomes the current version of the object. If there isn't an object with a null `versionId`,
///   and all versions of the object have a `versionId`, Amazon S3 does not remove the object and only inserts a delete marker.
///   To permanently delete an object that has a `versionId`, you must include the object’s `versionId` in the request.
///   For more information about versioning-suspended buckets, see [Deleting objects from versioning-suspended buckets](https://docs.aws.amazon.com/AmazonS3/latest/userguide/DeletingObjectsfromVersioningSuspendedBuckets.html)
///
/// # Arguments
pub fn delete_object(
    bucket_id: &str,
    object_key: &str,
    version_id: Option<String>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(aws_s3, delete_object);
    }

    let request = DeleteObjectRequest {
        bucket_id: bucket_id.to_string(),
        object_key: object_key.to_string(),
        version_id,
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let res = unsafe { aws_s3_delete_object(request.as_ptr(), request.len()) };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

/// Returns metadata about all versions of the objects in a bucket.
/// See https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjectVersions.html for full documentation
///
/// # Arguments
///
/// * `bucket_id` - The name of the bucket to upload the object to.
/// * `object_key` - The key to identify the object within the bucket.
pub fn list_object_versions(
    bucket_id: &str,
    object_key: &str,
) -> Result<ListObjectVersionsResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_s3, list_object_versions);
    }

    let request = ListObjectVersionsRequest {
        bucket_id: bucket_id.to_string(),
        object_key: object_key.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aws_s3_list_object_versions(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match serde_json::from_slice(&return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Uploads an object to S3
/// See https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutObject.html for full documentation
///
/// # Arguments
///
/// * `bucket_id` - The name of the bucket to upload the object to.
/// * `object_key` - The key to identify the object within the bucket.
/// * `object` - The binary data of the object to upload.
pub fn put_object(
    bucket_id: &str,
    object_key: &str,
    object: Vec<u8>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(aws_s3, put_object);
    }

    let request = PutObjectRequest {
        bucket_id: bucket_id.to_string(),
        object,
        object_key: object_key.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let res = unsafe { aws_s3_put_object(request.as_ptr(), request.len()) };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

/// Replaces the entire tag set for an S3 object using the AWS S3 `PutObjectTagging` API.
///
/// This operation **overwrites** any existing tags on the specified object. If you wish to
/// modify or remove individual tags, you must supply the complete set of desired tags—any
/// tags omitted from `tags` will be deleted. AWS enforces a maximum of **10 tags** per object,
/// and tag keys **may not** begin with the reserved prefix `aws:`. By default, this function
/// applies to the **current/latest version** of the object; tagging a specific prior version
/// is not supported by this variant.
///
/// # Arguments
///
/// * `bucket_id` – The name of the S3 bucket containing the object.
/// * `object_key` – The key (path/name) of the object to apply tags to.
/// * `tags` – A `HashMap` of tag keys and values to apply.  
///   - A tag key can be up to 128 Unicode characters in length, and tag values can be up to 256 Unicode characters in length.
///   - Neither keys nor values may begin with `aws:`. The set of allowed characters are
///     letters (`a-z`, `A-Z`), numbers (`0-9`), and spaces representable in UTF-8, and the following characters: `+ - = . _ : / @`
///   - At most 10 entries are allowed.
///
/// For full details on the S3 tagging model—including character limits, reserved prefixes,
/// replication behavior, and how tags interact with bucket versioning—see the AWS S3
/// Object Tagging documentation:
/// <https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-tagging.html>
pub fn put_object_tag(
    bucket_id: &str,
    object_key: &str,
    tags: HashMap<String, String>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(aws_s3, put_object_tag);
    }

    let request = PutObjectTagRequest {
        bucket_id: bucket_id.to_string(),
        object_key: object_key.to_string(),
        tags,
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let res = unsafe { aws_s3_put_object_tag(request.as_ptr(), request.len()) };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

/// Fetches an object's metadata from S3
/// See https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectAttributes.html for full documentation
///
/// # Arguments
///
/// * `bucket_id` - The name of the bucket from which to fetch the object.
/// * `object_key` - The key identifying the object to fetch.
pub fn get_object_attributes(
    bucket_id: &str,
    object_key: &str,
) -> Result<ObjectAttributes, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_s3, get_object_attributes);
    }

    let request = GetObjectRequest {
        bucket_id: bucket_id.to_string(),
        object_key: object_key.to_string(),
        expires_in: None,
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aws_s3_get_object_attributes(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match serde_json::from_slice(&return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Retrieves an object from S3.
/// See https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObject.html for full documentation
///
/// # Arguments
///
/// * `bucket_id` - The name of the bucket from which to fetch the object.
/// * `object_key` - The key identifying the object to fetch.
/// * `fetch_mode` - Specifies whether to fetch the full object or a presigned URL.
pub fn get_object(
    bucket_id: &str,
    object_key: &str,
    fetch_mode: ObjectFetchMode,
) -> Result<GetObjectReponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_s3, get_object);
    }

    let (expires_in, return_buffer_size) = match fetch_mode {
        ObjectFetchMode::Presigned(duration) => (Some(duration), RETURN_BUFFER_SIZE),
        _ => {
            // We need to figure out how big our return buffer should be based on the response from get_object_attributes
            let attributes = get_object_attributes(bucket_id, object_key)?;
            let Some(ret_buffer_size) = attributes.object_size else {
                return Err(PlaidFunctionError::InternalApiError);
            };

            (None, (ret_buffer_size * 5) as usize) // Bump the return buffer to account for how it will be serialized
        }
    };

    let request = GetObjectRequest {
        bucket_id: bucket_id.to_string(),
        object_key: object_key.to_string(),
        expires_in,
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; return_buffer_size];

    let res = unsafe {
        aws_s3_get_object(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            return_buffer_size,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match serde_json::from_slice(&return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}

/// Returns some or all (up to 1,000) of the objects in a bucket with each request.
/// You can use the request parameters as selection criteria to return a subset of the objects in a bucket.
/// See https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjectsV2.html for full documentation.
///
/// # Arguments
///
/// * `bucket_id` - The S3 bucket name or ARN where objects are stored.
/// * `prefix` - Filters the listed objects to those with keys that begin with this prefix.
/// * `continuation_key` - An optional token to continue pagination from a previous response.
pub fn list_objects(
    bucket_id: &str,
    prefix: &str,
    continuation_key: Option<String>,
) -> Result<ListObjectsResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_s3, list_objects);
    }

    let request = ListObjectsRequest {
        bucket_id: bucket_id.to_string(),
        prefix: prefix.to_string(),
        continuation_key,
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aws_s3_list_objects(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    match serde_json::from_slice(&return_buffer) {
        Ok(x) => Ok(x),
        Err(_) => Err(PlaidFunctionError::InternalApiError),
    }
}
