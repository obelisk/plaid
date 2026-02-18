use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source,
    messages::LogSource,
    network::make_named_request,
    plaid,
    web::{issue_jwt, JwtParams},
};

use jsonwebtoken::errors::Error as JwtError;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde_json::Value;

entrypoint_with_source!();

#[derive(Debug)]
pub enum ValidateJwtError {
    Jwt(JwtError),
    InvalidAlgorithm,
    InvalidToken,
    InvalidSecret(String),
}

impl From<JwtError> for ValidateJwtError {
    fn from(err: JwtError) -> Self {
        ValidateJwtError::Jwt(err)
    }
}

impl From<String> for ValidateJwtError {
    fn from(err: String) -> Self {
        ValidateJwtError::InvalidSecret(err)
    }
}

const ES256_KEY_ID: &str = "46c642b0da02030407c6463c013a8dbd";
const RS256_KEY_ID: &str = "b27b3d7279c0c6244e7e25e0f10ea4aa";

fn main(_: String, _: LogSource) -> Result<(), i32> {
    // RSA key expected enforced_claims
    let enforced_claims: HashMap<String, Value> = [
        ("aud".to_string(), Value::String("audience".to_string())),
        ("iss".to_string(), Value::String("issuer".to_string())),
    ]
    .into_iter()
    .collect();

    // Simple request - ecdsa key
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if let Ok(jwt) = issue_jwt(&jwt_params) {
        if validate_jwt(&jwt_params, &jwt, None).is_ok() {
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
    }

    // Simple request - rsa key
    let jwt_params = JwtParams {
        kid: RS256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if let Ok(jwt) = issue_jwt(&jwt_params) {
        if validate_jwt(&jwt_params, &jwt, Some(&enforced_claims)).is_ok() {
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
    }

    // Add a field which is allowlisted - ecdsa key
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: [("ext".to_string(), Value::String("v".to_string()))].into(),
    };
    if let Ok(jwt) = issue_jwt(&jwt_params) {
        if validate_jwt(&jwt_params, &jwt, None).is_ok() {
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
    }

    // Set aud field/claim to ecdsa key
    // Should pass as enforced_claims does not include "aud" for ECDSA key
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: Some("Something".to_string()),
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: [("iss".to_string(), Value::String("issuer".to_string()))].into(),
    };
    if let Ok(jwt) = issue_jwt(&jwt_params) {
        if validate_jwt(&jwt_params, &jwt, None).is_ok() {
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
    }

    // Set aud field/claim to rsa key
    // Should pass as value is the same as the enforced aud in the config for this key
    let jwt_params = JwtParams {
        kid: RS256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: Some("audience".to_string()),
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: [("iss".to_string(), Value::String("issuer".to_string()))].into(),
    };
    if let Ok(jwt) = issue_jwt(&jwt_params) {
        if validate_jwt(&jwt_params, &jwt, Some(&enforced_claims)).is_ok() {
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
    }

    // Set aud field/claim to rsa key - should fail
    // Should fail as enforced_claims includes "aud" for RSA key
    let jwt_params = JwtParams {
        kid: RS256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: Some("Something".to_string()),
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: [("iss".to_string(), Value::String("issuer".to_string()))].into(),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Add a field which is NOT allowlisted - should fail
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: [
            ("ext".to_string(), Value::String("v".to_string())),
            ("hck".to_string(), Value::String("x".to_string())),
        ]
        .into(),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Add a header which is allowlisted - ecdsa key
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: [("cty".to_string(), Value::String("something".to_string()))].into(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if let Ok(jwt) = issue_jwt(&jwt_params) {
        if validate_jwt(&jwt_params, &jwt, None).is_ok() {
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
    }

    // Add a header which is NOT allowlisted - should fail
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: [("smt".to_string(), Value::String("something".to_string()))].into(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Use a key ID that we are not allowed to use - should fail
    let jwt_params = JwtParams {
        kid: "230216b400a90b29f70db61aca97bbca".to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Use a key ID that does not exist - should fail
    let jwt_params = JwtParams {
        kid: "DOES_NOT_EXIST".to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Make a request without exp. Since the key does not enforce a max TTL, this will fail
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: None,
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    Ok(())
}

pub fn validate_jwt(
    jwt_params: &JwtParams,
    jwt: &str,
    enforced_claims: Option<&HashMap<String, Value>>,
) -> Result<Value, ValidateJwtError> {
    // ---- Normal header + signature validation setup ----
    let header = decode_header(jwt)?;
    let alg = header.alg;

    match alg {
        Algorithm::RS256 | Algorithm::ES256 => {}
        _ => {
            return Err(ValidateJwtError::InvalidAlgorithm);
        }
    }

    let key_name = format!("pub_{}", jwt_params.kid);
    let public_key_pem = plaid::get_secrets(key_name.as_str())
        .map_err(|e| ValidateJwtError::InvalidSecret(e.to_string()))?;
    let decoding_key = match alg {
        Algorithm::RS256 => DecodingKey::from_rsa_pem(public_key_pem.as_bytes())?,
        Algorithm::ES256 => DecodingKey::from_ec_pem(public_key_pem.as_bytes())?,
        _ => unreachable!("guarded above"),
    };

    let mut validation = Validation::new(alg);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = 0;

    let token = decode::<Value>(jwt, &decoding_key, &validation)?;
    let claims = token.claims;

    // Build expected claims from JwtParams
    let mut expected = HashMap::<String, Value>::new();
    expected.insert("sub".to_string(), Value::String(jwt_params.sub.clone()));

    // Validate the claims that defined explicitly in JwtParams
    if let Some(iat) = jwt_params.iat {
        expected.insert("iat".to_string(), Value::Number(iat.into()));
    }
    if let Some(exp) = jwt_params.exp {
        expected.insert("exp".to_string(), Value::Number(exp.into()));
    }

    // aud is an edge-case due to enforced_claims functionality in the JwtConfig
    // - if aud is set in JwtParams, and set in enforced_claims then it must match.
    // - if aud isn't set in JwtParams, then an aud from enforced_claims will be validated below.
    if let Some(aud) = &jwt_params.aud {
        expected.insert("aud".to_string(), Value::String(aud.clone()));
    }

    for (k, v) in &jwt_params.extra_fields {
        expected.insert(k.clone(), v.clone());
    }

    // Overlay enforced claims (enforced wins/overrides)
    if let Some(enforced) = enforced_claims {
        for (k, v) in enforced {
            expected.insert(k.clone(), v.clone());
        }
    }

    // Verify expected claims exist and match exactly
    for (k, expected_v) in &expected {
        match claims.get(k) {
            Some(actual_v) if actual_v == expected_v => {}
            _ => return Err(ValidateJwtError::InvalidToken),
        }
    }

    Ok(claims)
}
