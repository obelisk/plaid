use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source,
    messages::LogSource,
    network::make_named_request,
    web::{issue_jwt, JwtParams},
};
use serde_json::Value;

entrypoint_with_source!();

const KEY_ID: &str = "46c642b0da02030407c6463c013a8dbd";

fn main(_: String, _: LogSource) -> Result<(), i32> {
    // Simple request
    let jwt_params = JwtParams {
        kid: KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: plaid_stl::plaid::get_time() as u64,
        exp: plaid_stl::plaid::get_time() as u64 + 3600,
        extra_fields: None,
    };
    if issue_jwt(&jwt_params).is_ok() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Add a field which is allowlisted
    let jwt_params = JwtParams {
        kid: KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: plaid_stl::plaid::get_time() as u64,
        exp: plaid_stl::plaid::get_time() as u64 + 3600,
        extra_fields: Some([("ext".to_string(), Value::String("v".to_string()))].into()),
    };
    if issue_jwt(&jwt_params).is_ok() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Add a field which is NOT allowlisted - should fail
    let jwt_params = JwtParams {
        kid: KEY_ID.to_string(),
        sub: "Something".to_string(),
        iat: plaid_stl::plaid::get_time() as u64,
        exp: plaid_stl::plaid::get_time() as u64 + 3600,
        extra_fields: Some(
            [
                ("ext".to_string(), Value::String("v".to_string())),
                ("hck".to_string(), Value::String("x".to_string())),
            ]
            .into(),
        ),
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Use a key ID that we are not allowed to use - should fail
    let jwt_params = JwtParams {
        kid: "230216b400a90b29f70db61aca97bbca".to_string(),
        sub: "Something".to_string(),
        iat: plaid_stl::plaid::get_time() as u64,
        exp: plaid_stl::plaid::get_time() as u64 + 3600,
        extra_fields: None,
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Use a key ID that does not exist - should fail
    let jwt_params = JwtParams {
        kid: "DOES_NOT_EXIST".to_string(),
        sub: "Something".to_string(),
        iat: plaid_stl::plaid::get_time() as u64,
        exp: plaid_stl::plaid::get_time() as u64 + 3600,
        extra_fields: None,
    };
    if issue_jwt(&jwt_params).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    Ok(())
}
