use super::{safely_write_data_back, FunctionErrors};
use crate::apis::ApiError;
use crate::executor::Env;
use crate::functions::{get_memory, safely_get_string};
use wasmer::{AsStoreRef, Function, FunctionEnv, FunctionEnvMut, Store, WasmPtr};

const ALLOW_IN_TEST_MODE: bool = true;
const DISALLOW_IN_TEST_MODE: bool = false;

/// Macro to implement a new host function in a given API. The function does not fill a data buffer with returned values.
///
/// This macro generates two functions:
/// - A private implementation function (`_impl`) that handles the actual logic:
///   - Accessing and validating memory from the guest
///   - Checking that the API is configured.
///   - Running the function + returning the result (as an i32)
/// - A public wrapper function that calls the implementation function and returns the result as an integer.
///
/// # Parameters
/// - `$api`: The name of the API (e.g., `github`).
/// - `$function_name`: The name of the function to be implemented.
///
/// # Error Handling
/// The generated implementation function returns `FunctionErrors` in case of failures, which are then
/// converted to int error codes by the wrapper function. These errors include:
/// - `FunctionErrors::InternalApiError`: For internal API-related errors.
/// - `FunctionErrors::ApiNotConfigured`: If the API is not configured.
macro_rules! impl_new_function {
    ($api:ident, $function_name:ident, $allow_in_test_mode:expr) => {
        paste::item! {
            fn [< $api _ $function_name _impl>] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32) -> Result<i32, FunctionErrors> {
                let store = env.as_store_ref();
                let env_data = env.data();

                if let Err(e) = env_data.external_logging_system.log_function_call(env_data.module.name.clone(), stringify!([< $api _ $function_name >]).to_string(), env_data.module.test_mode) {
                    error!("Logging system is not working!!: {:?}", e);
                    return Err(FunctionErrors::InternalApiError);
                }


                // Disallow this function call from continuing if the module is in test mode
                if !$allow_in_test_mode && env_data.module.test_mode {
                    return Err(FunctionErrors::TestMode);
                }

                let memory_view = match get_memory(&env, &store) {
                    Ok(memory_view) => memory_view,
                    Err(e) => {
                        error!("{}: Memory error in {}: {:?}", env_data.module.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    },
                };

                let params = safely_get_string(&memory_view, params_buffer, params_buffer_len)?;

                // Check that the request API system is even configured.
                // This is something like Okta, Slack, or GitHub
                let api = env_data.api.$api.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;

                // Clone the APIs Arc to use in Tokio closure
                let env_api = env_data.api.clone();
                let module = env_data.module.clone();
                // Run the function on the Tokio runtime and wait for the result
                let result = env_api.runtime.block_on(async move {
                    api.$function_name(&params, module).await
                });

                let return_data = match result {
                    Ok(return_data) => return_data,
                    Err(ApiError::TestMode) => {
                        return Err(FunctionErrors::TestMode);
                    }
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", env_data.module.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                trace!("{} is calling {} got a return data of {}", env_data.module.name, stringify!([< $api _ $function_name >]), return_data);
                return Ok(return_data as i32);
            }

            fn [< $api _ $function_name >] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32) -> i32 {
                let name = env.data().module.name.clone();
                match [< $api _ $function_name _impl>](env, params_buffer, params_buffer_len) {
                    Ok(res) => res,
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", name, stringify!([< $api _ $function_name >]), e);
                        e as i32
                    }
                }
            }
        }
    }
}

