use jsonwebtoken::{encode, EncodingKey, Header};
use serde::Deserialize;

use std::collections::HashMap;

use super::ApiError;

#[derive(Debug)]
pub enum WebError {
    ModuleUnauthorizedToUseKey(String),
    FailedToEncodeJwt(String),
    UnsupportedHeaderField(String),
    BadRequest(String),
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
    pub async fn issue_jwt(&self, params: &str, module: &str) -> Result<String, ApiError> {
        let mut request: HashMap<&str, serde_json::Value> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Get the kid from the request params
        let kid = request
            .get("kid")
            .map(|x| x.clone())
            .ok_or(ApiError::MissingParameter("kid".to_owned()))?;
        // Remove this from request so that only keys for the Claim remains
        request.remove("kid");

        let kid = kid.as_str().ok_or(ApiError::BadRequest)?;

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

        // Build the header for the JWT
        // The header fields `alg`, `kid`, and `typ` are not configurable
        // The header field `x5u` is fetched from the request
        let mut header = Header::default();
        header.alg = jsonwebtoken::Algorithm::ES256;
        header.kid = Some(kid.to_string());

        if let Some(x5u) = request.get("x5u") {
            header.x5u = Some(x5u.to_string());
            // Remove this from request so that only keys for the Claim remains
            request.remove("x5u");
        };

        // The header field `x5c` is not supported
        if request.contains_key("x5c") {
            return Err(ApiError::WebError(WebError::UnsupportedHeaderField(
                format!("Module [{module}] tried to use unsupported header field x5c"),
            )));
        }

        // Encode the JWT token
        let jwt = match encode(
            &header,
            // The remaining fields in the request are put in the Claim
            &request,
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
