use std::collections::HashMap;

use crate::apis::ApiError;

use super::{GitHubError, Github};

// new_regex_validator!("REPOSITORY_NAME", "^[\w\-\.]+$");

macro_rules! define_regex_validator {
    ($validators:expr,$validator:tt,$regex: tt) => {
        $validators.insert(
            $validator,
            regex::Regex::new($regex)
                .expect(format!("Failed to create {} validator", stringify!($validator)).as_str()),
        );
    };
}

macro_rules! create_regex_validator_func {
    ($validator:ident) => {
        paste::item! {
            impl Github {
                pub fn [< validate_ $validator>]<'a> (&self, value_to_validate: &'a str) -> Result<&'a str, ApiError> {
                    let validator = if let Some(validator) = self.validators.get(stringify!($validator)) {
                        validator
                    } else {
                        error!("Validator {} not found in GitHub API. This should be impossible.", stringify!($validator));
                        return Err(ApiError::ImpossibleError);
                    };

                    validator.is_match(value_to_validate)
                        .then(|| value_to_validate)
                        .ok_or(ApiError::GitHubError(GitHubError::InvalidInput(value_to_validate.to_string())))
                }
            }
        }
    }
}

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

    // This validates a postive integer
    define_regex_validator!(validators, "pint", r"^\d+$");

    // Follows Github's guidelines on branch naming conventions
    // https://docs.github.com/en/get-started/using-git/dealing-with-special-characters-in-branch-and-tag-names#naming-branches-and-tags
    define_regex_validator!(validators, "branch_name", r"^[a-zA-Z][a-zA-Z0-9./_-]*$");

    define_regex_validator!(validators, "environment_name", r"^[a-zA-Z][a-zA-Z0-9./_-]*$");
    define_regex_validator!(validators, "secret_name", r"^[A-Z][A-Z0-9_]*$");
    define_regex_validator!(validators, "filename", r"^[a-zA-Z0-9\.]{1,32}$");

    validators
}

create_regex_validator_func!(repository_name);
create_regex_validator_func!(username);
create_regex_validator_func!(org);
create_regex_validator_func!(team_slug);
create_regex_validator_func!(commit_hash);
create_regex_validator_func!(pint);
create_regex_validator_func!(branch_name);
create_regex_validator_func!(environment_name);
create_regex_validator_func!(secret_name);
create_regex_validator_func!(filename);