/// Macro to implement a new host function in a given API.
///
/// This macro generates two functions:
/// - A private implementation function (`_impl`) that handles the actual logic:
///   - Accessing and validating memory from the guest
///   - Checking that the API is configured.
///   - Running the function + returning the result and handling errors
/// - A public wrapper function that calls the implementation function and returns the result as an integer.
///
/// # Parameters
/// - `$api`: The name of the API (e.g., `github`).
/// - `$function_name`: The name of the function to be implemented.
///
/// # Error Handling
/// The generated implementation function returns `FunctionErrors` in case of failures, which are then
/// converted to int error codes by the wrapper function. These errors include:
/// - `FunctionErrors::InternalApiError`: For internal API-related errors.
/// - `FunctionErrors::ApiNotConfigured`: If the API is not configured.
/// - `FunctionErrors::ReturnBufferTooSmall`: If the provided return buffer is too small to hold the result.
macro_rules! impl_new_function_with_error_buffer {
    ($api:ident, $function_name:ident, $allow_in_test_mode:expr) => {
        paste::item! {
            fn [< $api _ $function_name _impl>] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> Result<i32, FunctionErrors> {
                let store = env.as_store_ref();
                let env_data = env.data();

                if let Err(e) = env_data.external_logging_system.log_function_call(env_data.module.name.clone(), stringify!([< $api _ $function_name >]).to_string(), env_data.module.test_mode) {
                    error!("Logging system is not working!!: {:?}", e);
                    return Err(FunctionErrors::InternalApiError);
                }

                // Disallow this function call from continuing if the module is in test mode
                if !$allow_in_test_mode && env_data.module.test_mode {
                    return Err(FunctionErrors::TestMode);
                }

                let memory_view = match get_memory(&env, &store) {
                    Ok(memory_view) => memory_view,
                    Err(e) => {
                        error!("{}: Memory error in {}: {:?}", env_data.module.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    },
                };

                let params = safely_get_string(&memory_view, params_buffer, params_buffer_len)?;

                // Check the requested API system is configured.
                let api = env_data.api.$api.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;

                // Clone the APIs Arc to use in Tokio closure
                let env_api = env_data.api.clone();
                let module = env_data.module.clone();
                // Run the function on the Tokio runtime and wait for the result
                let result = env_api.runtime.block_on(async move {
                    api.$function_name(&params, module).await
                });

                let return_data = match result {
                    Ok(return_data) => return_data,
                    Err(ApiError::TestMode) => {
                        return Err(FunctionErrors::TestMode);
                    }
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", env_data.module.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                if return_data.len() > ret_buffer_len as usize {
                    error!("{} could not receive data from {} because it provided a return buffer that was too small. Got {}, needed {}", env_data.module.name, stringify!([< $api _ $function_name >]), ret_buffer_len, return_data.len());
                    trace!("Data: {}", return_data);
                    return Err(FunctionErrors::ReturnBufferTooSmall);
                }

                safely_write_data_back(&memory_view, return_data.as_bytes(), ret_buffer, ret_buffer_len)?;

                trace!("{} is calling {} got a return data length of {}", env_data.module.name, stringify!([< $api _ $function_name >]), return_data.len());
                return Ok(return_data.len() as i32);
            }

            fn [< $api _ $function_name >] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> i32 {
                let name = env.data().module.name.clone();
                match [< $api _ $function_name _impl>](env, params_buffer, params_buffer_len, ret_buffer, ret_buffer_len) {
                    Ok(res) => res,
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", name, stringify!([< $api _ $function_name >]), e);
                        e as i32
                    }
                }
            }
        }
    }
}

