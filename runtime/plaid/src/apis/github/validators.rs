use std::collections::HashMap;

use plaid_stl::github::PullRequestReviewEvent;

use crate::apis::{
    validators::{create_regex_validator_func, define_regex_validator},
    ApiError,
};

use super::{GitHubError, Github};

/// Initialize all available validators.
pub fn create_validators() -> HashMap<&'static str, regex::Regex> {
    let mut validators = HashMap::new();

    // This should be the same as GitHub's actual validation.
    define_regex_validator!(validators, "repository_name", r"^[\w\-\./]+$");

    // This is less strict than GitHub's actual requirements but it's good enough
    // to ensure safety.
    define_regex_validator!(validators, "username", r"^[\w\-]+$");

    // We assume that orgs follow the same rules as users
    define_regex_validator!(validators, "org", r"^[\w\-]+$");

    // This technically allows team slugs with underscores even though GitHub doesn't
    // allow that. However for our purposes of safety this should be fine.
    define_regex_validator!(validators, "team_slug", r"^[\w\-]+$");

    // This validates a SHA-1 hash which is what all commit hashes are.
    define_regex_validator!(validators, "commit_hash", r"^\b([a-f0-9]{40})\b$");

    // This validates a positive integer
    define_regex_validator!(validators, "pint", r"^\d+$");

    // Follows Github's guidelines on branch naming conventions
    // https://docs.github.com/en/get-started/using-git/dealing-with-special-characters-in-branch-and-tag-names#naming-branches-and-tags
    define_regex_validator!(validators, "branch_name", r"^[a-zA-Z][a-zA-Z0-9./_-]*$");

    define_regex_validator!(
        validators,
        "environment_name",
        r"^[a-zA-Z][a-zA-Z0-9./_-]*$"
    );
    define_regex_validator!(validators, "secret_name", r"^[A-Z][A-Z0-9_]*$");
    define_regex_validator!(validators, "filename", r"^[a-zA-Z0-9\.]{1,32}$");
    define_regex_validator!(validators, "extension", r"^[a-zA-Z0-9]{1,5}$");
    define_regex_validator!(validators, "path", r"^[a-zA-Z0-9\./_-]{1,128}$");

    define_regex_validator!(validators, "event_type", r"^[\w\-_]+$");

    // Matches if .. is present in a file path
    define_regex_validator!(validators, "contains_parent_directory_component", r"\.\.");

    validators
}

/// Register a GitHub validator, mapping rejections to `GitHubError::InvalidInput`.
macro_rules! github_validator {
    ($validator:ident) => {
        create_regex_validator_func!(Github, $validator, |value| ApiError::GitHubError(
            GitHubError::InvalidInput(value)
        ));
    };
}

github_validator!(repository_name);
github_validator!(username);
github_validator!(org);
github_validator!(team_slug);
github_validator!(commit_hash);
github_validator!(pint);
github_validator!(branch_name);
github_validator!(environment_name);
github_validator!(secret_name);
github_validator!(filename);
github_validator!(extension);
github_validator!(path);
github_validator!(event_type);
github_validator!(contains_parent_directory_component);

impl Github {
    pub fn validate_user_id<'a>(&self, user_id: &'a str) -> Result<&'a str, ApiError> {
        // User IDs are positive integers
        self.validate_pint(user_id)
    }

    pub fn validate_repo_id<'a>(&self, repo_id: &'a str) -> Result<&'a str, ApiError> {
        // Repo IDs are positive integers
        self.validate_pint(repo_id)
    }

    /// Ensure a pull request review has a body when GitHub requires one.
    /// A non-empty `body` is mandatory for `RequestChanges` and `Comment`;
    /// it is optional for `Approve`.
    pub fn validate_review_body(
        &self,
        event: PullRequestReviewEvent,
        body: Option<&str>,
    ) -> Result<(), ApiError> {
        match (event, body) {
            (PullRequestReviewEvent::Approve, _) => Ok(()),
            (_, Some(body)) if !body.is_empty() => Ok(()),
            (_, _) => Err(ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "a non-empty body is required for review event {event:?}"
            )))),
        }
    }
}
