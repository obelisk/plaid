use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

pub enum ObjectFetchMode {
    Presigned(u64),
    FullObject,
}

#[derive(Deserialize, Debug)]
pub enum GetObjectReponse {
    Object(Vec<u8>),
    PresignedUri(String),
}

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
        object: object,
        object_key: object_key.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aws_s3_put_object(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

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
        bucket_id: String,
        object_key: String,
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
