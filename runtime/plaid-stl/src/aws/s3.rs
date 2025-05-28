use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

/// The maximum size for return buffers used when fetching full objects from S3.
/// Set to 4 MiB.
const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

/// Specifies how an object should be fetched from S3.
pub enum ObjectFetchMode {
    /// Fetch a presigned URL that is valid for the specified number of seconds.
    Presigned(u64),
    /// Fetch the full object contents.
    FullObject,
}

/// Represents the response returned from the `get_object` function.
#[derive(Deserialize, Debug)]
pub enum GetObjectReponse {
    /// The full object data.
    Object(Vec<u8>),
    /// A presigned URI for accessing the object.
    PresignedUri(String),
}

/// Uploads an object to S3
///
/// # Arguments
///
/// * `bucket_id` - The name of the bucket to upload the object to.
/// * `object_key` - The key to identify the object within the bucket.
/// * `object` - The binary data of the object to upload.
///
/// Returns `PlaidFunctionError` if the serialization fails or the host function
/// reports an error (e.g., if the API is not properly configured).
pub fn put_object(
    bucket_id: &str,
    object_key: &str,
    object: Vec<u8>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_s3, put_object);
    }

    #[derive(Serialize)]
    struct PutObjectRequest {
        /// The bucket name to which the `PUT` action was initiated.
        bucket_id: String,
        /// Object data.
        object: Vec<u8>,
        /// Object key for which the `PUT` action was initiated.
        object_key: String,
    }

    let request = PutObjectRequest {
        bucket_id: bucket_id.to_string(),
        object,
        object_key: object_key.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; 0];

    let res = unsafe {
        aws_s3_put_object(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            0,
        )
    };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

/// Fetches an object from S3
///
/// # Arguments
///
/// * `bucket_id` - The name of the bucket from which to fetch the object.
/// * `object_key` - The key identifying the object to fetch.
/// * `fetch_mode` - Specifies whether to fetch the full object or a presigned URL.
///
/// # Returns
///
/// A `GetObjectReponse` indicating either the full object data or a presigned URL.
///
/// # Errors
///
/// Returns `PlaidFunctionError` if serialization fails, the host function reports
/// an error, or if the response cannot be deserialized.
pub fn get_object(
    bucket_id: &str,
    object_key: &str,
    fetch_mode: ObjectFetchMode,
) -> Result<GetObjectReponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_s3, get_object);
    }

    #[derive(Serialize)]
    struct GetObjectRequest {
        /// The bucket name from which the object is requested.
        bucket_id: String,
        /// The key identifying the object to fetch.
        object_key: String,
        /// Optional expiration time for the presigned URL (in seconds).
        expires_in: Option<u64>,
    }

    let (expires_in, return_buffer_size) = match fetch_mode {
        ObjectFetchMode::Presigned(duration) => (Some(duration), 1024),
        _ => (None, RETURN_BUFFER_SIZE),
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
