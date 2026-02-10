use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

/// Input for create_folder operation
#[derive(Serialize, Deserialize)]
pub struct CreateFolderInput {
    /// The Google Drive folder (parent folder) where to place the new folder
    pub parent_id: String,
    /// Name of the new folder
    pub name: String,
}

/// Output for create_folder operation
#[derive(Serialize, Deserialize)]
pub struct CreateFolderOutput {
    /// The ID of the newly created folder
    pub folder_id: String,
}

/// Input for copy_file operation
#[derive(Serialize, Deserialize)]
pub struct CopyFileInput {
    /// The Google Drive folder (parent folder) where to place the new file
    pub parent_id: String,
    /// The ID of the file to copy
    pub file_id: String,
    /// The name of the copied file
    pub name: String,
}

/// Outoupt for copy_file operation
#[derive(Serialize, Deserialize)]
pub struct CopyFileOutput {
    /// The document ID of the copied document
    pub document_id: String,
}

/// Input for upload_file operation
#[derive(Serialize, Deserialize)]
pub struct UploadFileInput {
    /// The Google Drive folder (parent folder) where to place the file
    pub parent_id: String,
    /// The name of the uploaded file
    pub name: String,
    /// The content of the uploaded file
    pub content: String,
    /// The source mime type of the file
    pub source_mime: String,
    /// The target mime type of the file (for conversion)
    pub target_mime: String,
}

/// Output for upload_file operation
#[derive(Serialize, Deserialize)]
pub struct UploadFileOutput {
    /// The document ID of the copied document
    pub document_id: String,
}

/// Input for create_doc_from_markdown operation
#[derive(Serialize, Deserialize)]
pub struct CreateDocFromMarkdownInput {
    /// The Google Drive folder (parent folder) where to place the document
    pub parent_id: String,
    /// Name of the document
    pub name: String,
    /// Markdown content of the document.
    /// CommonMark markdown specification - https://commonmark.org/
    pub content: String,
}

/// Output for create_doc_from_markdown operation
#[derive(Serialize, Deserialize)]
pub struct CreateDocFromMarkdownOutput {
    /// The document ID of the newly created document
    /// View at https://docs.google.com/document/d/<DOCUMENT_ID>
    pub document_id: String,
}

/// Input for create_sheet_from_csv operation
#[derive(Serialize, Deserialize)]
pub struct CreateSheetFromCsvInput {
    /// The Google Drive folder (parent folder) where to place the spreadsheet
    pub parent_id: String,
    /// Name of the spreadsheet
    pub name: String,
    /// CSV content of the spreadsheet
    pub content: String,
}

/// Output for create_sheet_from_csv operation
#[derive(Serialize, Deserialize)]
pub struct CreateSheetFromCsvOutput {
    /// The document ID of the newly created spreadsheet
    /// View at https://docs.google.com/spreadsheets/d/<DOCUMENT_ID>
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

/// Create google spreadsheet from csv template
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
