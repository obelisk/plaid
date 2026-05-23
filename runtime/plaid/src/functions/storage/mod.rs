use super::{
    calculate_max_buffer_size, get_memory, safely_get_memory, safely_get_string,
    safely_write_data_back,
};

pub use delete::{delete, delete_shared};
pub use get::{get, get_shared};
pub use insert::{insert, insert_shared};
pub use list::{list_keys, list_keys_shared};

macro_rules! safely_get_guest_string {
    ($variable:ident, $memory_view:expr, $buf:expr, $buf_len:expr, $env_data:expr) => {
        let $variable = match safely_get_string(&$memory_view, $buf, $buf_len) {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "{}: error while getting a string from guest memory: {:?}",
                    $env_data.module.name, e
                );
                return FunctionErrors::ParametersNotUtf8 as i32;
            }
        };
    };
}

macro_rules! safely_get_guest_memory {
    ($variable:ident, $memory_view:expr, $buf:expr, $buf_len:expr, $env_data:expr) => {
        let max_buffer_size = calculate_max_buffer_size($env_data.module.page_limit);
        let $variable = match safely_get_memory(&$memory_view, $buf, $buf_len, max_buffer_size) {
            Ok(d) => d,
            Err(e) => {
                error!(
                    "{}: error while getting bytes from guest memory: {:?}",
                    $env_data.module.name, e
                );
                return FunctionErrors::ParametersNotUtf8 as i32;
            }
        };
    };
}

mod delete;
mod get;
mod insert;
mod list;
