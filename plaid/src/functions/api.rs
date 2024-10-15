use super::{safely_write_data_back, FunctionErrors};
use crate::executor::Env;
use crate::functions::{get_memory, safely_get_string};
use wasmer::{AsStoreRef, Function, FunctionEnv, FunctionEnvMut, Store, WasmPtr};

macro_rules! impl_new_function {
    ($api:ident, $function_name:ident) => {
        paste::item! {
            fn [< $api _ $function_name _impl>] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32) -> Result<i32, FunctionErrors> {
                let store = env.as_store_ref();
                let env_data = env.data();

                if let Err(e) = env_data.external_logging_system.log_function_call(env_data.name.clone(), stringify!([< $api _ $function_name >]).to_string()) {
                    error!("Logging system is not working!!: {:?}", e);
                    return Err(FunctionErrors::InternalApiError);
                }

                let memory_view = match get_memory(&env, &store) {
                    Ok(memory_view) => memory_view,
                    Err(e) => {
                        error!("{}: Memory error in {}: {:?}", env.data().name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    },
                };

                let params = safely_get_string(&memory_view, params_buffer, params_buffer_len)?;

                // Check that the request API system is even configured.
                // This is something like Okta, Slack, or GitHub
                let api = env_data.api.$api.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;

                // Clone the APIs Arc to use in Tokio closure
                let env_api = env_data.api.clone();

                // Run the function on the Tokio runtime and wait for the result
                let result = env_api.runtime.block_on(async move {
                    api.$function_name(&params, &env_data.name).await
                });

                let return_data = match result {
                    Ok(return_data) => return_data,
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", env_data.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                trace!("{} is calling {} got a return data of {}", env_data.name, stringify!([< $api _ $function_name >]), return_data);
                return Ok(return_data as i32);
            }

            fn [< $api _ $function_name >] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32) -> i32 {
                let name = env.data().name.clone();
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

macro_rules! impl_new_function_with_error_buffer {
    ($api:ident, $function_name:ident) => {
        paste::item! {
            fn [< $api _ $function_name _impl>] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> Result<i32, FunctionErrors> {
                let store = env.as_store_ref();
                let env_data = env.data();

                if let Err(e) = env_data.external_logging_system.log_function_call(env_data.name.clone(), stringify!([< $api _ $function_name >]).to_string()) {
                    error!("Logging system is not working!!: {:?}", e);
                    return Err(FunctionErrors::InternalApiError);
                }

                let memory_view = match get_memory(&env, &store) {
                    Ok(memory_view) => memory_view,
                    Err(e) => {
                        error!("{}: Memory error in {}: {:?}", env.data().name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    },
                };

                let params = safely_get_string(&memory_view, params_buffer, params_buffer_len)?;

                // Check the requested API system is configured.
                let api = env_data.api.$api.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;

                // Clone the APIs Arc to use in Tokio closure
                let env_api = env_data.api.clone();
                let name = &env_data.name.clone();
                // Run the function on the Tokio runtime and wait for the result
                let result = env_api.runtime.block_on(async move {
                    api.$function_name(&params, name).await
                });

                let return_data = match result {
                    Ok(return_data) => return_data,
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", env_data.name, stringify!([< $api _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                if return_data.len() > ret_buffer_len as usize {
                    error!("{} could not receive data from {} because it provided a return buffer that was too small. Got {}, needed {}", env_data.name, stringify!([< $api _ $function_name >]), ret_buffer_len, return_data.len());
                    trace!("Data: {}", return_data);
                    return Err(FunctionErrors::ReturnBufferTooSmall);
                }

                safely_write_data_back(&memory_view, return_data.as_bytes(), ret_buffer, ret_buffer_len)?;

                trace!("{} is calling {} got a return data length of {}", env_data.name, stringify!([< $api _ $function_name >]), return_data.len());
                return Ok(return_data.len() as i32);
            }

            fn [< $api _ $function_name >] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> i32 {
                let name = env.data().name.clone();
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
macro_rules! impl_new_sub_module_function_with_error_buffer {
    ($api:ident, $sub_module:ident, $function_name:ident) => {
        paste::item! {
            fn [< $api _ $sub_module _ $function_name _impl>] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> Result<i32, FunctionErrors> {
                let store = env.as_store_ref();
                let env_data = env.data();

                // Log function call by module
                if let Err(e) = env_data.external_logging_system.log_function_call(env_data.name.clone(), stringify!([< $api _ $sub_module _ $function_name >]).to_string()) {
                    error!("Logging system is not working!!: {:?}", e);
                    return Err(FunctionErrors::InternalApiError);
                }

                let memory_view = match get_memory(&env, &store) {
                    Ok(memory_view) => memory_view,
                    Err(e) => {
                        error!("{}: Memory error in {}: {:?}", env.data().name, stringify!([< $api _ $sub_module _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    },
                };

                let params = safely_get_string(&memory_view, params_buffer, params_buffer_len)?;

                // Check that AWS API is configured
                let aws = env_data.api.$api.as_ref().ok_or(FunctionErrors::ApiNotConfigured)?;
                let sub_module = &aws.$sub_module;

                // Clone the APIs Arc to use in Tokio closure
                let env_api = env_data.api.clone();
                let name = &env_data.name.clone();
                // Run the function on the Tokio runtime and wait for the result
                let result = env_api.runtime.block_on(async move {
                    sub_module.$function_name(&params, name).await
                });

                let return_data = match result {
                    Ok(return_data) => return_data,
                    Err(e) => {
                        error!("{} experienced an issue calling {}: {:?}", env_data.name, stringify!([< $api _ $sub_module _ $function_name >]), e);
                        return Err(FunctionErrors::InternalApiError);
                    }
                };

                if return_data.len() > ret_buffer_len as usize {
                    error!("{} could not receive data from {} because it provided a return buffer that was too small. Got {}, needed {}", env_data.name,  stringify!([< $api _ $sub_module _ $function_name >]), ret_buffer_len, return_data.len());
                    trace!("Data: {}", return_data);
                    return Err(FunctionErrors::ReturnBufferTooSmall);
                }

                safely_write_data_back(&memory_view, return_data.as_bytes(), ret_buffer, ret_buffer_len)?;

                trace!("{} is calling {} got a return data length of {}", env_data.name,  stringify!([< $api _ $sub_module _ $function_name >]), return_data.len());
                return Ok(return_data.len() as i32);
            }

            fn [< $api _ $sub_module _ $function_name >] (env: FunctionEnvMut<Env>, params_buffer: WasmPtr<u8>, params_buffer_len: u32, ret_buffer: WasmPtr<u8>, ret_buffer_len: u32) -> i32 {
                let name = env.data().name.clone();
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
impl_new_function!(general, simple_json_post_request);
impl_new_function_with_error_buffer!(general, make_named_request);

// GitHub Functions
impl_new_function!(github, add_user_to_repo);
impl_new_function!(github, remove_user_from_repo);
impl_new_function!(github, add_user_to_team);
impl_new_function!(github, remove_user_from_team);
impl_new_function!(github, update_branch_protection_rule);
impl_new_function!(github, create_environment_for_repo);
impl_new_function!(github, configure_secret);
impl_new_function!(github, create_deployment_branch_protection_rule);
impl_new_function!(github, add_users_to_org_copilot);
impl_new_function!(github, remove_users_from_org_copilot);

impl_new_function_with_error_buffer!(github, make_graphql_query);
impl_new_function_with_error_buffer!(github, make_advanced_graphql_query);
impl_new_function_with_error_buffer!(github, fetch_commit);
impl_new_function_with_error_buffer!(github, list_files);
impl_new_function_with_error_buffer!(github, fetch_file);
impl_new_function_with_error_buffer!(github, get_branch_protection_rules);
impl_new_function_with_error_buffer!(github, get_repository_collaborators);
impl_new_function_with_error_buffer!(github, search_for_file);
impl_new_function_with_error_buffer!(github, list_seats_in_org_copilot);

// GitHub Functions only available with GitHub App authentication
impl_new_function!(github, review_fpat_requests_for_org);
impl_new_function_with_error_buffer!(github, list_fpat_requests_for_org);
impl_new_function_with_error_buffer!(github, get_repos_for_fpat);

// AWS functions
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, kms, sign_arbitrary_message);
#[cfg(feature = "aws")]
impl_new_sub_module_function_with_error_buffer!(aws, kms, get_public_key);

// Npm Functions
impl_new_function!(npm, publish_empty_stub);
impl_new_function!(npm, set_team_permission_on_package);
impl_new_function_with_error_buffer!(npm, create_granular_token_for_packages);
impl_new_function!(npm, delete_granular_token);
impl_new_function_with_error_buffer!(npm, list_granular_tokens);
impl_new_function!(npm, delete_package);
impl_new_function!(npm, add_user_to_team);
impl_new_function!(npm, remove_user_from_team);
impl_new_function!(npm, remove_user_from_organization);
impl_new_function!(npm, invite_user_to_organization);
impl_new_function_with_error_buffer!(npm, get_org_user_list);
impl_new_function_with_error_buffer!(npm, get_org_users_without_2fa);
impl_new_function_with_error_buffer!(npm, list_packages_with_team_permission);
impl_new_function_with_error_buffer!(npm, get_token_details);

// Okta Functions
impl_new_function!(okta, remove_user_from_group);
impl_new_function_with_error_buffer!(okta, get_user_data);

// PagerDuty Functions
impl_new_function!(pagerduty, trigger_incident);

// Quorum Functions
impl_new_function_with_error_buffer!(quorum, proposal_status);

// Rustica Functions
impl_new_function_with_error_buffer!(rustica, new_mtls_cert);

// Slack Functions
impl_new_function!(slack, views_open);
impl_new_function!(slack, post_message);
impl_new_function!(slack, post_to_arbitrary_webhook);
impl_new_function!(slack, post_to_named_webhook);

// Splunk Functions
impl_new_function!(splunk, post_hec);

// Yubikey Functions
impl_new_function_with_error_buffer!(yubikey, verify_otp);

// Web Functions
impl_new_function_with_error_buffer!(web, issue_jwt);

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
        "fetch_accessory_data_by_name" => Function::new_typed_with_env(
            &mut store,
            &env,
            super::message::fetch_accessory_data_by_name,
        ),
        "fetch_random_bytes" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::fetch_random_bytes)
        }

        // The below are types that deal with Plaid specific internals like
        // the data base or caching systems. These usually have specific implementations
        // so are broken out into their own module.
        "get_response" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::get_response)
        }
        "set_response" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::set_response)
        }
        "print_debug_string" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::print_debug_string)
        }
        "get_time" => Function::new_typed(&mut store, super::internal::get_time),
        "storage_insert" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::storage_insert)
        }
        "storage_get" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::storage_get)
        }

        "cache_insert" => {
            Function::new_typed_with_env(&mut store, &env, super::internal::cache_insert)
        }
        "cache_get" => Function::new_typed_with_env(&mut store, &env, super::internal::cache_get),
        "log_back" => Function::new_typed_with_env(&mut store, &env, super::internal::log_back),
        
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

        "npm_delete_package" => {
            Function::new_typed_with_env(&mut store, &env, npm_delete_package)
        }

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
        "github_list_files" => {
            Function::new_typed_with_env(&mut store, &env, github_list_files)
        }
        "github_fetch_file" => {
            Function::new_typed_with_env(&mut store, &env, github_fetch_file)
        }
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
        "github_get_repository_collaborators" => {
            Function::new_typed_with_env(&mut store, &env, github_get_repository_collaborators)
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
        "github_create_deployment_branch_protection_rule" => {
            Function::new_typed_with_env(&mut store, &env, github_create_deployment_branch_protection_rule)
        }
        "github_search_for_file" => {
            Function::new_typed_with_env(&mut store, &env, github_search_for_file)
        }
        "github_add_users_to_org_copilot" => {
            Function::new_typed_with_env(&mut store, &env, github_add_users_to_org_copilot)
        }
        "github_remove_users_from_org_copilot" => {
            Function::new_typed_with_env(&mut store, &env, github_remove_users_from_org_copilot)
        }
        "github_list_seats_in_org_copilot" => {
            Function::new_typed_with_env(&mut store, &env, github_list_seats_in_org_copilot)
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

        // Quorum Calls
        "quorum_proposal_status" => {
            Function::new_typed_with_env(&mut store, &env, quorum_proposal_status)
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
