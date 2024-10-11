use std::collections::HashMap;

use plaid_stl::npm::shared_structs::GranularTokenSpecs;

use super::{Npm, NpmError};
use crate::apis::ApiError;

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
            impl Npm {
                pub fn [< validate_ $validator>]<'a> (&self, value_to_validate: &'a str) -> Result<&'a str, ApiError> {
                    let validator = if let Some(validator) = self.validators.get(stringify!($validator)) {
                        validator
                    } else {
                        error!("Validator {} not found in npm API. This should be impossible.", stringify!($validator));
                        return Err(ApiError::ImpossibleError);
                    };

                    validator.is_match(value_to_validate)
                        .then(|| value_to_validate)
                        .ok_or(ApiError::NpmError(NpmError::InvalidInput(value_to_validate.to_string())))
                }
            }
        }
    }
}

pub fn create_validators() -> HashMap<&'static str, regex::Regex> {
    let mut validators = HashMap::new();

    // Validate an _unscoped_ package name. The scope is added later
    define_regex_validator!(
        validators,
        "npm_package_name",
        r"^[a-z0-9-_]+(?:\.[a-z0-9-_]+)*$"
    );

    // Validate a team's name for npm: it is possible more combinations are allowed, but we choose to be rather strict
    define_regex_validator!(validators, "npm_team_name", r"^[a-zA-Z0-9-_]+$");

    // Validate an npm username. This is probably stricter than necessary
    define_regex_validator!(validators, "npm_username", r"^[a-zA-Z0-9-_]+$");

    define_regex_validator!(
        validators,
        "ipv4",
        r"^((25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9]).){3}(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])$"
    );
    define_regex_validator!(validators, "npm_org_name", r"^[a-z0-9]+(-[a-z0-9]+)*$");
    define_regex_validator!(validators, "npm_at_org_name", r"^@[a-z0-9]+(-[a-z0-9]+)*$");
    define_regex_validator!(
        validators,
        "npm_scoped_package",
        r"^@[a-z0-9]+(-[a-z0-9]+)*\/[a-z0-9-_]+(?:\.[a-z0-9-_]+)*$"
    );

    // probably more strict than necessary
    define_regex_validator!(validators, "npm_token_name", r"^[a-z0-9._-]+$");

    define_regex_validator!(
        validators,
        "dsr_manifest_hash",
        r#"dsrManifestHash\"\s+?value=\"([a-z0-9]{64})\""#
    );

    define_regex_validator!(validators, "repository_name", r"^[\w\-\./]+$");
    
    // The token ID is actually a UUID
    define_regex_validator!(validators, "token_id", r"^[a-f0-9-]{36}$");

    validators
}

create_regex_validator_func!(npm_package_name);
create_regex_validator_func!(npm_team_name);
create_regex_validator_func!(npm_username);
create_regex_validator_func!(ipv4);
create_regex_validator_func!(npm_org_name);
create_regex_validator_func!(npm_at_org_name);
create_regex_validator_func!(npm_scoped_package);
create_regex_validator_func!(npm_token_name);
create_regex_validator_func!(repository_name);
create_regex_validator_func!(token_id);

impl Npm {
    /// Look for a valid `dsrManifestHash` in an HTML page and extract it
    pub fn validate_and_extract_dsr_manifest_hash<'a>(
        &self,
        value_to_validate: &'a str,
    ) -> Result<&'a str, ApiError> {
        self.validators
            .get("dsr_manifest_hash")
            .ok_or(ApiError::NpmError(NpmError::WrongConfig(
                "Validator not found. This should be impossible".to_string(),
            )))?
            .captures(&value_to_validate)
            .ok_or(ApiError::NpmError(NpmError::InvalidInput(
                value_to_validate.to_string(),
            )))?
            .get(1) // get the content of the capturing group, which contains the token we need
            .ok_or(ApiError::NpmError(NpmError::InvalidInput(
                value_to_validate.to_string(),
            )))
            .map(|v| v.as_str())
            .map_err(|_| ApiError::NpmError(NpmError::InvalidInput(value_to_validate.to_string())))
    }

    /// Special validator for GranularTokenSpecs
    pub fn validate_granular_token_specs<'a>(
        &self,
        value_to_validate: &'a GranularTokenSpecs,
    ) -> Result<&'a GranularTokenSpecs, ApiError> {
        // Validate each field of the struct

        // pub allowed_ip_ranges: Option<Vec<String>>,
        if let Some(ref ips) = value_to_validate.allowed_ip_ranges {
            ips.iter()
                .try_for_each(|v| self.validate_ipv4(v).map(|_| ()))?;
        }

        // pub expiration_days: Option<u16>,
        // No validation necessary: if it deserializes to u16, it's fine

        // pub orgs_permission: Option<GranularTokenOrgsPermission>,
        // No validation necessary: if it deserializes to the enum, it's fine

        // pub packages_and_scopes_permission: Option<GranularTokenPackagesAndScopesPermission>,
        // No validation necessary: if it deserializes to the enum, it's fine

        // pub selected_orgs: Option<Vec<String>>,
        if let Some(ref orgs) = value_to_validate.selected_orgs {
            orgs.iter()
                .try_for_each(|v| self.validate_npm_org_name(v).map(|_| ()))?;
        }

        // pub selected_packages: Option<Vec<String>>,
        if let Some(ref pkgs) = value_to_validate.selected_packages {
            pkgs.iter()
                .try_for_each(|v| self.validate_npm_scoped_package(v).map(|_| ()))?;
        }

        // pub selected_packages_and_scopes: Option<GranularTokenSelectedPackagesAndScopes>,
        // No validation necessary: if it deserializes to the enum, it's fine

        // pub selected_scopes: Option<Vec<String>>,
        if let Some(ref scopes) = value_to_validate.selected_scopes {
            scopes
                .iter()
                .try_for_each(|v| self.validate_npm_at_org_name(v).map(|_| ()))?;
        }

        // pub token_name: String,
        self.validate_npm_token_name(&value_to_validate.token_name)?;

        // pub token_description: String,
        // Impose it's at least 8 characters to avoid things like "test" or "testing"
        if value_to_validate.token_description.len() < 8 {
            return Err(ApiError::NpmError(NpmError::InvalidInput(
                value_to_validate.token_description.clone(),
            )));
        }

        Ok(value_to_validate)
    }
}
