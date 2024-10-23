use std::fmt::Display;

use crate::PlaidFunctionError;

pub mod cache;
pub mod random;
pub mod storage;

pub fn print_debug_string(log: &str) {
    extern "C" {
        /// Send a log to a Slack Webhook
        fn print_debug_string(log: *const u8, log_len: usize);
    }

    let log_bytes = log.as_bytes().to_vec();
    unsafe {
        print_debug_string(log_bytes.as_ptr(), log_bytes.len());
    };
}

/// Send a log to the logback system with no extra budget to trigger
/// further invocations. This is the most limited log_back but will fail
/// if the module has no more LogbacksAllowed.
pub fn log_back(type_: &str, log: &[u8], delay: u32) -> Result<(), i32> {
    log_back_with_budget(type_, log, delay, 0)
}

/// Send a log to the logback system with unlimited extra budget to trigger
/// further invocations. Whoever will pick up the message will also have an
/// unlimited budget for future invocations.
pub fn log_back_unlimited(type_: &str, log: &[u8], delay: u32) -> Result<(), i32> {
    extern "C" {
        /// Send a log to the logback system
        fn log_back_unlimited(
            type_: *const u8,
            type_len: usize,
            log: *const u8,
            log_len: usize,
            delay: u32,
        ) -> u32;
    }

    let type_bytes = type_.as_bytes().to_vec();
    unsafe {
        log_back_unlimited(
            type_bytes.as_ptr(),
            type_bytes.len(),
            log.as_ptr(),
            log.len(),
            delay,
        );
    };

    Ok(())
}

/// Send a log to the logback system with a budget to trigger further
/// invoations. The requested budget will be substracted from this invocation's
/// budget and the call will fail if it is exceeded. Calling this function itself
/// costs 1 budget, with a budget of 1, you must set logbacks_allowed to 0. This
/// means that those further invocations will not be able to trigger logbacks.
pub fn log_back_with_budget(
    type_: &str,
    log: &[u8],
    delay: u32,
    logbacks_allowed: u32,
) -> Result<(), i32> {
    extern "C" {
        /// Send a log to the logback system
        fn log_back(
            type_: *const u8,
            type_len: usize,
            log: *const u8,
            log_len: usize,
            delay: u32,
            logbacks_allowed: u32,
        ) -> u32;
    }

    let type_bytes = type_.as_bytes().to_vec();
    unsafe {
        log_back(
            type_bytes.as_ptr(),
            type_bytes.len(),
            log.as_ptr(),
            log.len(),
            delay,
            logbacks_allowed,
        );
    };

    Ok(())
}

pub fn get_time() -> u32 {
    extern "C" {
        /// Get time from host
        fn get_time() -> u32;
    }

    unsafe { get_time() }
}

pub fn get_accessory_data_by_name(name: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        fn fetch_accessory_data_by_name(
            name: *const u8,
            name_len: usize,
            data_buffer: *mut u8,
            buffer_size: u32,
        ) -> i32;
    }

    let name_bytes = name.as_bytes().to_vec();

    let buffer_size = unsafe {
        fetch_accessory_data_by_name(
            name_bytes.as_ptr(),
            name_bytes.len(),
            vec![].as_mut_ptr(),
            0,
        )
    };

    let buffer_size = if buffer_size < 0 {
        return Err(buffer_size.into());
    } else {
        buffer_size as u32
    };

    let mut data_buffer = vec![0; buffer_size as usize];

    let copied_size = unsafe {
        fetch_accessory_data_by_name(
            name_bytes.as_ptr(),
            name_bytes.len(),
            data_buffer.as_mut_ptr(),
            buffer_size,
        )
    };
    let copied_size = if copied_size < 0 {
        return Err(copied_size.into());
    } else {
        copied_size as u32
    };

    if copied_size != buffer_size {
        return Err(PlaidFunctionError::InternalApiError);
    }

    match String::from_utf8(data_buffer) {
        Ok(s) => Ok(s),
        Err(_) => Err(PlaidFunctionError::ParametersNotUtf8),
    }
}

/// Get the persistent response set by a previous invocation
/// of the module
pub fn get_response() -> Result<String, PlaidFunctionError> {
    extern "C" {
        fn get_response(data_buffer: *mut u8, buffer_size: u32) -> i32;
    }

    let buffer_size = unsafe { get_response(vec![].as_mut_ptr(), 0) };

    let buffer_size = if buffer_size < 0 {
        return Err(buffer_size.into());
    } else {
        buffer_size as u32
    };

    let mut data_buffer = vec![0; buffer_size as usize];

    let copied_size = unsafe { get_response(data_buffer.as_mut_ptr(), buffer_size) };
    let copied_size = if copied_size < 0 {
        return Err(copied_size.into());
    } else {
        copied_size as u32
    };

    if copied_size != buffer_size {
        return Err(PlaidFunctionError::InternalApiError);
    }

    match String::from_utf8(data_buffer) {
        Ok(s) => Ok(s),
        Err(_) => Err(PlaidFunctionError::ParametersNotUtf8),
    }
}

/// Give the runtime more context about an error encountered during execution
pub fn set_error_context(context: impl Display) {
    extern "C" {
        fn set_error_context(data_buffer: *const u8, buffer_size: u32);
    }
    let context_bytes = context.to_string().as_bytes().to_vec();
    unsafe {
        set_error_context(context_bytes.as_ptr(), context_bytes.len() as u32);
    };
}
