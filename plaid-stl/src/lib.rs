macro_rules! new_host_function_with_error_buffer {
    ($api:ident, $function_name:ident) => {
        paste::item! {
            fn [< $api _ $function_name >] (params_buffer: *const u8, params_buffer_len: usize, ret_buffer: *mut u8, ret_buffer_len: usize) -> i32;
        }
    }
}

macro_rules! new_host_function {
    ($api:ident, $function_name:ident) => {
        paste::item! {
            fn [< $api _ $function_name >] (params_buffer: *const u8, params_buffer_len: usize) -> i32;
        }
    }
}

#[derive(Debug)]
pub enum PlaidFunctionError {
    ApiNotConfigured,
    ReturnBufferTooSmall,
    ErrorCouldNotSerialize,
    InternalApiError,
    ParametersNotUtf8,
    InvalidPointer,
    Unknown,
}

impl core::fmt::Display for PlaidFunctionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaidFunctionError::ApiNotConfigured => write!(f, "The requested API is not configured for Plaid to use"),
            PlaidFunctionError::ReturnBufferTooSmall => write!(f, "The return data was too long for the buffer provided"),
            PlaidFunctionError::ErrorCouldNotSerialize => write!(f, "Plaid could not serialize the error"),
            PlaidFunctionError::InternalApiError => write!(f, "An opaque error occurred within Plaid. Talk to the Plaid management team."),
            PlaidFunctionError::ParametersNotUtf8 => write!(f, "Data sent to the Plaid subsystem was not UTF8 so could not be used."),
            PlaidFunctionError::InvalidPointer => write!(f, "We passed a pointer to Plaid that it couldn't use for some reason"),
            PlaidFunctionError::Unknown => write!(f, "An unknown error occurred. This can happen if the Plaid runtime is never than the STL this rule was compiled against."),

        }
    }
}

impl From<i32> for PlaidFunctionError {
    fn from(code: i32) -> Self {
        match code {
            -1 => Self::ApiNotConfigured,
            -2 => Self::ReturnBufferTooSmall,
            -3 => Self::ErrorCouldNotSerialize,
            -4 => Self::InternalApiError,
            -5 => Self::ParametersNotUtf8,
            -6 => Self::InvalidPointer,
            _ => Self::Unknown,
        }
    }
}

impl From<PlaidFunctionError> for i32 {
    fn from(e: PlaidFunctionError) -> Self {
        e as i32
    }
}

pub mod aws;
pub mod github;
pub mod network;
pub mod npm;
pub mod okta;
pub mod pagerduty;
pub mod plaid;
pub mod quorum;
pub mod rustica;
pub mod slack;
pub mod splunk;
pub mod web;
pub mod yubikey;

pub mod messages;
pub mod datetime;

//pub use ::plaid::LogSource;

#[macro_export]
macro_rules! set_panic_hook {
    () => {
        use std::sync::{Arc, Mutex};
        let buffer = Arc::new(Mutex::new([0u8; 512]));
        let buffer_clone = Arc::clone(&buffer);

        std::panic::set_hook(Box::new(move |panic_info| {
            let bytes = panic_info
                .payload()
                .downcast_ref::<&str>()
                .unwrap()
                .as_bytes();
            let mut buffer_lock = buffer_clone.lock().unwrap();

            unsafe {
                // Get raw pointers to the data and buffer
                let dest_ptr = buffer_lock.as_mut_ptr();
                let src_ptr = bytes.as_ptr();

                // Copy data into the buffer
                std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, bytes.len());
            }

            let message = std::str::from_utf8(&*buffer_lock).unwrap_or("[Invalid UTF-8]");
            plaid::set_error_context(message);
        }));
    };
}

#[macro_export]
macro_rules! entrypoint {
    () => {
        use plaid_stl::set_panic_hook;

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            extern "C" {
                fn fetch_data(data_buffer: *mut u8, buffer_size: u32) -> i32;
            }

            let buffer_size = fetch_data(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 0 {
                return buffer_size;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 0 {
                return copied_size;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log = match String::from_utf8(data_buffer) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            set_panic_hook!();

            match main(log) {
                Ok(_) => 0,
                Err(e) => {
                    plaid::set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}

#[macro_export]
macro_rules! entrypoint_with_source {
    () => {
        use plaid_stl::set_panic_hook;

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            extern "C" {
                fn fetch_data(data_buffer: *mut u8, buffer_size: u32) -> i32;
                fn fetch_source(data_buffer: *mut u8, buffer_size: u32) -> i32;
            }

            let buffer_size = fetch_data(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 0 {
                return buffer_size;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 0 {
                return copied_size;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log = match String::from_utf8(data_buffer) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            let buffer_size = fetch_source(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 0 {
                return buffer_size;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];
            let copied_size = fetch_source(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 0 {
                return copied_size;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let source = match serde_json::from_slice::<LogSource>(&data_buffer) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            set_panic_hook!();

            match main(log, source) {
                Ok(_) => 0,
                Err(e) => {
                    plaid::set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}

#[macro_export]
macro_rules! entrypoint_with_source_and_response {
    () => {
        use plaid_stl::set_panic_hook;

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            extern "C" {
                fn fetch_data(data_buffer: *mut u8, buffer_size: u32) -> i32;
                fn fetch_source(data_buffer: *mut u8, buffer_size: u32) -> i32;
                fn set_response(data_buffer: *const u8, buffer_size: u32);
            }

            let buffer_size = fetch_data(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 0 {
                return buffer_size;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 0 {
                return copied_size;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log = match String::from_utf8(data_buffer) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            let buffer_size = fetch_source(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 0 {
                return buffer_size;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];
            let copied_size = fetch_source(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 0 {
                return copied_size;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let source = match serde_json::from_slice::<LogSource>(&data_buffer) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            set_panic_hook!();

            match main(log, source) {
                Ok(Some(response)) => {
                    let response_bytes = response.as_bytes().to_vec();
                    unsafe {
                        set_response(response_bytes.as_ptr(), response_bytes.len() as u32);
                    };
                    0
                }
                Ok(None) => 0,
                Err(e) => {
                    plaid::set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}
