use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};

entrypoint_with_source!();


fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing crashtest With Log: [{log}]"));


    let mut pressure = vec![];
    let mut counter = 0;

    match log.as_str() {
        "crash" => {
            let _b = &pressure[1];
        },
        "memory_pressure" => {
            loop {
                pressure.push("String allocation".to_string());
                if counter % 50_000 == 0 {
                    plaid::print_debug_string(&format!("Counter: {counter}"));
                }
                counter += 1;
            }
        }
        x => {
            plaid::print_debug_string(&format!("Unknown crash type test: {x}"));
        }
    }

    Ok(())

}
