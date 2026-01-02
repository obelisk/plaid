use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source, github, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_with_source!();

const USERNAME: &str = "obelisk";
const USER_ID: &str = "2386877";

fn main(_log: String, _source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("testing gh_username_id"));

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

    // If we are here, then everything worked fine (no unwraps or early returns), so we send an OK
    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
