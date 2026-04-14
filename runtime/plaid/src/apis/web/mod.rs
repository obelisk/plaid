use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
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
    UnsupportedKeyType(String),
    FailedToParsePrivateKey(String),
    EnforcedClaimsConflict(String),
}

/// Which signing algorithm/key type a configured key uses.
///
/// Minimal support for now:
/// - `ES256` (ECDSA P-256) using an EC private key in PEM format
/// - `RS256` (RSA) using an RSA private key in PEM format
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JwtKeyType {
    /// ECDSA using P-256 + SHA-256 (ES256)
    Es256,
    /// RSA + SHA-256 (RS256)
    Rs256,
}

impl JwtKeyType {
    fn algorithm(self) -> Algorithm {
        match self {
            JwtKeyType::Es256 => Algorithm::ES256,
            JwtKeyType::Rs256 => Algorithm::RS256,
        }
    }

    fn parse_encoding_key(self, pem: &str) -> Result<EncodingKey, WebError> {
        let bytes = pem.as_bytes();
        match self {
            JwtKeyType::Es256 => EncodingKey::from_ec_pem(bytes).map_err(|e| {
                WebError::FailedToParsePrivateKey(format!("Failed to parse ES256 EC key: {e}"))
            }),
            JwtKeyType::Rs256 => EncodingKey::from_rsa_pem(bytes).map_err(|e| {
                WebError::FailedToParsePrivateKey(format!("Failed to parse RS256 RSA key: {e}"))
            }),
        }
    }
}

/// Claims that must not be set via `enforced_claims` because they are already
/// handled by dedicated config knobs or internal logic (and have security semantics).
const RESERVED_ENFORCED_CLAIMS: [&str; 3] = ["sub", "iat", "exp"];

#[derive(Deserialize)]
struct JwtConfigRaw {
    /// The private key in PEM format. Interpretation depends on `key_type`.
    private_key: String,
    /// The signing key type / JWT `alg`.
    ///
    /// Supported values: `es256`, `rs256`.
    key_type: JwtKeyType,

    /// Which rules are allowed to use the key
    allowed_rules: Vec<String>,
    /// If this is true, then `iat` is set to the current time,
    /// regardless of what is passed in the request
    enforce_accurate_iat: Option<bool>,
    /// If this is set, then all JWTs signed with this key must
    /// have a validity <= this value.
    max_ttl: Option<u64>,
    /// Headers that can be accepted from a requestor and included in a JWT.
    /// If this is missing, then no extra headers are accepted.
    /// Only these values are accepted: cty, jku, x5u, x5t, x5t_s256.
    /// Other values will be ignored.
    /// Note - Be careful when configuring these headers, as including a crucial,
    /// untrusted header in a JWT can impact the security of downstream systems.
    allowlisted_extra_headers: Option<Vec<String>>,
    /// Fields that can be accepted from a requestor and included in a JWT.
    /// If this is missing, then no extra fields are accepted.
    /// Note - Be careful when configuring these fields, as including a crucial,
    /// untrusted field in a claim can impact the security of downstream systems.
    allowlisted_extra_fields: Option<Vec<String>>,
    /// Generic enforced claims to always include in the JWT for this key.
    ///
    /// These are config-controlled, so they are NOT subject to `allowlisted_extra_fields`.
    /// NOTE: Reserved claims ("sub", "iat", "exp") are rejected here to avoid
    /// conflicting with dedicated enforcement logic.
    #[serde(default)]
    enforced_claims: HashMap<String, Value>,
}

/// Public runtime configuration for a JWT signing key.
pub struct JwtConfig {
    /// The signing key type / JWT `alg`.
    pub key_type: JwtKeyType,
    /// The signing key parsed from PEM.
    pub private_key: EncodingKey,
    /// Which rules are allowed to use the key
    pub allowed_rules: Vec<String>,
    /// If this is true, then `iat` is set to the current time,
    /// regardless of what is passed in the request
    pub enforce_accurate_iat: Option<bool>,
    /// If this is set, then all JWTs signed with this key must
    /// have a validity <= this value.
    pub max_ttl: Option<u64>,
    /// Headers that can be accepted from a requestor and included in a JWT.
    /// If this is missing, then no extra headers are accepted.
    /// Only these values are accepted: cty, jku, x5u, x5t, x5t_s256.
    /// Other values will be ignored.
    /// Note - Be careful when configuring these headers, as including a crucial,
    /// untrusted header in a JWT can impact the security of downstream systems.
    allowlisted_extra_headers: Option<Vec<String>>,
    /// Fields that can be accepted from a requestor and included in a JWT.
    /// If this is missing, then no extra fields are accepted.
    /// Note - Be careful when configuring these fields, as including a crucial,
    /// untrusted field in a claim can impact the security of downstream systems.
    allowlisted_extra_fields: Option<Vec<String>>,
    /// See `JwtConfigRaw::enforced_claims`.
    enforced_claims: HashMap<String, Value>,
}

