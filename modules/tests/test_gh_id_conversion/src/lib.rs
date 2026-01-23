use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source, github, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_with_source!();

const USERNAME: &str = "obelisk";
const USER_ID: &str = "2386877";
const REPO_OWNER: &str = "obelisk";
const REPO_NAME: &str = "plaid";
const REPO_ID: i64 = 777071534;

fn main(_log: String, _source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("testing gh_id_conversion"));

    let user_id = github::get_user_id_from_username(USERNAME).unwrap();
    if user_id != USER_ID {
        plaid::print_debug_string(&format!("Expected user ID [{USER_ID}] but got [{user_id}]"));
        return Err(-1);
    }

    let username = github::get_username_from_user_id(USER_ID).unwrap();
    if username != USERNAME {
        plaid::print_debug_string(&format!(
            "Expected username [{USERNAME}] but got [{username}]"
        ));
        return Err(-1);
    }

    let repo_id = github::get_repo_id_from_repo_name(REPO_OWNER, REPO_NAME).unwrap();
    if repo_id != REPO_ID {
        plaid::print_debug_string(&format!("Expected repo ID [{REPO_ID}] but got [{repo_id}]"));
        return Err(-1);
    }

    let repo_name = github::get_repo_name_from_repo_id(REPO_ID.to_string()).unwrap();
    if repo_name != format!("{REPO_OWNER}/{REPO_NAME}") {
        plaid::print_debug_string(&format!(
            "Expected repo name [{REPO_OWNER}/{REPO_NAME}] but got [{repo_name}]"
        ));
        return Err(-1);
    }

    // If we are here, then everything worked fine (no unwraps or early returns), so we send an OK
    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
