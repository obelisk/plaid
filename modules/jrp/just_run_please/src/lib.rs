use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};

entrypoint_with_source!();

fn main(data: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!(
        "I just ran please and thank you. Here's the data: {data}"
    ));

    Ok(())
}
