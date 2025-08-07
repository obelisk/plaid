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
                let sub_module = &aws.$sub_module;

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

// General Functions
impl_new_function!(general, simple_json_post_request, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(general, make_named_request, ALLOW_IN_TEST_MODE);

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
impl_new_function_with_error_buffer!(github, fetch_file, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_branch_protection_rules, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_branch_protection_ruleset, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_repository_collaborators, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, search_code, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, list_seats_in_org_copilot, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, add_users_to_org_copilot, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, remove_users_from_org_copilot, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_custom_properties_values, ALLOW_IN_TEST_MODE);
impl_new_function!(github, comment_on_pull_request, DISALLOW_IN_TEST_MODE);
impl_new_function!(
    github,
    pull_request_request_reviewers,
    DISALLOW_IN_TEST_MODE
);
impl_new_function_with_error_buffer!(github, get_weekly_commit_count, ALLOW_IN_TEST_MODE);

// GitHub Functions only available with GitHub App authentication
impl_new_function!(github, review_fpat_requests_for_org, DISALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, list_fpat_requests_for_org, ALLOW_IN_TEST_MODE);
impl_new_function_with_error_buffer!(github, get_repos_for_fpat, ALLOW_IN_TEST_MODE);

// AWS functions
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(
    aws,
    kms,
    sign_arbitrary_message,
    DISALLOW_IN_TEST_MODE
);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, kms, get_public_key, ALLOW_IN_TEST_MODE);

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

// Splunk Functions
impl_new_function!(splunk, post_hec, ALLOW_IN_TEST_MODE);

// Yubikey Functions
impl_new_function_with_error_buffer!(yubikey, verify_otp, ALLOW_IN_TEST_MODE);

// Web Functions
impl_new_function_with_error_buffer!(web, issue_jwt, DISALLOW_IN_TEST_MODE);

