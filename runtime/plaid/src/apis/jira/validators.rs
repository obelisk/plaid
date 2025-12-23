use super::Jira;
use crate::apis::jira::JiraError;
use crate::apis::ApiError;
use std::collections::HashMap;

/// Macro that compiles a regex validator and inserts it into a list of available validators.
macro_rules! define_regex_validator {
    ($validators:expr,$validator:tt,$regex: tt) => {
        $validators.insert(
            $validator,
            regex::Regex::new($regex)
                .expect(format!("Failed to create {} validator", stringify!($validator)).as_str()),
        );
    };
}

/// Macro that creates a function which uses a given validator to validate an input.
macro_rules! create_regex_validator_func {
    ($validator:ident) => {
        paste::item! {
            impl Jira {
                pub fn [< validate_ $validator>]<'a> (&self, value_to_validate: &'a str) -> Result<&'a str, ApiError> {
                    let validator = if let Some(validator) = self.validators.get(stringify!($validator)) {
                        validator
                    } else {
                        error!("Validator {} not found in Jira API. This should be impossible.", stringify!($validator));
                        return Err(ApiError::ImpossibleError);
                    };

                    validator.is_match(value_to_validate)
                        .then(|| value_to_validate)
                        .ok_or(ApiError::JiraError(JiraError::InvalidInput(value_to_validate.to_string())))
                }
            }
        }
    }
}

/// Initialize all available validators.
pub fn create_validators() -> HashMap<&'static str, regex::Regex> {
    let mut validators = HashMap::new();

    // This regex checks that we have a single '@' between the local part and the domain
    // and that there is at least one '.' in the domain, with no whitespaces anywhere.
    // Note - This regex is not perfect (e.g., it would not accept john@localhost but would
    // accept john@-mydomain.com) but it should be sufficient for the job here.
    define_regex_validator!(validators, "email", r"^[^\s@]+@[^\s@]+\.[^\s@]+$");

    // This regex checks that a string is a valid Jira issue ID, like ABC-123.
    // We mandate between 1 and 10 letters, underscores and dashes, a single dash, and up to 10 numbers.
    // It will accept MYPROJ-123, MY-PROJ-123, MY_PROJ-123.
    define_regex_validator!(validators, "issue_id", r"^[A-Za-z_-]{1,10}-\d{1,10}$");

    validators
}

create_regex_validator_func!(email);
create_regex_validator_func!(issue_id);
