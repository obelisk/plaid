use std::error::Error;

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
    CacheDisabled,
    CouldNotGetAdequateMemory,
    FailedToWriteGuestMemory,
    StorageLimitReached,
    TestMode,
    OperationNotAllowed,
    SharedDbError,
    TimeoutElapsed,
    Unknown,
    FailedToLogBack,
    LogbackBudgetExhausted,
    InvalidHttpResponseStatus,
    /// An async operation failed. The string describes the underlying error.
    InternalApiErrorWith(String),
    /// The async operation referenced an unknown ticket.
    UnknownTicket,
    /// Too many async operations are outstanding for this rule.
    TooManyPendingTickets,
    /// The state payload attached to an async spawn exceeded the size limit.
    AsyncStateTooLarge,
}

impl Error for PlaidFunctionError {}

impl core::fmt::Display for PlaidFunctionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaidFunctionError::ApiNotConfigured => write!(f, "The requested API is not configured for Plaid to use"),
            PlaidFunctionError::ReturnBufferTooSmall => write!(f, "The return data was too long for the buffer provided"),
            PlaidFunctionError::ErrorCouldNotSerialize => write!(f, "Plaid could not serialize the error"),
            PlaidFunctionError::InternalApiError => write!(f, "An opaque error occurred within Plaid. Talk to the Plaid management team."),
            PlaidFunctionError::ParametersNotUtf8 => write!(f, "Data sent to the Plaid subsystem was not UTF8 so could not be used."),
            PlaidFunctionError::InvalidPointer => write!(f, "We passed a pointer to Plaid that it couldn't use for some reason"),
            PlaidFunctionError::CacheDisabled => write!(f, "The cache is disabled"),
            PlaidFunctionError::CouldNotGetAdequateMemory => write!(f, "Plaid could not get enough memory"),
            PlaidFunctionError::FailedToWriteGuestMemory => write!(f, "Plaid could not write to guest memory"),
            PlaidFunctionError::StorageLimitReached => write!(f, "The storage limit has been reached"),
            PlaidFunctionError::TestMode => write!(f, "The function is not allowed in test mode"),
            PlaidFunctionError::OperationNotAllowed => write!(f, "Operation not allowed"),
            PlaidFunctionError::SharedDbError => write!(f, "Error encountered on a shared DB"),
            PlaidFunctionError::TimeoutElapsed => write!(f, "A timeout has elapsed"),
            PlaidFunctionError::Unknown => write!(f, "An unknown error occurred. This can happen if the Plaid runtime is newer than the STL this rule was compiled against."),
            PlaidFunctionError::FailedToLogBack => write!(f, "Failed to dispatch log message: the receiver is disconnected or at capacity"),
            PlaidFunctionError::LogbackBudgetExhausted => write!(f, "Logback budget exhausted"),
            PlaidFunctionError::InvalidHttpResponseStatus => write!(f, "Invalid HTTP response status"),
            PlaidFunctionError::InternalApiErrorWith(e) => write!(f, "An async operation failed: {e}"),
            PlaidFunctionError::UnknownTicket => write!(f, "The referenced async ticket is unknown: it never existed, was already claimed, or expired"),
            PlaidFunctionError::TooManyPendingTickets => write!(f, "Too many async operations are outstanding for this rule"),
            PlaidFunctionError::AsyncStateTooLarge => write!(f, "The state attached to an async spawn exceeded the size limit"),
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
            -7 => Self::CacheDisabled,
            -8 => Self::CouldNotGetAdequateMemory,
            -9 => Self::FailedToWriteGuestMemory,
            -10 => Self::StorageLimitReached,
            -11 => Self::TestMode,
            -12 => Self::OperationNotAllowed,
            -13 => Self::SharedDbError,
            -14 => Self::TimeoutElapsed,
            -15 => Self::FailedToLogBack,
            -16 => Self::LogbackBudgetExhausted,
            -17 => Self::InvalidHttpResponseStatus,
            -18 => Self::UnknownTicket,
            -19 => Self::TooManyPendingTickets,
            -20 => Self::AsyncStateTooLarge,
            _ => Self::Unknown,
        }
    }
}

