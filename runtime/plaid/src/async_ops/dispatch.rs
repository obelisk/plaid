//! Dispatch table for async API calls.
//!
//! Every host-facing API function has the uniform shape
//! `async fn (&self, params: &str, module: Arc<PlaidModule>) -> Result<T, ApiError>`
//! where `T` is `String`, `u32`, `i32`, or `bool`. This module maps the host
//! function names (the same names the STL uses, e.g.
//! `github_add_user_to_repo`) to futures that run the call and normalize the
//! result into a JSON-encoded [`TicketResult`].
//!
//! Test-mode gating mirrors the synchronous host functions: functions that
//! are disallowed in test mode reject the spawn at spawn time, so a rule
//! never waits on an operation that is guaranteed to fail.
//!
//! The table is intentionally explicit rather than macro-generated: each arm
//! borrows the subsystem and awaits the call, and the return value is
//! normalized through one small macro. This keeps the table auditable
//! against `functions/api.rs`'s registration list.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::apis::Api;
use crate::loader::PlaidModule;

use super::TicketResult;

/// Normalize a successful API return value into the JSON payload stored in
/// the ticket. Numbers and bools are JSON-encoded so the guest can decode
/// them with the matching `as_*` helper on `AsyncResult`.
macro_rules! json_result {
    ($value:expr) => {
        TicketResult::ok(serde_json::to_string(&$value).unwrap_or_default())
    };
}

/// Outcome of a spawn request: either a future to run, or an immediate
/// error (API not configured, test mode).
pub enum SpawnOutcome {
    /// The call is valid; drive this future to completion.
    Future(Pin<Box<dyn Future<Output = TicketResult> + Send>>),
    /// The call can never succeed; complete the ticket with this error
    /// instead of spawning.
    ImmediateError(String),
}

