use serde_json::{json, to_value, Value};
use std::collections::HashMap;

use base64::{prelude::BASE64_STANDARD, Engine};
use plaid_stl::{
    entrypoint_with_source,
    gcp::google_docs::{create_doc_from_markdown, CreateDocFromMarkdownInput},
    messages::LogSource,
    network::make_named_request,
    plaid,
};

entrypoint_with_source!();

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing google_docs With Log: [{log}]"));
    let table_name = String::from("local_test");

    // expect to fail
    let input = CreateDocFromMarkdownInput {
        folder_id: String::from("hello"),
        title: String::from("hello"),
        template: String::from("hello"),
        variables: json!({}),
    };
    let out = create_doc_from_markdown(input);

    // If we are here, then everything worked fine (no unwraps or early returns), so we send an OK
    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