impl std::fmt::Debug for JwtConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtConfig")
            .field("key_type", &self.key_type)
            .field("allowed_rules", &self.allowed_rules)
            .field("enforce_accurate_iat", &self.enforce_accurate_iat)
            .field("max_ttl", &self.max_ttl)
            .field("allowlisted_extra_headers", &self.allowlisted_extra_headers)
            .field("allowlisted_extra_fields", &self.allowlisted_extra_fields)
            .field(
                "enforced_claims",
                &self
                    .enforced_claims
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            // safe (no key material)
            .finish()
    }
}

impl<'de> Deserialize<'de> for JwtConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = JwtConfigRaw::deserialize(deserializer)?;

        // Validate enforced_claims don't conflict with reserved claims.
        if !raw.enforced_claims.is_empty() {
            for k in raw.enforced_claims.keys() {
                if RESERVED_ENFORCED_CLAIMS.contains(&k.as_str()) {
                    return Err(serde::de::Error::custom(format!(
                        "enforced_claims may not include reserved claim [{k}]"
                    )));
                }
            }
        }

        let encoding_key = raw
            .key_type
            .parse_encoding_key(&raw.private_key)
            // serde::de::Error::custom requires Display; pass a String instead.
            .map_err(|e| serde::de::Error::custom(format!("{e:?}")))?;

        Ok(JwtConfig {
            key_type: raw.key_type,
            private_key: encoding_key,
            allowed_rules: raw.allowed_rules,
            enforce_accurate_iat: raw.enforce_accurate_iat,
            max_ttl: raw.max_ttl,
            allowlisted_extra_headers: raw.allowlisted_extra_headers,
            allowlisted_extra_fields: raw.allowlisted_extra_fields,
            enforced_claims: raw.enforced_claims,
        })
    }
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

fn get_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// The headers that can be allowlisted to be taken from the passed request.
const ALLOWLISTABLE_HEADERS: [&str; 5] = ["cty", "jku", "x5u", "x5t", "x5t_s256"];

/// Sanitize the configuration before usage
fn sanitize_config(config: WebConfig) -> WebConfig {
    let keys: HashMap<String, JwtConfig> = config
        .keys
        .into_iter()
        .map(|(kid, config)| {
            match &config.allowlisted_extra_headers {
                None => {
                    // In this case no processing is needed
                    (kid, config)
                }
                Some(configured_headers) => {
                    // Keep only the headers that can be allowlisted
                    let accepted_headers: Vec<_> = configured_headers
                        .iter()
                        .filter(|v| ALLOWLISTABLE_HEADERS.contains(&v.as_str()))
                        .cloned()
                        .collect();
                    let mut new_config = config;
                    new_config.allowlisted_extra_headers = Some(accepted_headers);
                    (kid, new_config)
                }
            }
        })
        .collect();
    WebConfig { keys }
}