/// Dispatch an async API call by host function name. `params` is the same
/// JSON parameter string the synchronous host function takes.
///
/// Returns `None` if the function name is not async-dispatchable.
pub fn prepare_async_api_call(
    api: &Arc<Api>,
    name: &str,
    params: &str,
    module: Arc<PlaidModule>,
) -> Option<SpawnOutcome> {
    // Test-mode gate first, mirroring the synchronous host functions.
    if module.test_mode && !async_function_allowed_in_test_mode(name) {
        return Some(SpawnOutcome::ImmediateError(format!(
            "{name} is not available in test mode"
        )));
    }

    let params = params.to_string();

    // The future captures a clone of the Arc<Api> and resolves the
    // subsystem inside, so it is 'static and can be driven on the tokio
    // runtime after this call returns. `Api` itself is not Clone (its
    // subsystems hold clients and config), but it is shared behind an Arc
    // for exactly this kind of access.
    macro_rules! go {
        ($sub:ident, $func:ident) => {{
            match api.$sub.as_ref() {
                Some(_) => {
                    let api = api.clone();
                    let fut = async move {
                        let sub = api.$sub.as_ref().expect("subsystem checked at spawn");
                        match sub.$func(&params, module).await {
                            Ok(v) => json_result!(v),
                            Err(e) => TicketResult::err(format!("{e:?}")),
                        }
                    };
                    Some(SpawnOutcome::Future(Box::pin(fut)))
                }
                None => Some(SpawnOutcome::ImmediateError(
                    "The requested API is not configured for Plaid to use".to_string(),
                )),
            }
        }};
    }

    // Nested subsystems (aws.kms, aws.s3, blockchain.evm, gcp.google_docs...).
    macro_rules! sub_go {
        ($api_field:ident, $sub:ident, $func:ident) => {{
            match api.$api_field.as_ref().and_then(|a| a.$sub.as_ref()) {
                Some(_) => {
                    let api = api.clone();
                    let fut = async move {
                        let sub = api
                            .$api_field
                            .as_ref()
                            .and_then(|a| a.$sub.as_ref())
                            .expect("subsystem checked at spawn");
                        match sub.$func(&params, module).await {
                            Ok(v) => json_result!(v),
                            Err(e) => TicketResult::err(format!("{e:?}")),
                        }
                    };
                    Some(SpawnOutcome::Future(Box::pin(fut)))
                }
                None => Some(SpawnOutcome::ImmediateError(
                    "The requested API is not configured for Plaid to use".to_string(),
                )),
            }
        }};
    }

    match name {
        // ---- General ----
        "general_simple_json_post_request" => go!(general, simple_json_post_request),
        "general_make_named_request" => go!(general, make_named_request),
        "general_retrieve_tls_certificate_with_sni" => {
            go!(general, retrieve_tls_certificate_with_sni)
        }

        // ---- GitHub ----
        "github_add_user_to_repo" => go!(github, add_user_to_repo),
        "github_remove_user_from_repo" => go!(github, remove_user_from_repo),
        "github_add_user_to_team" => go!(github, add_user_to_team),
        "github_remove_user_from_team" => go!(github, remove_user_from_team),
        "github_make_graphql_query" => go!(github, make_graphql_query),
        "github_make_advanced_graphql_query" => {
            go!(github, make_advanced_graphql_query)
        }
        "github_fetch_commit" => go!(github, fetch_commit),
        "github_list_files" => go!(github, list_files),
        "github_fetch_file_with_custom_media_type" => {
            go!(github, fetch_file_with_custom_media_type)
        }
        "github_list_fpat_requests_for_org" => go!(github, list_fpat_requests_for_org),
        "github_review_fpat_requests_for_org" => {
            go!(github, review_fpat_requests_for_org)
        }
        "github_get_repos_for_fpat" => go!(github, get_repos_for_fpat),
        "github_get_branch_protection_rules" => {
            go!(github, get_branch_protection_rules)
        }
        "github_get_branch_protection_ruleset" => {
            go!(github, get_branch_protection_ruleset)
        }
        "github_get_repository_collaborators" => {
            go!(github, get_repository_collaborators)
        }
        "github_get_custom_properties_values" => {
            go!(github, get_custom_properties_values)
        }
        "github_check_codeowners_file" => go!(github, check_codeowners_file),
        "github_update_branch_protection_rule" => {
            go!(github, update_branch_protection_rule)
        }
        "github_create_environment_for_repo" => {
            go!(github, create_environment_for_repo)
        }
        "github_configure_secret" => go!(github, configure_secret),
        "github_create_deployment_branch_protection_rule" => {
            go!(github, create_deployment_branch_protection_rule)
        }
        "github_search_code" => go!(github, search_code),
        "github_add_users_to_org_copilot" => go!(github, add_users_to_org_copilot),
        "github_remove_users_from_org_copilot" => {
            go!(github, remove_users_from_org_copilot)
        }
        "github_list_seats_in_org_copilot" => go!(github, list_seats_in_org_copilot),
        "github_trigger_repo_dispatch" => go!(github, trigger_repo_dispatch),
        "github_check_org_membership_of_user" => {
            go!(github, check_org_membership_of_user)
        }
        "github_comment_on_pull_request" => go!(github, comment_on_pull_request),
        "github_delete_deploy_key" => go!(github, delete_deploy_key),
        "github_create_deploy_key" => go!(github, create_deploy_key),
        "github_pull_request_request_reviewers" => {
            go!(github, pull_request_request_reviewers)
        }
        "github_submit_pull_request_review" => {
            go!(github, submit_pull_request_review)
        }
        "github_require_signed_commits" => go!(github, require_signed_commits),
        "github_get_weekly_commit_count" => go!(github, get_weekly_commit_count),
        "github_add_repo_to_team" => go!(github, add_repo_to_team),
        "github_remove_repo_from_team" => go!(github, remove_repo_from_team),
        "github_get_reference" => go!(github, get_reference),
        "github_create_reference" => go!(github, create_reference),
        "github_get_pull_requests" => go!(github, get_pull_requests),
        "github_create_pull_request" => go!(github, create_pull_request),
        "github_create_file" => go!(github, create_file),
        "github_get_repo_sbom" => go!(github, get_repo_sbom),
        "github_add_labels" => go!(github, add_labels),
        "github_get_user_id_from_username" => go!(github, get_user_id_from_username),
        "github_get_username_from_user_id" => {
            go!(github, get_username_from_user_id)
        }
        "github_get_repo_id_from_repo_name" => {
            go!(github, get_repo_id_from_repo_name)
        }
        "github_get_repo_name_from_repo_id" => {
            go!(github, get_repo_name_from_repo_id)
        }
        "github_get_repo_teams" => go!(github, get_repo_teams),
        "github_remove_outside_collaborator_from_org" => {
            go!(github, remove_outside_collaborator_from_org)
        }
        "github_add_repo_to_org_secret" => go!(github, add_repo_to_org_secret),
        "github_remove_repo_from_org_secret" => {
            go!(github, remove_repo_from_org_secret)
        }
        "github_list_org_secrets_for_repo" => go!(github, list_org_secrets_for_repo),
        "github_grant_repo_access_to_org_installation" => {
            go!(github, grant_repo_access_to_org_installation)
        }
        "github_remove_repo_access_from_org_installation" => {
            go!(github, remove_repo_access_from_org_installation)
        }
        "github_get_enterprise_license_status" => {
            go!(github, get_enterprise_license_status)
        }
        "github_create_installation_access_token" => {
            go!(github, create_installation_access_token)
        }
        "github_revoke_installation_access_token" => {
            go!(github, revoke_installation_access_token)
        }
        "github_merge_pr" => go!(github, merge_pr),

        // ---- Slack ----
        "slack_post_to_named_webhook" => go!(slack, post_to_named_webhook),
        "slack_post_to_arbitrary_webhook" => go!(slack, post_to_arbitrary_webhook),
        "slack_post_message" => go!(slack, post_message),
        "slack_views_open" => go!(slack, views_open),
        "slack_get_id_from_email" => go!(slack, get_id_from_email),
        "slack_get_presence" => go!(slack, get_presence),
        "slack_get_dnd" => go!(slack, get_dnd),
        "slack_user_info" => go!(slack, user_info),
        "slack_create_channel" => go!(slack, create_channel),
        "slack_invite_to_channel" => go!(slack, invite_to_channel),
        "slack_update_message" => go!(slack, update_message),
        "slack_remove_from_channel" => go!(slack, remove_from_channel),
        "slack_schedule_message" => go!(slack, schedule_message),
        "slack_delete_scheduled_message" => {
            go!(slack, delete_scheduled_message)
        }
        "slack_conversations_history" => go!(slack, conversations_history),

        // ---- Jira ----
        "jira_create_issue" => go!(jira, create_issue),
        "jira_get_issue" => go!(jira, get_issue),
        "jira_update_issue" => go!(jira, update_issue),
        "jira_get_user" => go!(jira, get_user),
        "jira_post_comment" => go!(jira, post_comment),
        "jira_search_issues" => go!(jira, search_issues),

        // ---- npm ----
        "npm_publish_empty_stub" => go!(npm, publish_empty_stub),
        "npm_set_team_permission_on_package" => {
            go!(npm, set_team_permission_on_package)
        }
        "npm_create_granular_token_for_packages" => {
            go!(npm, create_granular_token_for_packages)
        }
        "npm_delete_granular_token" => go!(npm, delete_granular_token),
        "npm_list_granular_tokens" => go!(npm, list_granular_tokens),
        "npm_delete_package" => go!(npm, delete_package),
        "npm_add_user_to_team" => go!(npm, add_user_to_team),
        "npm_remove_user_from_team" => go!(npm, remove_user_from_team),
        "npm_remove_user_from_organization" => {
            go!(npm, remove_user_from_organization)
        }
        "npm_invite_user_to_organization" => {
            go!(npm, invite_user_to_organization)
        }
        "npm_get_org_user_list" => go!(npm, get_org_user_list),
        "npm_get_org_users_without_2fa" => go!(npm, get_org_users_without_2fa),
        "npm_list_packages_with_team_permission" => {
            go!(npm, list_packages_with_team_permission)
        }
        "npm_get_token_details" => go!(npm, get_token_details),

        // ---- Okta ----
        "okta_remove_user_from_group" => go!(okta, remove_user_from_group),
        "okta_get_user_data" => go!(okta, get_user_data),

        // ---- PagerDuty ----
        "pagerduty_trigger_incident" => go!(pagerduty, trigger_incident),
        "pagerduty_get_incident_alerts" => go!(pagerduty, get_incident_alerts),

        // ---- Splunk ----
        "splunk_post_hec" => go!(splunk, post_hec),

        // ---- Yubikey ----
        "yubikey_verify_otp" => go!(yubikey, verify_otp),

        // ---- Web ----
        "web_issue_jwt" => go!(web, issue_jwt),

        // ---- Rustica ----
        "rustica_new_mtls_cert" => go!(rustica, new_mtls_cert),

        // ---- Cryptography ----
        "cryptography_aes_128_cbc_encrypt" => {
            go!(cryptography, aes_128_cbc_encrypt)
        }
        "cryptography_aes_128_cbc_decrypt" => {
            go!(cryptography, aes_128_cbc_decrypt)
        }

        // ---- Bloom filter ----
        "bloom_filter_build_with_items" => go!(bloom_filter, build_with_items),

        // ---- Blockchain: EVM ----
        "blockchain_evm_get_transaction_by_hash" => {
            sub_go!(blockchain, evm, get_transaction_by_hash)
        }
        "blockchain_evm_get_transaction_receipt" => {
            sub_go!(blockchain, evm, get_transaction_receipt)
        }
        "blockchain_evm_send_raw_transaction" => {
            sub_go!(blockchain, evm, send_raw_transaction)
        }
        "blockchain_evm_get_transaction_count" => {
            sub_go!(blockchain, evm, get_transaction_count)
        }
        "blockchain_evm_get_balance" => sub_go!(blockchain, evm, get_balance),
        "blockchain_evm_estimate_gas" => sub_go!(blockchain, evm, estimate_gas),
        "blockchain_evm_eth_call" => sub_go!(blockchain, evm, eth_call),
        "blockchain_evm_gas_price" => sub_go!(blockchain, evm, gas_price),
        "blockchain_evm_get_logs" => sub_go!(blockchain, evm, get_logs),
        "blockchain_evm_get_block" => sub_go!(blockchain, evm, get_block),
        "blockchain_evm_get_fee_history" => {
            sub_go!(blockchain, evm, get_fee_history)
        }

        // ---- Blockchain: Solana ----
        "blockchain_solana_send_signed_transaction" => {
            sub_go!(blockchain, solana, send_signed_transaction)
        }
        "blockchain_solana_get_balance" => {
            sub_go!(blockchain, solana, get_balance)
        }
        "blockchain_solana_get_account_info" => {
            sub_go!(blockchain, solana, get_account_info)
        }
        "blockchain_solana_get_slot" => sub_go!(blockchain, solana, get_slot),
        "blockchain_solana_get_latest_blockhash" => {
            sub_go!(blockchain, solana, get_latest_blockhash)
        }
        "blockchain_solana_get_transaction_count" => {
            sub_go!(blockchain, solana, get_transaction_count)
        }
        "blockchain_solana_get_transaction" => {
            sub_go!(blockchain, solana, get_transaction)
        }
        "blockchain_solana_get_signature_statuses" => {
            sub_go!(blockchain, solana, get_signature_statuses)
        }
        "blockchain_solana_get_block" => {
            sub_go!(blockchain, solana, get_block)
        }
        "blockchain_solana_get_multiple_accounts" => {
            sub_go!(blockchain, solana, get_multiple_accounts)
        }
        "blockchain_solana_get_program_accounts" => {
            sub_go!(blockchain, solana, get_program_accounts)
        }
        "blockchain_solana_get_token_accounts_by_owner" => {
            sub_go!(blockchain, solana, get_token_accounts_by_owner)
        }
        "blockchain_solana_get_token_account_balance" => {
            sub_go!(blockchain, solana, get_token_account_balance)
        }
        "blockchain_solana_get_token_supply" => {
            sub_go!(blockchain, solana, get_token_supply)
        }
        "blockchain_solana_get_minimum_balance_for_rent_exemption" => {
            sub_go!(blockchain, solana, get_minimum_balance_for_rent_exemption)
        }
        "blockchain_solana_get_fee_for_message" => {
            sub_go!(blockchain, solana, get_fee_for_message)
        }
        "blockchain_solana_get_recent_prioritization_fees" => {
            sub_go!(blockchain, solana, get_recent_prioritization_fees)
        }
        "blockchain_solana_simulate_transaction" => {
            sub_go!(blockchain, solana, simulate_transaction)
        }
        "blockchain_solana_get_signatures_for_address" => {
            sub_go!(blockchain, solana, get_signatures_for_address)
        }

        // ---- AWS (feature-gated) ----
        #[cfg(feature = "aws")]
        "aws_kms_generate_mac" => sub_go!(aws, kms, generate_mac),
        #[cfg(feature = "aws")]
        "aws_kms_verify_mac" => sub_go!(aws, kms, verify_mac),
        #[cfg(feature = "aws")]
        "aws_kms_sign_arbitrary_message" => {
            sub_go!(aws, kms, sign_arbitrary_message)
        }
        #[cfg(feature = "aws")]
        "aws_kms_get_public_key" => sub_go!(aws, kms, get_public_key),
        #[cfg(feature = "aws")]
        "aws_dynamodb_put_item" => sub_go!(aws, dynamodb, put_item),
        #[cfg(feature = "aws")]
        "aws_dynamodb_delete_item" => sub_go!(aws, dynamodb, delete_item),
        #[cfg(feature = "aws")]
        "aws_dynamodb_query" => sub_go!(aws, dynamodb, query),
        #[cfg(feature = "aws")]
        "aws_s3_delete_object" => sub_go!(aws, s3, delete_object),
        #[cfg(feature = "aws")]
        "aws_s3_get_object" => sub_go!(aws, s3, get_object),
        #[cfg(feature = "aws")]
        "aws_s3_get_object_attributes" => {
            sub_go!(aws, s3, get_object_attributes)
        }
        #[cfg(feature = "aws")]
        "aws_s3_list_objects" => sub_go!(aws, s3, list_objects),
        #[cfg(feature = "aws")]
        "aws_s3_list_object_versions" => {
            sub_go!(aws, s3, list_object_versions)
        }
        #[cfg(feature = "aws")]
        "aws_s3_put_object" => sub_go!(aws, s3, put_object),
        #[cfg(feature = "aws")]
        "aws_s3_put_object_tags" => sub_go!(aws, s3, put_object_tags),

        // ---- GCP (feature-gated) ----
        #[cfg(feature = "gcp")]
        "gcp_google_docs_upload_file" => {
            sub_go!(gcp, google_docs, upload_file)
        }
        #[cfg(feature = "gcp")]
        "gcp_google_docs_copy_file" => sub_go!(gcp, google_docs, copy_file),
        #[cfg(feature = "gcp")]
        "gcp_google_docs_create_folder" => {
            sub_go!(gcp, google_docs, create_folder)
        }
        #[cfg(feature = "gcp")]
        "gcp_google_docs_create_doc_from_markdown" => {
            sub_go!(gcp, google_docs, create_doc_from_markdown)
        }
        #[cfg(feature = "gcp")]
        "gcp_google_docs_create_sheet_from_csv" => {
            sub_go!(gcp, google_docs, create_sheet_from_csv)
        }
        #[cfg(feature = "gcp")]
        "gcp_bigquery_query_table" => sub_go!(gcp, bigquery, query_table),

        _ => None,
    }
}