pub fn to_api_function(
    name: &str,
    mut store: &mut Store,
    env: FunctionEnv<Env>,
) -> Option<Function> {
    Some(match name {
        // The below are types that deal with getting data into the guest that comes
        // from the message itself.
        "fetch_data" => Function::new_typed_with_env(&mut store, &env, super::message::fetch_data),
        "fetch_source" => {
            Function::new_typed_with_env(&mut store, &env, super::message::fetch_source)
        }
        "fetch_data_and_source" => {
            Function::new_typed_with_env(&mut store, &env, super::message::fetch_data_and_source)
        }
        "get_accessory_data" => {
            Function::new_typed_with_env(&mut store, &env, super::runtime_data::get_accessory_data)
        }
        "get_secrets" => {
            Function::new_typed_with_env(&mut store, &env, super::runtime_data::get_secrets)
        }
        "get_headers" => {
            Function::new_typed_with_env(&mut store, &env, super::message::get_headers)
        }
        "get_query_params" => {
            Function::new_typed_with_env(&mut store, &env, super::message::get_query_params)
        }
        "fetch_random_bytes" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::fetch_random_bytes)
        }

        // The below are types that deal with Plaid specific internals like
        // the data base or caching systems. These usually have specific implementations
        // so are broken out into their own module.
        "get_response" => {
            Function::new_typed_with_env(&mut store, &env, super::response::get_response)
        }
        "set_response" => {
            Function::new_typed_with_env(&mut store, &env, super::response::set_response)
        }
        "set_error_context" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::set_error_context)
        }
        "print_debug_string" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::print_debug_string)
        }
        "get_time" => Function::new_typed(&mut store, super::internal::get_time),
        "storage_insert" => Function::new_typed_with_env(&mut store, &env, super::storage::insert),
        "storage_insert_shared" => {
            Function::new_typed_with_env(&mut store, &env, super::storage::insert_shared)
        }
        "storage_get" => Function::new_typed_with_env(&mut store, &env, super::storage::get),
        "storage_get_shared" => {
            Function::new_typed_with_env(&mut store, &env, super::storage::get_shared)
        }
        "storage_delete" => Function::new_typed_with_env(&mut store, &env, super::storage::delete),
        "storage_delete_shared" => {
            Function::new_typed_with_env(&mut store, &env, super::storage::delete_shared)
        }
        "storage_list_keys" => {
            Function::new_typed_with_env(&mut store, &env, super::storage::list_keys)
        }
        "storage_list_keys_shared" => {
            Function::new_typed_with_env(&mut store, &env, super::storage::list_keys_shared)
        }
        "cache_insert" => Function::new_typed_with_env(&mut store, &env, super::cache::insert),
        "cache_get" => Function::new_typed_with_env(&mut store, &env, super::cache::get),
        "log_back" => Function::new_typed_with_env(&mut store, &env, super::internal::log_back),
        "log_back_unlimited" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::log_back_unlimited)
        }
        // Npm Calls
        "npm_publish_empty_stub" => {
            Function::new_typed_with_env(&mut store, &env, npm_publish_empty_stub)
        }

        "npm_set_team_permission_on_package" => {
            Function::new_typed_with_env(&mut store, &env, npm_set_team_permission_on_package)
        }

        "npm_create_granular_token_for_packages" => {
            Function::new_typed_with_env(&mut store, &env, npm_create_granular_token_for_packages)
        }

        "npm_delete_granular_token" => {
            Function::new_typed_with_env(&mut store, &env, npm_delete_granular_token)
        }

        "npm_list_granular_tokens" => {
            Function::new_typed_with_env(&mut store, &env, npm_list_granular_tokens)
        }

        "npm_delete_package" => Function::new_typed_with_env(&mut store, &env, npm_delete_package),

        "npm_add_user_to_team" => {
            Function::new_typed_with_env(&mut store, &env, npm_add_user_to_team)
        }

        "npm_remove_user_from_team" => {
            Function::new_typed_with_env(&mut store, &env, npm_remove_user_from_team)
        }

        "npm_remove_user_from_organization" => {
            Function::new_typed_with_env(&mut store, &env, npm_remove_user_from_organization)
        }

        "npm_invite_user_to_organization" => {
            Function::new_typed_with_env(&mut store, &env, npm_invite_user_to_organization)
        }

        "npm_get_org_user_list" => {
            Function::new_typed_with_env(&mut store, &env, npm_get_org_user_list)
        }

        "npm_get_org_users_without_2fa" => {
            Function::new_typed_with_env(&mut store, &env, npm_get_org_users_without_2fa)
        }

        "npm_list_packages_with_team_permission" => {
            Function::new_typed_with_env(&mut store, &env, npm_list_packages_with_team_permission)
        }

        "npm_get_token_details" => {
            Function::new_typed_with_env(&mut store, &env, npm_get_token_details)
        }

        // Okta Calls
        "okta_remove_user_from_group" => {
            Function::new_typed_with_env(&mut store, &env, okta_remove_user_from_group)
        }
        "okta_get_user_data" => Function::new_typed_with_env(&mut store, &env, okta_get_user_data),

        // GitHub Calls
        "github_remove_user_from_repo" => {
            Function::new_typed_with_env(&mut store, &env, github_remove_user_from_repo)
        }
        "github_add_user_to_repo" => {
            Function::new_typed_with_env(&mut store, &env, github_add_user_to_repo)
        }
        "github_add_user_to_team" => {
            Function::new_typed_with_env(&mut store, &env, github_add_user_to_team)
        }
        "github_remove_user_from_team" => {
            Function::new_typed_with_env(&mut store, &env, github_remove_user_from_team)
        }
        "github_make_graphql_query" => {
            Function::new_typed_with_env(&mut store, &env, github_make_graphql_query)
        }
        "github_make_advanced_graphql_query" => {
            Function::new_typed_with_env(&mut store, &env, github_make_advanced_graphql_query)
        }
        "github_fetch_commit" => {
            Function::new_typed_with_env(&mut store, &env, github_fetch_commit)
        }
        "github_list_files" => Function::new_typed_with_env(&mut store, &env, github_list_files),
        "github_fetch_file" => Function::new_typed_with_env(&mut store, &env, github_fetch_file),
        "github_list_fpat_requests_for_org" => {
            Function::new_typed_with_env(&mut store, &env, github_list_fpat_requests_for_org)
        }
        "github_review_fpat_requests_for_org" => {
            Function::new_typed_with_env(&mut store, &env, github_review_fpat_requests_for_org)
        }
        "github_get_repos_for_fpat" => {
            Function::new_typed_with_env(&mut store, &env, github_get_repos_for_fpat)
        }
        "github_get_branch_protection_rules" => {
            Function::new_typed_with_env(&mut store, &env, github_get_branch_protection_rules)
        }
        "github_get_branch_protection_ruleset" => {
            Function::new_typed_with_env(&mut store, &env, github_get_branch_protection_ruleset)
        }
        "github_get_repository_collaborators" => {
            Function::new_typed_with_env(&mut store, &env, github_get_repository_collaborators)
        }
        "github_get_custom_properties_values" => {
            Function::new_typed_with_env(&mut store, &env, github_get_custom_properties_values)
        }
        "github_update_branch_protection_rule" => {
            Function::new_typed_with_env(&mut store, &env, github_update_branch_protection_rule)
        }
        "github_create_environment_for_repo" => {
            Function::new_typed_with_env(&mut store, &env, github_create_environment_for_repo)
        }
        "github_configure_secret" => {
            Function::new_typed_with_env(&mut store, &env, github_configure_secret)
        }
        "github_create_deployment_branch_protection_rule" => Function::new_typed_with_env(
            &mut store,
            &env,
            github_create_deployment_branch_protection_rule,
        ),
        "github_search_code" => Function::new_typed_with_env(&mut store, &env, github_search_code),
        "github_add_users_to_org_copilot" => {
            Function::new_typed_with_env(&mut store, &env, github_add_users_to_org_copilot)
        }
        "github_remove_users_from_org_copilot" => {
            Function::new_typed_with_env(&mut store, &env, github_remove_users_from_org_copilot)
        }
        "github_list_seats_in_org_copilot" => {
            Function::new_typed_with_env(&mut store, &env, github_list_seats_in_org_copilot)
        }
        "github_trigger_repo_dispatch" => {
            Function::new_typed_with_env(&mut store, &env, github_trigger_repo_dispatch)
        }
        "github_check_org_membership_of_user" => {
            Function::new_typed_with_env(&mut store, &env, github_check_org_membership_of_user)
        }
        "github_comment_on_pull_request" => {
            Function::new_typed_with_env(&mut store, &env, github_comment_on_pull_request)
        }
        "github_delete_deploy_key" => {
            Function::new_typed_with_env(&mut store, &env, github_delete_deploy_key)
        }
        "github_pull_request_request_reviewers" => {
            Function::new_typed_with_env(&mut store, &env, github_pull_request_request_reviewers)
        }
        "github_require_signed_commits" => {
            Function::new_typed_with_env(&mut store, &env, github_require_signed_commits)
        }
        "github_get_weekly_commit_count" => {
            Function::new_typed_with_env(&mut store, &env, github_get_weekly_commit_count)
        }
        "github_add_repo_to_team" => {
            Function::new_typed_with_env(&mut store, &env, github_add_repo_to_team)
        }
        "github_remove_repo_from_team" => {
            Function::new_typed_with_env(&mut store, &env, github_remove_repo_from_team)
        }

        // Slack Calls
        "slack_post_to_named_webhook" => {
            Function::new_typed_with_env(&mut store, &env, slack_post_to_named_webhook)
        }
        "slack_post_to_arbitrary_webhook" => {
            Function::new_typed_with_env(&mut store, &env, slack_post_to_arbitrary_webhook)
        }
        "slack_post_message" => Function::new_typed_with_env(&mut store, &env, slack_post_message),
        "slack_views_open" => Function::new_typed_with_env(&mut store, &env, slack_views_open),
        "slack_get_id_from_email" => {
            Function::new_typed_with_env(&mut store, &env, slack_get_id_from_email)
        }
        "slack_get_presence" => Function::new_typed_with_env(&mut store, &env, slack_get_presence),
        "slack_get_dnd" => Function::new_typed_with_env(&mut store, &env, slack_get_dnd),
        "slack_user_info" => Function::new_typed_with_env(&mut store, &env, slack_user_info),

        // General Calls
        "general_simple_json_post_request" => {
            Function::new_typed_with_env(&mut store, &env, general_simple_json_post_request)
        }
        "general_make_named_request" => {
            Function::new_typed_with_env(&mut store, &env, general_make_named_request)
        }

        // KMS calls
        #[cfg(feature = "aws")]
        "aws_kms_sign_arbitrary_message" => {
            Function::new_typed_with_env(&mut store, &env, aws_kms_sign_arbitrary_message)
        }

        #[cfg(feature = "aws")]
        "aws_kms_get_public_key" => {
            Function::new_typed_with_env(&mut store, &env, aws_kms_get_public_key)
        }

        // PagerDuty Calls
        "pagerduty_trigger_incident" => {
            Function::new_typed_with_env(&mut store, &env, pagerduty_trigger_incident)
        }

        // Rustica Calls
        "rustica_new_mtls_cert" => {
            Function::new_typed_with_env(&mut store, &env, rustica_new_mtls_cert)
        }

        // Yubikey Calls
        "yubikey_verify_otp" => Function::new_typed_with_env(&mut store, &env, yubikey_verify_otp),

        // Splunk Calls
        "splunk_post_hec" => Function::new_typed_with_env(&mut store, &env, splunk_post_hec),

        // Web Calls
        "web_issue_jwt" => Function::new_typed_with_env(&mut store, &env, web_issue_jwt),

        // No match
        _ => return None,
    })
}
