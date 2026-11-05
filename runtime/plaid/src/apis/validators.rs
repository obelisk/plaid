//! Shared helpers for validating module-supplied parameters before they are used
//! to build requests to external APIs.

use std::collections::HashMap;

use crate::apis::ApiError;

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

pub(crate) use define_regex_validator;

/// Macro that creates a `validate_<name>` method on an API struct, backed by the
/// regex registered under `<name>` in that struct's `validators` map.
///
/// `$error` is a function or closure that turns a rejected value into the API's
/// own `ApiError` variant.
macro_rules! create_regex_validator_func {
    ($api:ident, $validator:ident, $error:expr) => {
        paste::item! {
            impl $api {
                pub fn [< validate_ $validator>]<'a> (&self, value_to_validate: &'a str) -> Result<&'a str, $crate::apis::ApiError> {
                    $crate::apis::validators::validate_with(
                        &self.validators,
                        stringify!($validator),
                        value_to_validate,
                        $error,
                    )
                }
            }
        }
    }
}

pub(crate) use create_regex_validator_func;

/// Check `value` against the regex registered under `name`, returning the value
/// unchanged when it matches and `make_error(value)` when it does not.
pub fn validate_with<'a>(
    validators: &HashMap<&'static str, regex::Regex>,
    name: &str,
    value: &'a str,
    make_error: impl FnOnce(String) -> ApiError,
) -> Result<&'a str, ApiError> {
    let validator = match validators.get(name) {
        Some(validator) => validator,
        None => {
            error!("Validator {name} not found. This should be impossible.");
            return Err(ApiError::ImpossibleError);
        }
    };

    if validator.is_match(value) {
        Ok(value)
    } else {
        Err(make_error(value.to_string()))
    }
}