/// Functions that may be spawned while the owning module is in test mode.
/// This list mirrors the `ALLOW_IN_TEST_MODE` entries in `functions/api.rs`.
#[cfg(feature = "aws")] // MM: why is this gated behind the AWS feature? It seems to make no sense. Oh there is another list below... hmmm... not sure how clean this is.
const TEST_MODE_ALLOWED: &[&str] = &[
    "general_simple_json_post_request",
    "general_make_named_request",
    "general_retrieve_tls_certificate_with_sni",
    "github_make_graphql_query",
    "github_make_advanced_graphql_query",
    "github_fetch_commit",
    "github_list_files",
    "github_fetch_file_with_custom_media_type",
    "github_list_fpat_requests_for_org",
    "github_get_repos_for_fpat",
    "github_get_branch_protection_rules",
    "github_get_branch_protection_ruleset",
    "github_get_repository_collaborators",
    "github_search_code",
    "github_list_seats_in_org_copilot",
    "github_get_custom_properties_values",
    "github_check_codeowners_file",
    "github_get_repo_sbom",
    "github_get_weekly_commit_count",
    "github_get_reference",
    "github_get_pull_requests",
    "github_get_user_id_from_username",
    "github_get_username_from_user_id",
    "github_get_repo_id_from_repo_name",
    "github_get_repo_name_from_repo_id",
    "github_get_repo_teams",
    "github_list_org_secrets_for_repo",
    "github_get_enterprise_license_status",
    "github_check_org_membership_of_user",
    "slack_views_open",
    "slack_post_to_named_webhook",
    "slack_post_to_arbitrary_webhook",
    "slack_post_message",
    "slack_update_message",
    "slack_schedule_message",
    "slack_delete_scheduled_message",
    "slack_conversations_history",
    "slack_get_id_from_email",
    "slack_get_presence",
    "slack_get_dnd",
    "slack_user_info",
    "jira_get_issue",
    "jira_get_user",
    "jira_search_issues",
    "npm_list_granular_tokens",
    "npm_get_org_user_list",
    "npm_get_org_users_without_2fa",
    "npm_list_packages_with_team_permission",
    "npm_get_token_details",
    "okta_get_user_data",
    "pagerduty_get_incident_alerts",
    "splunk_post_hec",
    "yubikey_verify_otp",
    "cryptography_aes_128_cbc_encrypt",
    "cryptography_aes_128_cbc_decrypt",
    "bloom_filter_build_with_items",
    "blockchain_evm_get_transaction_by_hash",
    "blockchain_evm_get_transaction_receipt",
    "blockchain_evm_get_transaction_count",
    "blockchain_evm_get_balance",
    "blockchain_evm_estimate_gas",
    "blockchain_evm_eth_call",
    "blockchain_evm_gas_price",
    "blockchain_evm_get_logs",
    "blockchain_evm_get_block",
    "blockchain_evm_get_fee_history",
    "blockchain_solana_get_balance",
    "blockchain_solana_get_account_info",
    "blockchain_solana_get_slot",
    "blockchain_solana_get_latest_blockhash",
    "blockchain_solana_get_transaction_count",
    "blockchain_solana_get_transaction",
    "blockchain_solana_get_signature_statuses",
    "blockchain_solana_get_block",
    "blockchain_solana_get_multiple_accounts",
    "blockchain_solana_get_program_accounts",
    "blockchain_solana_get_token_accounts_by_owner",
    "blockchain_solana_get_token_account_balance",
    "blockchain_solana_get_token_supply",
    "blockchain_solana_get_minimum_balance_for_rent_exemption",
    "blockchain_solana_get_fee_for_message",
    "blockchain_solana_get_recent_prioritization_fees",
    "blockchain_solana_simulate_transaction",
    "blockchain_solana_get_signatures_for_address",
    "aws_kms_get_public_key",
    "aws_dynamodb_query",
    "aws_s3_get_object",
    "aws_s3_get_object_attributes",
    "aws_s3_list_objects",
    "aws_s3_list_object_versions",
    "gcp_bigquery_query_table",
];