impl From<PlaidFunctionError> for i32 {
    fn from(e: PlaidFunctionError) -> Self {
        match e {
            PlaidFunctionError::ApiNotConfigured => -1,
            PlaidFunctionError::ReturnBufferTooSmall => -2,
            PlaidFunctionError::ErrorCouldNotSerialize => -3,
            PlaidFunctionError::InternalApiError => -4,
            PlaidFunctionError::ParametersNotUtf8 => -5,
            PlaidFunctionError::InvalidPointer => -6,
            PlaidFunctionError::CacheDisabled => -7,
            PlaidFunctionError::CouldNotGetAdequateMemory => -8,
            PlaidFunctionError::FailedToWriteGuestMemory => -9,
            PlaidFunctionError::StorageLimitReached => -10,
            PlaidFunctionError::TestMode => -11,
            PlaidFunctionError::OperationNotAllowed => -12,
            PlaidFunctionError::SharedDbError => -13,
            PlaidFunctionError::TimeoutElapsed => -14,
            PlaidFunctionError::FailedToLogBack => -15,
            PlaidFunctionError::LogbackBudgetExhausted => -16,
            PlaidFunctionError::InvalidHttpResponseStatus => -17,
            PlaidFunctionError::UnknownTicket => -18,
            PlaidFunctionError::TooManyPendingTickets => -19,
            PlaidFunctionError::AsyncStateTooLarge => -20,
            // Errors that carry a payload have no stable numeric code: map
            // them to the generic internal error. The payload is preserved
            // on the Rust side via Display / the AsyncResult envelope.
            PlaidFunctionError::InternalApiErrorWith(_) => -4,
            PlaidFunctionError::Unknown => -100,
        }
    }
}

/// A response returned by a rule serving a webhook request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookResponse {
    /// The HTTP status to return to the webhook sender.
    pub status: u16,
    /// The response body to return to the webhook sender.
    pub body: String,
}

impl WebhookResponse {
    /// Construct a webhook response. The runtime validates that `status` is a
    /// valid HTTP status code when the response is served.
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

pub mod r#async;
pub mod aws;
pub mod blockchain;
pub mod bloom_filter;
pub mod cryptography;
pub mod gcp;
pub mod github;
pub mod jira;
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

pub mod datetime;
pub mod messages;

#[macro_export]
macro_rules! set_panic_hook {
    () => {
        std::panic::set_hook(Box::new(move |panic_info| {
            extern "C" {
                fn set_error_context(data_buffer: *const u8, buffer_size: u32);
            }
            if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
                set_error_context(s.as_ptr(), s.len() as u32);
            } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
                set_error_context(s.as_str().as_ptr(), s.len() as u32);
            } else {
                set_error_context("Unknown panic type".as_ptr(), 18);
            }
        }));
    };
}

