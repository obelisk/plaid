use serde::{Deserialize, Serialize};

use crate::{github::RepositoryDispatchParams, PlaidFunctionError};

/// Trigger a GHA workflow via repository_dispatch.
/// For more details, see
/// * https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#create-a-repository-dispatch-event
/// * https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows#repository_dispatch
pub fn trigger_repo_dispatch<T>(
    owner: &str,
    repo: &str,
    event_type: &str,
    client_payload: T,
) -> Result<(), PlaidFunctionError>
where
    T: Serialize + Deserialize<'static>,
{
    extern "C" {
        new_host_function!(github, trigger_repo_dispatch);
    }

    let params = RepositoryDispatchParams::<T> {
        owner: owner.to_string(),
        repo: repo.to_string(),
        event_type: event_type.to_string(),
        client_payload,
    };

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_trigger_repo_dispatch(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
