use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};

entrypoint_with_source!();

fn main(data: String, source: LogSource) -> Result<(), i32> {
    let source_type = match &source {
        LogSource::WebhookPost(_) => "webhook POST",
        LogSource::WebhookGet(_) => "webhook GET",
        LogSource::Logback(_) => "logback",
        _ => "unknown",
    };

    plaid::print_debug_string(&format!("[hello-world] source: {source_type}"));
    plaid::print_debug_string(&format!("[hello-world] payload: {data}"));

    // Parse as JSON if possible, otherwise treat as plain text
    match serde_json::from_str::<serde_json::Value>(&data) {
        Ok(json) => {
            plaid::print_debug_string(&format!(
                "[hello-world] parsed JSON with {} keys",
                json.as_object().map(|o| o.len()).unwrap_or(0)
            ));
        }
        Err(_) => {
            plaid::print_debug_string(&format!(
                "[hello-world] plain text ({} bytes)",
                data.len()
            ));
        }
    }

    Ok(())
}
