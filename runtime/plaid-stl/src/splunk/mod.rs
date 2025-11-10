use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

#[derive(Serialize, Deserialize)]
pub struct PostLogRequest {
    pub hec_name: String,
    pub data: String,
    pub blocking: bool,
}

/// Sends a log message to Splunk HEC using a blocking HTTP request.
///
/// This function serializes the provided log data to JSON and sends it to the specified
/// Splunk HEC endpoint. The caller will block until the HTTP request completes and receives
/// a response from the Splunk server.
///
/// # Arguments
/// * `hec_name` - The name of the configured HEC endpoint to send the log to
/// * `log` - The log data to serialize and send (must implement Serialize)
///
/// # Returns
/// * `Ok(())` - Log was successfully delivered to Splunk HEC
/// * `Err(PlaidFunctionError)` - Request failed due to serialization error, network error,
///   or non-success HTTP status code from Splunk
///
/// # Behavior
/// The function will wait for the complete HTTP request/response cycle before returning.
/// Any errors during delivery will propagate back to the caller.
pub fn post_log<T>(hec_name: &str, log: T) -> Result<(), PlaidFunctionError>
where
    T: Serialize,
{
    extern "C" {
        new_host_function!(splunk, post_hec);
    }

    let data = serde_json::to_string(&log).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let request = PostLogRequest {
        hec_name: hec_name.to_string(),
        data,
        blocking: true,
    };

    let params = serde_json::to_string(&request).unwrap();

    let res = unsafe { splunk_post_hec(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Sends a log message to Splunk HEC using a non-blocking HTTP request.
///
/// This function serializes the provided log data to JSON and queues it for delivery
/// to the specified Splunk HEC endpoint. The HTTP request is executed in a background
/// task, allowing the caller to continue immediately without waiting for completion.
///
/// # Arguments
/// * `hec_name` - The name of the configured HEC endpoint to send the log to
/// * `log` - The log data to serialize and send (must implement Serialize)
///
/// # Returns
/// * `Ok(())` - Log was successfully queued for delivery (HTTP outcome unknown)
/// * `Err(PlaidFunctionError)` - Failed to serialize log data or queue the request
///
/// # Behavior
/// The function returns immediately after queuing the request. The actual HTTP delivery
/// happens asynchronously in the background. Network errors or HTTP failures from Splunk
/// are logged internally but do not affect the return value. This makes it suitable for
/// high-throughput logging where delivery confirmation is not critical.
pub fn post_log_non_blocking<T>(hec_name: &str, log: T) -> Result<(), PlaidFunctionError>
where
    T: Serialize,
{
    extern "C" {
        new_host_function!(splunk, post_hec);
    }

    let data = serde_json::to_string(&log).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let request = PostLogRequest {
        hec_name: hec_name.to_string(),
        data,
        blocking: false,
    };

    let params = serde_json::to_string(&request).unwrap();

    let res = unsafe { splunk_post_hec(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
