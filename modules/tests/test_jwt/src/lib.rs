use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source,
    messages::LogSource,
    network::make_named_request,
    web::{issue_jwt, JwtParams},
};
use serde_json::Value;

entrypoint_with_source!();

const ES256_KEY_ID: &str = "46c642b0da02030407c6463c013a8dbd";
const RS256_KEY_ID: &str = "b27b3d7279c0c6244e7e25e0f10ea4aa";

fn main(_: String, _: LogSource) -> Result<(), i32> {
    // Simple request - es256
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_ok() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Simple request - rs256
    let jwt_params = JwtParams {
        kid: RS256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_ok() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Add a field which is allowlisted - es256
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: [("ext".to_string(), Value::String("v".to_string()))].into(),
    };
    if issue_jwt(&jwt_params).is_ok() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Add aud, which is an enforced claim but doesn't match enforced value - should fail
    let jwt_params = JwtParams {
        kid: RS256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: Some("Something".to_string()),
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Add aud, which is an enforced claim but matches enforced value - should succeed
    let jwt_params = JwtParams {
        kid: RS256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: Some("audience".to_string()),
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_ok() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }


    // Add a field which is an enforced claim, and is not allowlisted - should fail
    let jwt_params = JwtParams {
        kid: RS256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: HashMap::<String, Value>::new(),
        extra_fields: [("iss".to_string(), Value::String("Something".to_string()))].into(),
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

    // Add a header which is allowlisted
    let jwt_params = JwtParams {
        kid: ES256_KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: Some(plaid_stl::plaid::get_time() as u64),
        exp: Some(plaid_stl::plaid::get_time() as u64 + 3600),
        aud: None::<String>,
        extra_headers: [("cty".to_string(), Value::String("something".to_string()))].into(),
        extra_fields: HashMap::<String, Value>::new(),
    };
    if issue_jwt(&jwt_params).is_ok() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
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