/// Same list without the AWS/GCP entries, for builds with those features
/// disabled.
#[cfg(not(feature = "aws"))] // MM: what's the link with GCP here? I don't love this.
const TEST_MODE_ALLOWED: &[&str] = &[
    "general_simple_json_post_request",
    "general_make_named_request",
    "general_retrieve_tls_certificate_with_sni",
    "github_make_graphql_query",
    "github_make_advanced_graphql_query",
    "github_fetch_commit",
    "github_list_files",
    "github_fetch_file_with_custom_media_type",
    "github_list_fpat_requests_for_org",
    "github_get_repos_for_fpat",
    "github_get_branch_protection_rules",
    "github_get_branch_protection_ruleset",
    "github_get_repository_collaborators",
    "github_search_code",
    "github_list_seats_in_org_copilot",
    "github_get_custom_properties_values",
    "github_check_codeowners_file",
    "github_get_repo_sbom",
    "github_get_weekly_commit_count",
    "github_get_reference",
    "github_get_pull_requests",
    "github_get_user_id_from_username",
    "github_get_username_from_user_id",
    "github_get_repo_id_from_repo_name",
    "github_get_repo_name_from_repo_id",
    "github_get_repo_teams",
    "github_list_org_secrets_for_repo",
    "github_get_enterprise_license_status",
    "github_check_org_membership_of_user",
    "slack_views_open",
    "slack_post_to_named_webhook",
    "slack_post_to_arbitrary_webhook",
    "slack_post_message",
    "slack_update_message",
    "slack_schedule_message",
    "slack_delete_scheduled_message",
    "slack_conversations_history",
    "slack_get_id_from_email",
    "slack_get_presence",
    "slack_get_dnd",
    "slack_user_info",
    "jira_get_issue",
    "jira_get_user",
    "jira_search_issues",
    "npm_list_granular_tokens",
    "npm_get_org_user_list",
    "npm_get_org_users_without_2fa",
    "npm_list_packages_with_team_permission",
    "npm_get_token_details",
    "okta_get_user_data",
    "pagerduty_get_incident_alerts",
    "splunk_post_hec",
    "yubikey_verify_otp",
    "cryptography_aes_128_cbc_encrypt",
    "cryptography_aes_128_cbc_decrypt",
    "bloom_filter_build_with_items",
    "blockchain_evm_get_transaction_by_hash",
    "blockchain_evm_get_transaction_receipt",
    "blockchain_evm_get_transaction_count",
    "blockchain_evm_get_balance",
    "blockchain_evm_estimate_gas",
    "blockchain_evm_eth_call",
    "blockchain_evm_gas_price",
    "blockchain_evm_get_logs",
    "blockchain_evm_get_block",
    "blockchain_evm_get_fee_history",
    "blockchain_solana_get_balance",
    "blockchain_solana_get_account_info",
    "blockchain_solana_get_slot",
    "blockchain_solana_get_latest_blockhash",
    "blockchain_solana_get_transaction_count",
    "blockchain_solana_get_transaction",
    "blockchain_solana_get_signature_statuses",
    "blockchain_solana_get_block",
    "blockchain_solana_get_multiple_accounts",
    "blockchain_solana_get_program_accounts",
    "blockchain_solana_get_token_accounts_by_owner",
    "blockchain_solana_get_token_account_balance",
    "blockchain_solana_get_token_supply",
    "blockchain_solana_get_minimum_balance_for_rent_exemption",
    "blockchain_solana_get_fee_for_message",
    "blockchain_solana_get_recent_prioritization_fees",
    "blockchain_solana_simulate_transaction",
    "blockchain_solana_get_signatures_for_address",
];

/// Returns `true` if `name` may be spawned in test mode.
pub fn async_function_allowed_in_test_mode(name: &str) -> bool {
    TEST_MODE_ALLOWED.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_list_entries_follow_naming_conventions() {
        // Every test-mode-allowed name must look like a real host function
        // name. This catches typos in the list. We can't probe the dispatch
        // table without an Api instance, so we check the known API prefixes.
        for name in TEST_MODE_ALLOWED {
            let valid = [
                "general_", "github_", "slack_", "jira_", "okta_", "pagerduty_", "splunk_",
                "yubikey_", "cryptography_", "bloom_filter_", "blockchain_", "aws_", "gcp_",
                "web_", "rustica_", "npm_",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix));
            assert!(valid, "unexpected test-mode entry: {name}");
        }
    }

    #[test]
    fn test_mode_gate_rejects_side_effecting_functions() {
        assert!(!async_function_allowed_in_test_mode(
            "github_add_user_to_repo"
        ));
        assert!(!async_function_allowed_in_test_mode(
            "github_create_file"
        ));
        assert!(async_function_allowed_in_test_mode(
            "general_make_named_request"
        ));
    }
}
