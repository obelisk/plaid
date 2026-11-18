use plaid_stl::{entrypoint_with_source, github, messages::LogSource};

entrypoint_with_source!();

fn main(_log: String, _source: LogSource) -> Result<(), i32> {
    let _ = github::get_user_id_from_username("noauth", "octocat");
    Ok(())
}
