use std::collections::HashMap;

use serde::Serialize;

use crate::PlaidFunctionError;

pub fn post_log<T>(hec_name: &str, log: T) -> Result<(), PlaidFunctionError>
where
    T: Serialize,
{
    extern "C" {
        new_host_function!(splunk, post_hec);
    }

    let data = serde_json::to_string(&log).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("hec_name", hec_name.to_string());
    params.insert("log", data);

    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { splunk_post_hec(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
