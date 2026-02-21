use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source,
    messages::LogSource,
    network::make_named_request,
    plaid,
    web::{issue_jwt, JwtParams},
};

use jwt_simple::{
    algorithms::{
        ES256PublicKey, RS256PublicKey,
        ECDSAP256PublicKeyLike, RSAPublicKeyLike,
    },
    claims::{Audiences, JWTClaims},
    token::Token,
};
use jwt_simple::prelude::{ VerificationOptions, Duration};
use serde_json::{Map as JsonMap, Value};

entrypoint_with_source!();

type JwtError = jwt_simple::Error;

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

fn jwt_claims_to_value_object(claims: JWTClaims<JsonMap<String, Value>>) -> Value {
    // custom claims are flattened in the JWT payload; jwt-simple stores them in `claims.custom` :contentReference[oaicite:3]{index=3}
    let mut obj = claims.custom;

    if let Some(sub) = claims.subject {
        obj.insert("sub".to_string(), Value::String(sub));
    }
    if let Some(iat) = claims.issued_at {
        obj.insert("iat".to_string(), Value::Number((iat.as_secs()).into()));
    }
    if let Some(exp) = claims.expires_at {
        obj.insert("exp".to_string(), Value::Number((exp.as_secs()).into()));
    }
    if let Some(nbf) = claims.invalid_before {
        obj.insert("nbf".to_string(), Value::Number((nbf.as_secs()).into()));
    }
    if let Some(iss) = claims.issuer {
        obj.insert("iss".to_string(), Value::String(iss));
    }
    if let Some(audiences) = claims.audiences {
        // Your current logic expects a single string value for aud.
        // If it ever becomes a set/array, this will turn into an array of strings.
        match audiences {
            Audiences::AsString(a) => {
                obj.insert("aud".to_string(), Value::String(a));
            }
            Audiences::AsSet(set) => {
                let mut v: Vec<Value> = set.into_iter().map(Value::String).collect();
                // stable ordering not required for your current expected-values (strings),
                // but sorting makes outputs deterministic if you ever log/debug.
                v.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                obj.insert("aud".to_string(), Value::Array(v));
            }
        }
    }

    Value::Object(obj)
}

pub fn validate_jwt(
    jwt_params: &JwtParams,
    jwt: &str,
    enforced_claims: Option<&HashMap<String, Value>>,
) -> Result<Value, ValidateJwtError> {
    // ---- Peek at header metadata (kid/alg/etc.) ----
    // NOTE: jwt-simple warns metadata is untrusted; we only use it the same way you did before:
    // to reject unexpected algorithms and pick the correct public-key parser. :contentReference[oaicite:4]{index=4}
    let metadata = Token::decode_metadata(jwt)?;
    let alg = metadata.algorithm();

    if alg != "RS256" && alg != "ES256" {
        return Err(ValidateJwtError::InvalidAlgorithm);
    }

    // ---- Load the public key PEM from secrets ----
    let key_name = format!("pub_{}", jwt_params.kid);
    let public_key_pem = plaid::get_secrets(key_name.as_str())
        .map_err(|e| ValidateJwtError::InvalidSecret(e.to_string()))?;

    // ---- Verify signature + standard JWT time claims ----
    // jwt-simple validates exp/nbf automatically; we set time_tolerance to 0 to match your `leeway = 0`. :contentReference[oaicite:5]{index=5}
    let mut options = VerificationOptions::default();
    options.time_tolerance = Some(Duration::from_secs(0));

    let verified_claims: JWTClaims<JsonMap<String, Value>> = match alg {
        "RS256" => {
            let pk = RS256PublicKey::from_pem(&public_key_pem)?;
            pk.verify_token::<JsonMap<String, Value>>(jwt, Some(options))?
        }
        "ES256" => {
            let pk = ES256PublicKey::from_pem(&public_key_pem)?;
            pk.verify_token::<JsonMap<String, Value>>(jwt, Some(options))?
        }
        _ => unreachable!("guarded above"),
    };

    let claims_value = jwt_claims_to_value_object(verified_claims);
    let claims_obj = claims_value
        .as_object()
        .ok_or(ValidateJwtError::InvalidToken)?;

    // ---- Build expected claims from JwtParams ----
    let mut expected = HashMap::<String, Value>::new();
    expected.insert("sub".to_string(), Value::String(jwt_params.sub.clone()));

    if let Some(iat) = jwt_params.iat {
        expected.insert("iat".to_string(), Value::Number(iat.into()));
    }
    if let Some(exp) = jwt_params.exp {
        expected.insert("exp".to_string(), Value::Number(exp.into()));
    }
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
        match claims_obj.get(k) {
            Some(actual_v) if actual_v == expected_v => {}
            _ => return Err(ValidateJwtError::InvalidToken),
        }
    }

    Ok(claims_value)
}