#[macro_export]
macro_rules! entrypoint {
    () => {
        use plaid_stl::{plaid::set_error_context, set_panic_hook};

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
                    set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}

#[macro_export]
macro_rules! entrypoint_with_source {
    () => {
        use plaid_stl::{plaid::set_error_context, set_panic_hook};

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            extern "C" {
                fn fetch_data_and_source(data_buffer: *mut u8, buffer_size: u32) -> i32;
            }

            let buffer_size = fetch_data_and_source(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 4 {
                return -3;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data_and_source(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 4 {
                return -4;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log_length = u32::from_le_bytes(data_buffer[0..4].try_into().unwrap()) as usize;

            let log = &data_buffer[4..4 + log_length];
            let log = match String::from_utf8(log.to_vec()) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            let log_source = &data_buffer[4 + log_length..];
            let source = match serde_json::from_slice::<LogSource>(log_source) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            set_panic_hook!();

            match main(log, source) {
                Ok(_) => 0,
                Err(e) => {
                    set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}

#[macro_export]
macro_rules! entrypoint_with_source_and_response {
    () => {
        use plaid_stl::{plaid::set_error_context, set_panic_hook};

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            extern "C" {
                fn fetch_data_and_source(data_buffer: *mut u8, buffer_size: u32) -> i32;
                fn set_response(data_buffer: *const u8, buffer_size: u32);
            }

            let buffer_size = fetch_data_and_source(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 4 {
                return -3;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data_and_source(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 4 {
                return -4;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log_length = u32::from_le_bytes(data_buffer[0..4].try_into().unwrap()) as usize;

            let log = &data_buffer[4..4 + log_length];
            let log = match String::from_utf8(log.to_vec()) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            let log_source = &data_buffer[4 + log_length..];
            let source = match serde_json::from_slice::<LogSource>(log_source) {
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
                    set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}

/// Generate an entrypoint for a rule that returns a body and HTTP status for a
/// webhook request. The rule's `main` function must return
/// `Result<Option<WebhookResponse>, _>`.
#[macro_export]
macro_rules! entrypoint_with_source_and_webhook_response {
    () => {
        use plaid_stl::{plaid::set_error_context, set_panic_hook};

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            extern "C" {
                fn fetch_data_and_source(data_buffer: *mut u8, buffer_size: u32) -> i32;
                fn set_response_status(status: u32) -> i32;
                fn set_response(data_buffer: *const u8, buffer_size: u32);
            }

            let buffer_size = fetch_data_and_source(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 4 {
                return -3;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data_and_source(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 4 {
                return -4;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log_length = u32::from_le_bytes(data_buffer[0..4].try_into().unwrap()) as usize;

            let log = &data_buffer[4..4 + log_length];
            let log = match String::from_utf8(log.to_vec()) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            let log_source = &data_buffer[4 + log_length..];
            let source = match serde_json::from_slice::<LogSource>(log_source) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            set_panic_hook!();

            match main(log, source) {
                Ok(Some(response)) => {
                    let status_result = unsafe { set_response_status(response.status.into()) };
                    if status_result != 0 {
                        set_error_context("Invalid HTTP response status");
                        return 1;
                    }

                    let response_bytes = response.body.as_bytes().to_vec();
                    unsafe {
                        set_response(response_bytes.as_ptr(), response_bytes.len() as u32);
                    };
                    0
                }
                Ok(None) => 0,
                Err(e) => {
                    set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}

#[macro_export]
macro_rules! entrypoint_vec_with_source {
    () => {
        use plaid_stl::{plaid::set_error_context, set_panic_hook};

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            extern "C" {
                fn fetch_data_and_source(data_buffer: *mut u8, buffer_size: u32) -> i32;
            }

            let buffer_size = fetch_data_and_source(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 4 {
                return buffer_size;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data_and_source(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 4 {
                return copied_size;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log_length = u32::from_le_bytes(data_buffer[0..4].try_into().unwrap()) as usize;

            let log = &data_buffer[4..4 + log_length];

            // We keep the log as a Vec<u8>
            let log = log.to_vec();

            let log_source = &data_buffer[4 + log_length..];
            let source = match serde_json::from_slice::<LogSource>(log_source) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            set_panic_hook!();

            match main(log, source) {
                Ok(_) => 0,
                Err(e) => {
                    set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}

/// Generate an entrypoint for a rule that uses async operations.
///
/// The rule must define:
///
/// * `fn main(data: String, source: LogSource, ctx: AsyncContext) -> Result<(), i32>`
///   for normal invocations (webhooks, generators, logbacks), and
/// * one `fn <name>(completion: AsyncCompletion, ctx: AsyncContext) -> Result<(), i32>`
///   for every continuation name passed to `AsyncContext::spawn` /
///   `AsyncContext::spawn_with_state`.
///
/// When an async operation completes, the runtime re-invokes the rule with a
/// completion message. This macro detects that message and dispatches to the
/// continuation handler with the same name that was registered at spawn
/// time. An unknown continuation name is a rule error (surfaced through the
/// error context), not a panic.
///
/// Example:
///
/// ```ignore
/// use plaid_stl::{
///     async::{AsyncCompletion, AsyncContext},
///     entrypoint_with_async,
///     messages::LogSource,
/// };
///
/// entrypoint_with_async!(on_done);
///
/// fn main(data: String, _source: LogSource, ctx: AsyncContext) -> Result<(), i32> {
///     let ticket = ctx.spawn("general_make_named_request", &params, "on_done", 1)?;
///     plaid::print_debug_string(&format!("waiting on {ticket}"));
///     Ok(())
/// }
///
/// fn on_done(completion: AsyncCompletion, _ctx: AsyncContext) -> Result<(), i32> {
///     let body = completion.result.as_string()?;
///     plaid::print_debug_string(&format!("got {body}"));
///     Ok(())
/// }
/// ```
#[macro_export]
macro_rules! entrypoint_with_async {
    ($($handler:ident),+ $(,)?) => {
        // The macro refers to its dependencies with fully-qualified paths so
        // it never conflicts with the rule's own imports.
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint() -> i32 {
            use plaid_stl::plaid::set_error_context;
            use plaid_stl::set_panic_hook;

            extern "C" {
                fn fetch_data_and_source(data_buffer: *mut u8, buffer_size: u32) -> i32;
            }

            let buffer_size = fetch_data_and_source(vec![].as_mut_ptr(), 0);
            let buffer_size = if buffer_size < 4 {
                return -3;
            } else {
                buffer_size as u32
            };

            let mut data_buffer = vec![0; buffer_size as usize];

            let copied_size = fetch_data_and_source(data_buffer.as_mut_ptr(), buffer_size);
            let copied_size = if copied_size < 4 {
                return -4;
            } else {
                copied_size as u32
            };

            if copied_size != buffer_size {
                return -1;
            }

            let log_length = u32::from_le_bytes(data_buffer[0..4].try_into().unwrap()) as usize;

            let log = &data_buffer[4..4 + log_length];
            let log = match String::from_utf8(log.to_vec()) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            let log_source = &data_buffer[4 + log_length..];
            let source = match serde_json::from_slice::<plaid_stl::messages::LogSource>(log_source) {
                Ok(s) => s,
                Err(_) => return -2,
            };

            set_panic_hook!();

            // Async completions are delivered as a message whose source is
            // LogSource::AsyncCompletion. Route those to the continuation
            // handler; everything else goes to main.
            if let plaid_stl::messages::LogSource::AsyncCompletion(_) = source {
                let completion: plaid_stl::r#async::AsyncCompletion =
                    match serde_json::from_str(&log) {
                        Ok(c) => c,
                        Err(e) => {
                            set_error_context(&format!("Failed to decode async completion: {e}"));
                            return 1;
                        }
                    };

                let continuation = completion.continuation.clone();

                return match continuation.as_str() {
                    $(
                        stringify!($handler) => {
                            match $handler(completion, plaid_stl::r#async::AsyncContext) {
                                Ok(_) => 0,
                                Err(e) => {
                                    set_error_context(&e.to_string());
                                    1
                                }
                            }
                        }
                    )*
                    other => {
                        set_error_context(&format!(
                            "Async completion referenced unknown continuation handler: {other}"
                        ));
                        1
                    }
                };
            }

            match main(log, source, plaid_stl::r#async::AsyncContext) {
                Ok(_) => 0,
                Err(e) => {
                    set_error_context(&e.to_string());
                    1
                }
            }
        }
    };
}
