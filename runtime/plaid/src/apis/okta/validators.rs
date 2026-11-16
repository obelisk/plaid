use std::collections::HashMap;

use crate::apis::{
    validators::{create_regex_validator_func, define_regex_validator},
    ApiError,
};

use super::{Okta, OktaError};

/// Initialize all available validators.
pub fn create_validators() -> HashMap<&'static str, regex::Regex> {
    let mut validators = HashMap::new();

    // Okta identifiers (users, groups, group rules, apps, etc.) are alphanumeric.
    //
    // Okta does not document their format, so we bound the length rather than
    // assuming the 20 characters observed in practice:
    // https://devforum.okta.com/t/is-user-id-unique-and-always-20-character-length/2487
    //
    // Restricting to alphanumerics guarantees the value is a single URL path
    // segment, so it cannot alter the path of the request it is interpolated into.
    define_regex_validator!(validators, "okta_id", r"^[a-zA-Z0-9]{1,128}$");

    validators
}

create_regex_validator_func!(Okta, okta_id, |value| ApiError::OktaError(
    OktaError::InvalidInput(value)
));