impl Web {
    pub fn new(config: WebConfig) -> Self {
        Self {
            config: sanitize_config(config),
        }
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

        let mut claims = HashMap::<String, Value>::new();
        claims.insert("sub".to_string(), Value::String(request.sub.clone()));

        // iat (optional)
        // If the key is enforcing an accurate iat, then we set it to now,
        // otherwise we take whatever value is passed in the request (if any).
        if key_specs.enforce_accurate_iat.unwrap_or(false) {
            claims.insert("iat".to_string(), Value::Number(get_time().into()));
        } else if let Some(iat) = request.iat {
                claims.insert("iat".to_string(), Value::Number(iat.into()));
        }

        // aud (optional)
        if let Some(aud) = &request.aud {
            if aud.trim().is_empty() {
                return Err(ApiError::WebError(WebError::UnsupportedField(
                    "aud cannot be empty".to_string(),
                )));
            }
            claims.insert("aud".to_string(), Value::String(aud.clone()));
        }

        // exp (mandatory)
        // If the key enforces a max TTL, then the request may or may not pass an exp: if it does,
        // we take the minimum between that and now+TTL.
        // If the key does not enforce a max TTL, then the request must pass an exp, because we
        // still want to enforce all JWTs have an exp.
        let exp =
            {
                if let Some(max_ttl) = key_specs.max_ttl {
                    match request.exp {
                        None => get_time() + max_ttl,
                        Some(t) => std::cmp::min(t, get_time() + max_ttl),
                    }
                } else {
                    match request.exp {
                        None => return Err(ApiError::WebError(WebError::UnsupportedField(
                            "The key does not enforce a max TTL, so an exp field must be passed"
                                .to_string(),
                        ))),
                        Some(t) => t,
                    }
                }
            };
        claims.insert("exp".to_string(), Value::Number(exp.into()));


        // Include extra fields, but only if they are allowlisted in the config
        if !request.extra_fields.is_empty() {
            match &key_specs.allowlisted_extra_fields {
                Some(allowlisted_extras) => {
                    // We have an allowlist: see if the requested fields can be included
                    for (k, v) in request.extra_fields.iter() {
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
                }
                None => {
                    // The allowlist is empty
                    return Err(ApiError::WebError(WebError::UnsupportedField(
                        "The request contains extra fields but none is allowed".to_string(),
                    )));
                }
            }
        }

        // Apply config-enforced claims (generic, not subject to allowlist).
        // If the requester set the same claim to a different value, error out.
        if !key_specs.enforced_claims.is_empty() {
            for (k, v) in key_specs.enforced_claims.iter() {
                if let Some(existing) = claims.get(k) {
                    if existing != v {
                        return Err(ApiError::WebError(WebError::EnforcedClaimsConflict(format!(
                            "Claim [{k}] is enforced by config and cannot be set to a different value"
                        ))));
                    }
                }
                claims.insert(k.clone(), v.clone());
            }
        }

        // Build the header for the JWT
        // The header fields `alg`, `kid`, and `typ` are not configurable.
        // `typ` is set internally to "JWT" when calling Header::default()
        let mut header = Header::default();
        header.alg = key_specs.key_type.algorithm();
        header.kid = Some(kid.to_string());

        // Include extra headers, but only if they are allowlisted in the config
        if !request.extra_headers.is_empty() {
            match &key_specs.allowlisted_extra_headers {
                Some(allowlisted_extras) => {
                    // We have an allowlist: see if the requested headers can be included
                    for (k, v) in request.extra_headers.iter() {
                        if allowlisted_extras.contains(k) {
                            add_header(
                                &mut header,
                                k.to_string(),
                                v.as_str()
                                    .ok_or(ApiError::WebError(WebError::UnsupportedHeaderField(
                                        format!(
                                            "Could not parse value for header [{k}] as a string"
                                        ),
                                    )))?
                                    .to_string(),
                            );
                        } else {
                            // We found a header that is not allowed: stop immediately instead of dropping
                            // it silently, which could be confusing for the requester.
                            return Err(ApiError::WebError(WebError::UnsupportedHeaderField(
                                format!("The request contained header [{k}] but it is not allowed"),
                            )));
                        }
                    }
                }
                None => {
                    // The allowlist is empty
                    return Err(ApiError::WebError(WebError::UnsupportedHeaderField(
                        "The request contains extra headers but none is allowed".to_string(),
                    )));
                }
            }
        }

        // Encode the JWT token
        let jwt = match encode(&header, &claims, &key_specs.private_key) {
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

/// Add a header to a Header object
fn add_header(headers: &mut Header, k: String, v: String) {
    match k.as_str() {
        "cty" => {
            headers.cty = Some(v);
        }
        "jku" => {
            headers.jku = Some(v);
        }
        "x5u" => {
            headers.x5u = Some(v);
        }
        "x5t" => {
            headers.x5t = Some(v);
        }
        "x5t_s256" => {
            headers.x5t_s256 = Some(v);
        }
        _ => unreachable!(),
    }
}
