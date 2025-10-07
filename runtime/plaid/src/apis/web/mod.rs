use jsonwebtoken::{encode, EncodingKey, Header};
use plaid_stl::web::JwtParams;
use serde::Deserialize;
use serde_json::Value;

use std::{collections::HashMap, sync::Arc};

use crate::loader::PlaidModule;

use super::ApiError;

#[derive(Debug)]
pub enum WebError {
    ModuleUnauthorizedToUseKey(String),
    FailedToEncodeJwt(String),
    UnsupportedHeaderField(String),
    BadRequest(String),
    UnsupportedField(String),
}

#[derive(Deserialize)]
pub struct JwtConfig {
    /// The ECDSA256 private key in PEM format
    #[serde(deserialize_with = "jwt_private_key_deserializer")]
    pub private_key: EncodingKey,
    /// Which rules are allowed to use the key
    pub allowed_rules: Vec<String>,
}

#[derive(Deserialize)]
pub struct WebConfig {
    /// This contains a mapping of available keys that can be used
    /// to sign JWTs
    keys: HashMap<String, JwtConfig>,
    /// Fields that can be accepted from a requestor and included in a JWT.
    /// If this is missing, then no extra fields are accepted.
    /// Note - Be careful when configuring these fields, as including a crucial,
    /// untrusted field in a claim can impact the security of downstream systems.
    allowlisted_extra_fields: Option<Vec<String>>,
}

pub struct Web {
    config: WebConfig,
}

/// Deserialize a string to an `EncodingKey` or return an error
fn jwt_private_key_deserializer<'de, D>(deserializer: D) -> Result<EncodingKey, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw_key: String = Deserialize::deserialize(deserializer)?;

    EncodingKey::from_ec_pem(raw_key.as_bytes()).map_err(serde::de::Error::custom)
}

impl Web {
    pub fn new(config: WebConfig) -> Self {
        Self { config }
    }

    /// Create, sign, and encode a new JWT with the contents specified in `params`.
    /// This fails if `params` contains invalid values or if `module` is not allowed to
    /// use the key specified in `params`.
    pub async fn issue_jwt(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: JwtParams = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Get the key ID from the request params
        let kid = &request.kid;

        // Fetch the key object from the Plaid config
        let key_specs =
            self.config
                .keys
                .get(kid)
                .ok_or(ApiError::WebError(WebError::BadRequest(format!(
                    "Module [{module}] tried to use [{kid}] that does not exist"
                ))))?;

        // Check that the module is allowed to use this kid
        if !key_specs.allowed_rules.contains(&module.to_string()) {
            return Err(ApiError::WebError(WebError::ModuleUnauthorizedToUseKey(
                format!("Module [{module}] is not authorized to use key [{kid}]"),
            )));
        }

        // TODO Discuss with @obelisk if we want to validate fields like 'iat' and 'exp'

        let mut claims = HashMap::<String, Value>::new();
        claims.insert("sub".to_string(), Value::String(request.sub.clone()));
        claims.insert("iat".to_string(), Value::Number(request.iat.into()));
        claims.insert("exp".to_string(), Value::Number(request.exp.into()));

        // Include extra fields, but only if they are allowlisted in the config
        if let Some(extra_fields) = request.extra_fields {
            if let Some(allowlisted_extras) = &self.config.allowlisted_extra_fields {
                for (k, v) in extra_fields.iter() {
                    if allowlisted_extras.contains(k) {
                        claims.insert(k.to_string(), v.clone());
                    } else {
                        // We found a field that is not allowed: stop immediately instead of dropping
                        // it silently, which could be confusing for the requester.
                        return Err(ApiError::WebError(WebError::UnsupportedField(format!(
                            "The request contained field [{k}] but it is not allowed"
                        ))));
                    }
                }
            } else {
                // The allowlist is empty
                return Err(ApiError::WebError(WebError::UnsupportedField(
                    "The request contains extra fields but none is allowed".to_string(),
                )));
            }
        }

        // Build the header for the JWT
        // The header fields `alg`, `kid`, and `typ` are not configurable.
        // `typ` is set internally to "JWT" when calling Header::default()
        let mut header = Header::default();
        header.alg = jsonwebtoken::Algorithm::ES256;
        header.kid = Some(kid.to_string());

        // Encode the JWT token
        let jwt = match encode(
            &header,
            // The remaining fields in the request are put in the Claim
            &claims,
            &key_specs.private_key,
        ) {
            Ok(token) => token,
            Err(e) => {
                return Err(ApiError::WebError(WebError::FailedToEncodeJwt(
                    e.to_string(),
                )))
            }
        };

        Ok(jwt)
    }
}