/// Macro to implement a function in a specific API's submodule.
///
/// This macro generates two functions:
/// - A private implementation function (`_impl`) that handles the actual logic:
///   - Accessing and validating memory from the guest
///   - Checking that the API is configured.
///   - Running the function + returning the result and handling errors
/// - A public wrapper function that calls the implementation function and returns the result as an integer.
///
/// # Parameters
/// - `$api`: The name of the API (e.g., `aws`).
/// - `$sub_module`: The name of the submodule within the API (e.g., `kms`).
/// - `$function_name`: The name of the function to be implemented (e.g., `put_object`, `encrypt`).
///
/// # Error Handling
/// The generated implementation function returns `FunctionErrors` in case of failures, which are then
/// converted to int error codes by the wrapper function. These errors include:
/// - `FunctionErrors::InternalApiError`: For internal API-related errors.
/// - `FunctionErrors::ApiNotConfigured`: If the API is not configured.
/// - `FunctionErrors::ReturnBufferTooSmall`: If the provided return buffer is too small to hold the result.
#[allow(unused_macros)] // not to have a warning when compiling without the `aws` feature
macro_rules! impl_new_sub_module_function_with_error_buffer {
    ($api:ident, $sub_module:ident, $function_name:ident, $allow_in_test_mode:expr) => {
        paste::item! {
            fn [< $api _ $sub_module _ $function_name _impl>] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> Result<i32, FunctionErrors> {
                let store = env.as_store_ref();
                let env_data = env.data();

                // Log function call by module
                if let Err(e) = env_data.external_logging_system.log_function_call(env_data.module.name.clone(), stringify!([< $api _ $sub_module _ $function_name >]).to_string(), env_data.module.test_mode) {
                    error!("Logging system is not working!!: {:?}", e);
                    return Err(FunctionErrors::InternalApiError);
                }


                // Disallow this function call from continuing if the module is in test mode
                if !$allow_in_test_mode && env_data.module.test_mode {
                    return Err(FunctionErrors::TestMode);
                }

                let memory_view = match get_memory(&env, &store) {
                    Ok(memory_view) => memory_view,
                    Err(e) => {
                        error!("{}: Memory error in {}: {:?}", env_data.module.name, stringify!([< $api _ $sub_module _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    },
                };

                let params = safely_get_string(&memory_view, params_buffer, params_buffer_len)?;

                // Check that AWS API is configured
                let aws = env_data.api.$api.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;
                let sub_module = aws.$sub_module.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;

                // Clone the APIs Arc to use in Tokio closure
                let env_api = env_data.api.clone();
                let module = env_data.module.clone();
                // Run the function on the Tokio runtime and wait for the result
                let result = env_api.runtime.block_on(async move {
                    sub_module.$function_name(&params, module).await
                });

                let return_data = match result {
                    Ok(return_data) => return_data,
                    Err(ApiError::TestMode) => {
                        return Err(FunctionErrors::TestMode);
                    }
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", env_data.module.name, stringify!([< $api _ $sub_module _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                if return_data.len() > ret_buffer_len as usize {
                    error!("{} could not receive data from {} because it provided a return buffer that was too small. Got {}, needed {}", env_data.module.name,  stringify!([< $api _ $sub_module _ $function_name >]), ret_buffer_len, return_data.len());
                    trace!("Data: {}", return_data);
                    return Err(FunctionErrors::ReturnBufferTooSmall);
                }

                safely_write_data_back(&memory_view, return_data.as_bytes(), ret_buffer, ret_buffer_len)?;

                trace!("{} is calling {} got a return data length of {}", env_data.module.name,  stringify!([< $api _ $sub_module _ $function_name >]), return_data.len());
                return Ok(return_data.len() as i32);
            }

            fn [< $api _ $sub_module _ $function_name >] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> i32 {
                let name = env.data().module.name.clone();
                match [< $api _ $sub_module _ $function_name _impl>](env, params_buffer, params_buffer_len, ret_buffer, ret_buffer_len) {
                    Ok(res) => res,
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", name,  stringify!([< $api _ $sub_module _ $function_name >]), e);
                        e as i32
                    }
                }
            }
        }
    }
}

/// Macro to implement a new host function in a given API. The function does not fill a data buffer with returned values.
///
/// This macro generates two functions:
/// - A private implementation function (`_impl`) that handles the actual logic:
///   - Accessing and validating memory from the guest
///   - Checking that the API is configured.
///   - Running the function + returning the result (as an i32)
/// - A public wrapper function that calls the implementation function and returns the result as an integer.
///
/// # Parameters
/// - `$api`: The name of the API (e.g., `aws`).
/// - `$sub_module`: The name of the submodule (e.g., `s3`).
/// - `$function_name`: The name of the function to be implemented.
///
/// # Error Handling
/// The generated implementation function returns `FunctionErrors` in case of failures, which are then
/// converted to int error codes by the wrapper function. These errors include:
/// - `FunctionErrors::InternalApiError`: For internal API-related errors.
/// - `FunctionErrors::ApiNotConfigured`: If the API is not configured.
macro_rules! impl_new_sub_module_function {
    ($api:ident, $sub_module:ident, $function_name:ident, $allow_in_test_mode:expr) => {
        paste::item! {
            fn [< $api _ $sub_module _ $function_name _impl>] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32) -> Result<i32, FunctionErrors> {
                let store = env.as_store_ref();
                let env_data = env.data();

                if let Err(e) = env_data.external_logging_system.log_function_call(env_data.module.name.clone(), stringify!([< $api _ $function_name >]).to_string(), env_data.module.test_mode) {
                    error!("Logging system is not working!!: {:?}", e);
                    return Err(FunctionErrors::InternalApiError);
                }


                // Disallow this function call from continuing if the module is in test mode
                if !$allow_in_test_mode && env_data.module.test_mode {
                    return Err(FunctionErrors::TestMode);
                }

                let memory_view = match get_memory(&env, &store) {
                    Ok(memory_view) => memory_view,
                    Err(e) => {
                        error!("{}: Memory error in {}: {:?}", env_data.module.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    },
                };

                let params = safely_get_string(&memory_view, params_buffer, params_buffer_len)?;

                // Check that API is configured
                let api = env_data.api.$api.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;
                let sub_module = api.$sub_module.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;

                // Clone the APIs Arc to use in Tokio closure
                let env_api = env_data.api.clone();
                let module = env_data.module.clone();
                // Run the function on the Tokio runtime and wait for the result
                let result = env_api.runtime.block_on(async move {
                    sub_module.$function_name(&params, module).await
                });

                let return_data = match result {
                    Ok(return_data) => return_data,
                    Err(ApiError::TestMode) => {
                        return Err(FunctionErrors::TestMode);
                    }
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", env_data.module.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                trace!("{} is calling {} got a return data of {}", env_data.module.name, stringify!([< $api _ $function_name >]), return_data);
                return Ok(return_data as i32);
            }

            fn [< $api _ $sub_module _ $function_name >] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32) -> i32 {
                let name = env.data().module.name.clone();
                match [< $api _ $sub_module _ $function_name _impl>](env, params_buffer, params_buffer_len) {
                    Ok(res) => res,
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", name,  stringify!([< $api _ $sub_module _ $function_name >]), e);
                        e as i32
                    }
                }
            }
        }
    }
}

// General Functions
impl_new_function!(general, simple_json_post_request, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(general, make_named_request, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(
    general,
    retrieve_tls_certificate_with_sni,
    ALLOW_IN_TEST_MODE
);

// GitHub Functions
impl_new_function!(github, add_user_to_repo, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, remove_user_from_repo, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, add_user_to_team, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, remove_user_from_team, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, update_branch_protection_rule, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, create_environment_for_repo, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, configure_secret, DISALLOW_IN_TEST_MODE);
impl_new_function!(
    github,
    create_deployment_branch_protection_rule,
    DISALLOW_IN_TEST_MODE
);
impl_new_function!(github, trigger_repo_dispatch, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, check_org_membership_of_user, ALLOW_IN_TEST_MODE);
impl_new_function!(github, delete_deploy_key, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, require_signed_commits, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, add_repo_to_team, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, remove_repo_from_team, DISALLOW_IN_TEST_MODE);

impl_new_function_with_error_buffer!(github, make_graphql_query, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, make_advanced_graphql_query, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, fetch_commit, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, list_files, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(
    github,
    fetch_file_with_custom_media_type,
    ALLOW_IN_TEST_MODE
);
impl_new_function_with_error_buffer!(github, get_branch_protection_rules, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_branch_protection_ruleset, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_repository_collaborators, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, search_code, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, list_seats_in_org_copilot, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, add_users_to_org_copilot, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, remove_users_from_org_copilot, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_custom_properties_values, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, check_codeowners_file, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_repo_sbom, ALLOW_IN_TEST_MODE);
impl_new_function!(github, comment_on_pull_request, DISALLOW_IN_TEST_MODE);
impl_new_function!(
    github,
    pull_request_request_reviewers,
    DISALLOW_IN_TEST_MODE
);
impl_new_function_with_error_buffer!(github, get_weekly_commit_count, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_reference, ALLOW_IN_TEST_MODE);
impl_new_function!(github, create_reference, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_pull_requests, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, create_pull_request, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, create_file, DISALLOW_IN_TEST_MODE);
impl_new_function!(github, add_labels, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_user_id_from_username, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_username_from_user_id, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_repo_id_from_repo_name, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_repo_name_from_repo_id, ALLOW_IN_TEST_MODE);

// GitHub Functions only available with GitHub App authentication
impl_new_function!(github, review_fpat_requests_for_org, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, list_fpat_requests_for_org, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_repos_for_fpat, ALLOW_IN_TEST_MODE);

// AES functions
impl_new_function_with_error_buffer!(cryptography, aes_128_cbc_encrypt, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(cryptography, aes_128_cbc_decrypt, ALLOW_IN_TEST_MODE);

// Jira functions
impl_new_function_with_error_buffer!(jira, create_issue, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(jira, get_issue, ALLOW_IN_TEST_MODE);
impl_new_function!(jira, update_issue, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(jira, get_user, ALLOW_IN_TEST_MODE);
impl_new_function!(jira, post_comment, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(jira, search_issues, ALLOW_IN_TEST_MODE);

// AWS functions

// KMS
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, kms, generate_mac, DISALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function!(aws, kms, verify_mac, DISALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(
    aws,
    kms,
    sign_arbitrary_message,
    DISALLOW_IN_TEST_MODE
);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, kms, get_public_key, ALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, dynamodb, put_item, DISALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, dynamodb, delete_item, DISALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, dynamodb, query, ALLOW_IN_TEST_MODE);

// S3
#[cfg(feature = "aws")]
impl_new_sub_module_function!(aws, s3, delete_object, DISALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, s3, get_object_attributes, ALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, s3, get_object, ALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, s3, list_objects, ALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, s3, list_object_versions, ALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function!(aws, s3, put_object, DISALLOW_IN_TEST_MODE);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, s3, put_object_tags, DISALLOW_IN_TEST_MODE);

// GCP
#[cfg(feature = "gcp")]
impl_new_sub_module_function_with_error_buffer!(
    gcp,
    google_docs,
    upload_file,
    DISALLOW_IN_TEST_MODE
);
#[cfg(feature = "gcp")]
impl_new_sub_module_function_with_error_buffer!(gcp, google_docs, copy_file, DISALLOW_IN_TEST_MODE);
#[cfg(feature = "gcp")]
impl_new_sub_module_function_with_error_buffer!(
    gcp,
    google_docs,
    create_folder,
    DISALLOW_IN_TEST_MODE
);
#[cfg(feature = "gcp")]
impl_new_sub_module_function_with_error_buffer!(
    gcp,
    google_docs,
    create_doc_from_markdown,
    DISALLOW_IN_TEST_MODE
);
#[cfg(feature = "gcp")]
impl_new_sub_module_function_with_error_buffer!(
    gcp,
    google_docs,
    create_sheet_from_csv,
    DISALLOW_IN_TEST_MODE
);

// Npm Functions
impl_new_function_with_error_buffer!(npm, publish_empty_stub, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, set_team_permission_on_package, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(
    npm,
    create_granular_token_for_packages,
    DISALLOW_IN_TEST_MODE
);
impl_new_function_with_error_buffer!(npm, delete_granular_token, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, list_granular_tokens, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, delete_package, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, add_user_to_team, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, remove_user_from_team, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, remove_user_from_organization, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, invite_user_to_organization, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, get_org_user_list, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, get_org_users_without_2fa, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, list_packages_with_team_permission, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(npm, get_token_details, ALLOW_IN_TEST_MODE);

// Okta Functions
impl_new_function!(okta, remove_user_from_group, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(okta, get_user_data, ALLOW_IN_TEST_MODE);

// PagerDuty Functions
impl_new_function!(pagerduty, trigger_incident, DISALLOW_IN_TEST_MODE);

// Rustica Functions
impl_new_function_with_error_buffer!(rustica, new_mtls_cert, DISALLOW_IN_TEST_MODE);

// Slack Functions
impl_new_function!(slack, views_open, ALLOW_IN_TEST_MODE);
impl_new_function!(slack, post_message, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(slack, get_id_from_email, ALLOW_IN_TEST_MODE);
impl_new_function!(slack, post_to_arbitrary_webhook, ALLOW_IN_TEST_MODE);
impl_new_function!(slack, post_to_named_webhook, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(slack, get_presence, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(slack, get_dnd, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(slack, user_info, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(slack, create_channel, DISALLOW_IN_TEST_MODE);
impl_new_function!(slack, invite_to_channel, DISALLOW_IN_TEST_MODE);

// Splunk Functions
impl_new_function!(splunk, post_hec, ALLOW_IN_TEST_MODE);

// Yubikey Functions
impl_new_function_with_error_buffer!(yubikey, verify_otp, ALLOW_IN_TEST_MODE);

// Web Functions
impl_new_function_with_error_buffer!(web, issue_jwt, DISALLOW_IN_TEST_MODE);

// Blockchain functions
impl_new_sub_module_function_with_error_buffer!(
    blockchain,
    evm,
    get_transaction_by_hash,
    ALLOW_IN_TEST_MODE
);
impl_new_sub_module_function_with_error_buffer!(
    blockchain,
    evm,
    get_transaction_receipt,
    ALLOW_IN_TEST_MODE
);
impl_new_sub_module_function_with_error_buffer!(
    blockchain,
    evm,
    send_raw_transaction,
    DISALLOW_IN_TEST_MODE
);
impl_new_sub_module_function_with_error_buffer!(
    blockchain,
    evm,
    get_transaction_count,
    ALLOW_IN_TEST_MODE
);
impl_new_sub_module_function_with_error_buffer!(blockchain, evm, get_balance, ALLOW_IN_TEST_MODE);
impl_new_sub_module_function_with_error_buffer!(blockchain, evm, estimate_gas, ALLOW_IN_TEST_MODE);
impl_new_sub_module_function_with_error_buffer!(blockchain, evm, eth_call, ALLOW_IN_TEST_MODE);
impl_new_sub_module_function_with_error_buffer!(blockchain, evm, gas_price, ALLOW_IN_TEST_MODE);
impl_new_sub_module_function_with_error_buffer!(blockchain, evm, get_logs, ALLOW_IN_TEST_MODE);
impl_new_sub_module_function_with_error_buffer!(blockchain, evm, get_block, ALLOW_IN_TEST_MODE);

// Bloom filter functions
impl_new_function_with_error_buffer!(bloom_filter, build_with_items, ALLOW_IN_TEST_MODE);

/// Generates `to_api_function` and `is_known_api_function` from a single source-of-truth list.
///
/// `with_env` entries produce `Function::new_typed_with_env`
/// `without_env` entries produce `Function::new_typed` (for host functions that need no env, e.g. `get_time`).
macro_rules! define_api_functions {
    (
        with_env: [
            $($(#[$we_attr:meta])* $we_name:literal => $we_fn:expr),* $(,)?
        ],
        without_env: [
            $($(#[$woe_attr:meta])* $woe_name:literal => $woe_fn:expr),* $(,)?
        ] $(,)?
    ) => {
        pub fn to_api_function(
            name: &str,
            mut store: &mut Store,
            env: FunctionEnv<Env>,
        ) -> Option<Function> {
            Some(match name {
                $(
                    $(#[$we_attr])*
                    $we_name => Function::new_typed_with_env(&mut store, &env, $we_fn),
                )*
                $(
                    $(#[$woe_attr])*
                    $woe_name => Function::new_typed(&mut store, $woe_fn),
                )*
                _ => return None,
            })
        }

        /// Returns `true` if `name` is a host function known to Plaid.
        pub fn is_known_api_function(name: &str) -> bool {
            match name {
                $(
                    $(#[$we_attr])*
                    $we_name => true,
                )*
                $(
                    $(#[$woe_attr])*
                    $woe_name => true,
                )*
                _ => false,
            }
        }
    };
}

define_api_functions! {
    with_env: [
        // Message / request data
        "fetch_data"             => super::message::fetch_data,
        "fetch_source"           => super::message::fetch_source,
        "fetch_data_and_source"  => super::message::fetch_data_and_source,
        "get_accessory_data"     => super::runtime_data::get_accessory_data,
        "get_secrets"            => super::runtime_data::get_secrets,
        "get_headers"            => super::message::get_headers,
        "get_query_params"       => super::message::get_query_params,
        "fetch_random_bytes"     => super::internal::fetch_random_bytes,

        // The below are types that deal with Plaid specific internals like
        // the data base or caching systems. These usually have specific implementations
        // so are broken out into their own module.
        "get_response"           => super::response::get_response,
        "set_response"           => super::response::set_response,
        "set_error_context"      => super::internal::set_error_context,
        "print_debug_string"     => super::internal::print_debug_string,
        "storage_insert"         => super::storage::insert,
        "storage_insert_shared"  => super::storage::insert_shared,
        "storage_get"            => super::storage::get,
        "storage_get_shared"     => super::storage::get_shared,
        "storage_delete"         => super::storage::delete,
        "storage_delete_shared"  => super::storage::delete_shared,
        "storage_list_keys"      => super::storage::list_keys,
        "storage_list_keys_shared" => super::storage::list_keys_shared,
        "cache_insert"           => super::cache::insert,
        "cache_get"              => super::cache::get,
        "log_back"               => super::internal::log_back,
        "log_back_unlimited"     => super::internal::log_back_unlimited,

        // Npm Calls
        "npm_publish_empty_stub"                  => npm_publish_empty_stub,
        "npm_set_team_permission_on_package"      => npm_set_team_permission_on_package,
        "npm_create_granular_token_for_packages"  => npm_create_granular_token_for_packages,
        "npm_delete_granular_token"               => npm_delete_granular_token,
        "npm_list_granular_tokens"                => npm_list_granular_tokens,
        "npm_delete_package"                      => npm_delete_package,
        "npm_add_user_to_team"                    => npm_add_user_to_team,
        "npm_remove_user_from_team"               => npm_remove_user_from_team,
        "npm_remove_user_from_organization"       => npm_remove_user_from_organization,
        "npm_invite_user_to_organization"         => npm_invite_user_to_organization,
        "npm_get_org_user_list"                   => npm_get_org_user_list,
        "npm_get_org_users_without_2fa"           => npm_get_org_users_without_2fa,
        "npm_list_packages_with_team_permission"  => npm_list_packages_with_team_permission,
        "npm_get_token_details"                   => npm_get_token_details,

        // Okta
        "okta_remove_user_from_group" => okta_remove_user_from_group,
        "okta_get_user_data"          => okta_get_user_data,

        // AES calls
        "cryptography_aes_128_cbc_encrypt" => cryptography_aes_128_cbc_encrypt,
        "cryptography_aes_128_cbc_decrypt" => cryptography_aes_128_cbc_decrypt,

        // GitHub Calls
        "github_add_user_to_repo"                          => github_add_user_to_repo,
        "github_remove_user_from_repo"                     => github_remove_user_from_repo,
        "github_add_user_to_team"                          => github_add_user_to_team,
        "github_remove_user_from_team"                     => github_remove_user_from_team,
        "github_make_graphql_query"                        => github_make_graphql_query,
        "github_make_advanced_graphql_query"               => github_make_advanced_graphql_query,
        "github_fetch_commit"                              => github_fetch_commit,
        "github_list_files"                                => github_list_files,
        "github_fetch_file_with_custom_media_type"         => github_fetch_file_with_custom_media_type,
        "github_list_fpat_requests_for_org"                => github_list_fpat_requests_for_org,
        "github_review_fpat_requests_for_org"              => github_review_fpat_requests_for_org,
        "github_get_repos_for_fpat"                        => github_get_repos_for_fpat,
        "github_get_branch_protection_rules"               => github_get_branch_protection_rules,
        "github_get_branch_protection_ruleset"             => github_get_branch_protection_ruleset,
        "github_get_repository_collaborators"              => github_get_repository_collaborators,
        "github_get_custom_properties_values"              => github_get_custom_properties_values,
        "github_check_codeowners_file"                     => github_check_codeowners_file,
        "github_update_branch_protection_rule"             => github_update_branch_protection_rule,
        "github_create_environment_for_repo"               => github_create_environment_for_repo,
        "github_configure_secret"                          => github_configure_secret,
        "github_create_deployment_branch_protection_rule"  => github_create_deployment_branch_protection_rule,
        "github_search_code"                               => github_search_code,
        "github_add_users_to_org_copilot"                  => github_add_users_to_org_copilot,
        "github_remove_users_from_org_copilot"             => github_remove_users_from_org_copilot,
        "github_list_seats_in_org_copilot"                 => github_list_seats_in_org_copilot,
        "github_trigger_repo_dispatch"                     => github_trigger_repo_dispatch,
        "github_check_org_membership_of_user"              => github_check_org_membership_of_user,
        "github_comment_on_pull_request"                   => github_comment_on_pull_request,
        "github_delete_deploy_key"                         => github_delete_deploy_key,
        "github_pull_request_request_reviewers"            => github_pull_request_request_reviewers,
        "github_require_signed_commits"                    => github_require_signed_commits,
        "github_get_weekly_commit_count"                   => github_get_weekly_commit_count,
        "github_add_repo_to_team"                          => github_add_repo_to_team,
        "github_remove_repo_from_team"                     => github_remove_repo_from_team,
        "github_get_reference"                             => github_get_reference,
        "github_create_reference"                          => github_create_reference,
        "github_get_pull_requests"                         => github_get_pull_requests,
        "github_create_pull_request"                       => github_create_pull_request,
        "github_create_file"                               => github_create_file,
        "github_get_repo_sbom"                             => github_get_repo_sbom,
        "github_add_labels"                                => github_add_labels,
        "github_get_user_id_from_username"                 => github_get_user_id_from_username,
        "github_get_username_from_user_id"                 => github_get_username_from_user_id,
        "github_get_repo_id_from_repo_name"                => github_get_repo_id_from_repo_name,
        "github_get_repo_name_from_repo_id"                => github_get_repo_name_from_repo_id,

        // Slack Calls
        "slack_post_to_named_webhook"    => slack_post_to_named_webhook,
        "slack_post_to_arbitrary_webhook" => slack_post_to_arbitrary_webhook,
        "slack_post_message"             => slack_post_message,
        "slack_views_open"               => slack_views_open,
        "slack_get_id_from_email"        => slack_get_id_from_email,
        "slack_get_presence"             => slack_get_presence,
        "slack_get_dnd"                  => slack_get_dnd,
        "slack_user_info"                => slack_user_info,
        "slack_create_channel"           => slack_create_channel,
        "slack_invite_to_channel"        => slack_invite_to_channel,

        // General Calls
        "general_simple_json_post_request"          => general_simple_json_post_request,
        "general_make_named_request"                => general_make_named_request,
        "general_retrieve_tls_certificate_with_sni" => general_retrieve_tls_certificate_with_sni,

        // Jira Calls
        "jira_create_issue"  => jira_create_issue,
        "jira_get_issue"     => jira_get_issue,
        "jira_update_issue"  => jira_update_issue,
        "jira_get_user"      => jira_get_user,
        "jira_post_comment"  => jira_post_comment,
        "jira_search_issues" => jira_search_issues,

        // KMS calls
        #[cfg(feature = "aws")] "aws_kms_generate_mac"        => aws_kms_generate_mac,
        #[cfg(feature = "aws")] "aws_kms_verify_mac"          => aws_kms_verify_mac,
        #[cfg(feature = "aws")] "aws_kms_sign_arbitrary_message" => aws_kms_sign_arbitrary_message,
        #[cfg(feature = "aws")] "aws_kms_get_public_key"      => aws_kms_get_public_key,

        // DynamoDB calls
        #[cfg(feature = "aws")] "aws_dynamodb_put_item"    => aws_dynamodb_put_item,
        #[cfg(feature = "aws")] "aws_dynamodb_delete_item" => aws_dynamodb_delete_item,
        #[cfg(feature = "aws")] "aws_dynamodb_query"       => aws_dynamodb_query,

        // S3 calls
        #[cfg(feature = "aws")] "aws_s3_delete_object"         => aws_s3_delete_object,
        #[cfg(feature = "aws")] "aws_s3_get_object"            => aws_s3_get_object,
        #[cfg(feature = "aws")] "aws_s3_get_object_attributes" => aws_s3_get_object_attributes,
        #[cfg(feature = "aws")] "aws_s3_list_objects"          => aws_s3_list_objects,
        #[cfg(feature = "aws")] "aws_s3_list_object_versions"  => aws_s3_list_object_versions,
        #[cfg(feature = "aws")] "aws_s3_put_object"            => aws_s3_put_object,
        #[cfg(feature = "aws")] "aws_s3_put_object_tags"       => aws_s3_put_object_tags,

        // GCP
        #[cfg(feature = "gcp")] "gcp_google_docs_upload_file"            => gcp_google_docs_upload_file,
        #[cfg(feature = "gcp")] "gcp_google_docs_copy_file"              => gcp_google_docs_copy_file,
        #[cfg(feature = "gcp")] "gcp_google_docs_create_folder"          => gcp_google_docs_create_folder,
        #[cfg(feature = "gcp")] "gcp_google_docs_create_doc_from_markdown" => gcp_google_docs_create_doc_from_markdown,
        #[cfg(feature = "gcp")] "gcp_google_docs_create_sheet_from_csv"  => gcp_google_docs_create_sheet_from_csv,

        // PagerDuty Calls
        "pagerduty_trigger_incident" => pagerduty_trigger_incident,

        // Rustica Calls
        "rustica_new_mtls_cert" => rustica_new_mtls_cert,

        // Yubikey Calls
        "yubikey_verify_otp" => yubikey_verify_otp,

        // Splunk Calls
        "splunk_post_hec" => splunk_post_hec,

        // Web Calls
        "web_issue_jwt" => web_issue_jwt,

        // Blockchain calls
        "blockchain_evm_get_transaction_by_hash" => blockchain_evm_get_transaction_by_hash,
        "blockchain_evm_get_transaction_receipt"  => blockchain_evm_get_transaction_receipt,
        "blockchain_evm_send_raw_transaction"     => blockchain_evm_send_raw_transaction,
        "blockchain_evm_get_transaction_count"    => blockchain_evm_get_transaction_count,
        "blockchain_evm_get_balance"              => blockchain_evm_get_balance,
        "blockchain_evm_estimate_gas"             => blockchain_evm_estimate_gas,
        "blockchain_evm_eth_call"                 => blockchain_evm_eth_call,
        "blockchain_evm_gas_price"                => blockchain_evm_gas_price,
        "blockchain_evm_get_logs"                 => blockchain_evm_get_logs,
        "blockchain_evm_get_block"                => blockchain_evm_get_block,

        // Bloomfilter calls
        "bloom_filter_build_with_items" => bloom_filter_build_with_items,
    ],
    without_env: [
        "get_time" => super::internal::get_time,
    ],
}
