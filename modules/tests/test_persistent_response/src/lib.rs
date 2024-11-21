use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};

entrypoint_with_source_and_response!();

fn main(log: String, _source: LogSource) -> Result<Option<String>, i32> {
    plaid::print_debug_string(&format!("Testing Persistent Response With Log: [{log}]"));

    let query_data = plaid::get_accessory_data_by_name("querydata").unwrap();

    let webpage = include_str!("../resources/index.html")
        .to_string()
        .replace("{{data}}", &query_data);

    Ok(Some(webpage))
}
