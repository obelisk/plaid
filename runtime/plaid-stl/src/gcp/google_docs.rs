use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

#[derive(Serialize, Deserialize)]
pub struct CreateDocFromMarkdownInput {
    pub folder_id: String,
    pub title: String,
    pub template: String,
    pub variables: Value,
}

#[derive(Serialize, Deserialize)]
pub struct CreateDocFromMarkdownOutput {
    pub document_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct CreateSheetFromCsvInput {
    pub folder_id: String,
    pub title: String,
    pub template: String,
    pub variables: Value,
}

#[derive(Serialize, Deserialize)]
pub struct CreateSheetFromCsvOutput {
    pub document_id: String,
}

/// Create google doc from markdown template
pub fn create_doc_from_markdown(
    input: CreateDocFromMarkdownInput,
) -> Result<CreateDocFromMarkdownOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(gcp_google_docs, create_doc_from_markdown);
    }

    let input = serde_json::to_string(&input).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        gcp_google_docs_create_doc_from_markdown(
            input.as_ptr(),
            input.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    serde_json::from_slice::<CreateDocFromMarkdownOutput>(&return_buffer)
        .map_err(|_| PlaidFunctionError::InternalApiError)
}

/// Create google sheet from csv template
pub fn create_sheet_from_csv(
    input: CreateDocFromMarkdownInput,
) -> Result<CreateDocFromMarkdownOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(gcp_google_docs, create_sheet_from_csv);
    }

    let input = serde_json::to_string(&input).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        gcp_google_docs_create_sheet_from_csv(
            input.as_ptr(),
            input.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    serde_json::from_slice::<CreateDocFromMarkdownOutput>(&return_buffer)
        .map_err(|_| PlaidFunctionError::InternalApiError)
}
