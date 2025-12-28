use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use plaid_stl::gcp::google_docs::{
    CreateDocFromMarkdownInput, CreateDocFromMarkdownOutput, CreateSheetFromCsvInput,
};
use pulldown_cmark::{html, Options, Parser};
use reqwest::{multipart, Client};
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::apis::{AccessScope, ApiError};
use crate::loader::PlaidModule;

/// Defines configuration for the Google Docs API
#[derive(Deserialize)]
pub struct GoogleDocsConfig {
    /// Google OAuth Client ID
    client_id: String,
    /// Google OAuth Client Secret
    client_secret: String,
    /// Google OAuth refresh token, used to obtain valid OAuth Access Token
    refresh_token: String,
    /// Configured writers - maps a folder ID to a list of rules that are allowed to READ or WRITE files
    rw: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a folder ID to a list of rules that are allowed to READ files
    r: HashMap<String, HashSet<String>>,
}

/// Represents the Google Docs API client
pub struct GoogleDocs {
    /// Inner HTTP client used to make requests to Google APIs
    client: Client,
    /// Inner Config
    config: GoogleDocsConfig,
    /// Cached Google OAuth Access Token (Token, Expiry)
    access_token: Mutex<Option<(String, Instant)>>,
}

// Google OAuth Token URL
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
// Google Drive Multipart Upload URL
const DRIVE_UPLOAD_URL: &str =
    "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";

#[derive(Error, Debug)]
pub enum GoogleDocsError {
    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Missing field in response: {0}")]
    MissingField(&'static str),
    #[error("OAuth error: {description}")]
    OAuth { description: String },
    #[error("Google API error: {0}")]
    GoogleApi(String),
}

impl GoogleDocs {
    pub fn new(config: GoogleDocsConfig) -> Self {
        let client = Client::new();

        Self {
            client,
            config,
            access_token: Mutex::new(None),
        }
    }

    /// Checks if a module can perform a given action on a specific resource
    /// Modules are registered as as read (R) or write (RW) under self
    fn check_module_permissions(
        &self,
        access_scope: AccessScope,
        module: Arc<PlaidModule>,
        resource_id: &str,
    ) -> Result<(), ApiError> {
        match access_scope {
            AccessScope::Read => {
                // check if read access is configured for this folder
                if let Some(folder_readers) = self.config.r.get(resource_id) {
                    // check if this module has read access to this folder
                    if folder_readers.contains(&module.to_string()) {
                        return Ok(());
                    }
                }

                // check if write access is configured for this folder
                // writers can also read
                if let Some(folder_writers) = self.config.rw.get(resource_id) {
                    // check if this module has write access to this folder
                    if folder_writers.contains(&module.to_string()) {
                        return Ok(());
                    }
                }

                warn!(
                "[{module}] failed [read] permission check for google drive folder [{resource_id}]"
            );
                Err(ApiError::BadRequest)
            }
            AccessScope::Write => {
                // check if write access is configured for this folder
                if let Some(write_access) = self.config.rw.get(resource_id) {
                    // check if this module has write access to this folder
                    if write_access.contains(&module.to_string()) {
                        return Ok(());
                    };
                }

                warn!(
                "[{module}] failed [write] permission check for google drive folder [{resource_id}]"
            );
                Err(ApiError::BadRequest)
            }
        }
    }

    /// Returns a valid Access Token, reusing the cached one if valid, or refreshing it
    async fn refresh_access_token(&self) -> Result<String, GoogleDocsError> {
        let mut lock = self.access_token.lock().await;

        if let Some((token, expiry)) = &*lock {
            if Instant::now() < *expiry {
                return Ok(token.clone());
            }
        }

        let params = [
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
            ("refresh_token", self.config.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let response = self.client.post(TOKEN_URL).form(&params).send().await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(GoogleDocsError::OAuth {
                description: format!("Token refresh failed: {}", error_text),
            });
        }

        let json: Value = response.json().await?;

        let access_token = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or(GoogleDocsError::MissingField("access_token"))?;

        let expires_in = json
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);

        let expiry = Instant::now() + Duration::from_secs(expires_in.saturating_sub(60));
        *lock = Some((access_token.clone(), expiry));

        Ok(access_token)
    }

    /// Uploads a file to a Google Drive folder
    async fn upload_file(
        &self,
        folder_id: &str,
        title: &str,
        content: String,
        source_mime: &str,
        target_mime: &str,
    ) -> Result<String, GoogleDocsError> {
        let access_token = self.refresh_access_token().await?;

        let metadata = json!({
            "name": title,
            "mimeType": target_mime,
            "parents": [folder_id]
        });

        let metadata_part = multipart::Part::text(metadata.to_string())
            .mime_str("application/json; charset=UTF-8")?;

        let content_part = multipart::Part::text(content).mime_str(source_mime)?;

        let form = multipart::Form::new()
            .part("metadata", metadata_part)
            .part("media", content_part);

        let response = self
            .client
            .post(DRIVE_UPLOAD_URL)
            .bearer_auth(access_token)
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(GoogleDocsError::GoogleApi(format!(
                "Drive Upload failed: {}",
                error_text
            )));
        }

        let json: Value = response.json().await?;

        json.get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or(GoogleDocsError::MissingField("id"))
    }

    /// Create Google Doc from markdown content
    pub async fn create_doc_from_markdown(
        &self,
        input: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let CreateDocFromMarkdownInput {
            folder_id,
            title,
            content,
        } = serde_json::from_str(input).map_err(|err| ApiError::SerdeError(err.to_string()))?;

        // check this module has access to this folder
        self.check_module_permissions(AccessScope::Write, module, &folder_id)?;

        let html_output = markdown_to_html(&content);
        let document_id = self
            .upload_file(
                &folder_id,
                &title,
                html_output,
                "text/html",
                "application/vnd.google-apps.document",
            )
            .await
            .map_err(|err| ApiError::GoogleDocsError(err))?;

        let output = CreateDocFromMarkdownOutput { document_id };
        serde_json::to_string(&output).map_err(|err| ApiError::SerdeError(err.to_string()))
    }

    /// Create Google Sheet from csv content
    pub async fn create_sheet_from_csv(
        &self,
        input: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let CreateSheetFromCsvInput {
            folder_id,
            title,
            content,
        } = serde_json::from_str(input).map_err(|err| ApiError::SerdeError(err.to_string()))?;

        // check this module has access to this folder
        self.check_module_permissions(AccessScope::Write, module, &folder_id)?;

        let document_id = self
            .upload_file(
                &folder_id,
                &title,
                content,
                "text/csv",
                "application/vnd.google-apps.spreadsheet",
            )
            .await
            .map_err(|err| ApiError::GoogleDocsError(err))?;

        let output = CreateDocFromMarkdownOutput { document_id };
        serde_json::to_string(&output).map_err(|err| ApiError::SerdeError(err.to_string()))
    }
}

/// utility function for converting markdown into HTML
fn markdown_to_html(md: &str) -> String {
    // Convert Markdown to HTML
    // We enable tables and footnotes for better compatibility
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(md, options);
    let mut output = String::new();
    html::push_html(&mut output, parser);

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::CONTENT_TYPE;
    use serde_json::from_value;
    use serde_json::Value;
    use std::io::BufRead;
    use wasmer::{
        sys::{Cranelift, EngineBuilder},
        Module, Store,
    };

    use crate::loader::LimitValue;

    // helper function to generate a blank module that does nothing
    fn test_module(name: &str, test_mode: bool) -> Arc<PlaidModule> {
        let store = Store::default();
        // stub wasm module, just enough to pass validation
        // https://docs.rs/wabt/latest/wabt/fn.wat2wasm.html
        let wasm = &[
            0, 97, 115, 109, // \0ASM - magic
            1, 0, 0, 0, //  0x01 - version
        ];
        let compiler_config = Cranelift::default();
        let engine = EngineBuilder::new(compiler_config);
        let m = Module::new(&store, wasm).unwrap();

        Arc::new(PlaidModule {
            name: name.to_string(),
            logtype: "test".to_string(),
            module: m,
            engine: engine.into(),
            computation_limit: 0,
            page_limit: 0,
            storage_current: Default::default(),
            storage_limit: LimitValue::Unlimited,
            accessory_data: Default::default(),
            secrets: Default::default(),
            persistent_response: Default::default(),
            test_mode,
        })
    }

    #[tokio::test]
    async fn permission_checks() {
        let folder_name = String::from("local_test");
        // permissions
        let r = json!({folder_name.clone(): ["module_a"]});
        let r = from_value::<HashMap<String, HashSet<String>>>(r).unwrap();

        let rw = json!({folder_name.clone(): ["module_b"]});
        let rw = from_value::<HashMap<String, HashSet<String>>>(rw).unwrap();

        let config = GoogleDocsConfig {
            client_id: String::default(),
            client_secret: String::default(),
            refresh_token: String::default(),
            r,
            rw,
        };
        let client = GoogleDocs::new(config);

        // modules
        let module_a = test_module("module_a", true); // reader
        let module_b = test_module("module_b", true); // writer
        let module_c = test_module("module_c", true); // no access

        // modules can read folder

        client
            .check_module_permissions(AccessScope::Read, module_a.clone(), &folder_name)
            .unwrap();

        client
            .check_module_permissions(AccessScope::Read, module_b.clone(), &folder_name)
            .unwrap();

        client
            .check_module_permissions(AccessScope::Read, module_c.clone(), &folder_name)
            .expect_err("expect to fail with BadRequest");

        // readers can't write
        client
            .check_module_permissions(AccessScope::Write, module_a.clone(), &folder_name)
            .expect_err("expect to fail with BadRequest");

        client
            .check_module_permissions(AccessScope::Write, module_b.clone(), &folder_name)
            .unwrap();

        client
            .check_module_permissions(AccessScope::Write, module_c.clone(), &folder_name)
            .expect_err("expect to fail with BadRequest");

        // unknown folder
        client
            .check_module_permissions(AccessScope::Read, module_a.clone(), "unknown_folder")
            .expect_err("expect to fail with BadRequest");

        client
            .check_module_permissions(AccessScope::Read, module_b.clone(), "unknown_folder")
            .expect_err("expect to fail with BadRequest");

        client
            .check_module_permissions(AccessScope::Read, module_c.clone(), "unknown_folder")
            .expect_err("expect to fail with BadRequest");
    }

    #[tokio::test]
    // cli util OAuth flow to obtain refresh token for a google account
    async fn get_refresh_token() {
        let client_id = std::env::var("CLIENT_ID").unwrap();
        let client_secret = std::env::var("CLIENT_SECRET").unwrap();
        // From your client-secret.json
        let redirect_uri = "http://localhost:8080".to_string(); // Must match what you set in console

        // Scopes (use space-separated for multiple)
        let scope =
            "https://www.googleapis.com/auth/documents https://www.googleapis.com/auth/drive.file";

        // Generate auth URL
        let auth_url = reqwest::Url::parse_with_params(
            "https://accounts.google.com/o/oauth2/v2/auth",
            &[
                ("client_id", client_id.as_str()),
                ("redirect_uri", redirect_uri.as_str()),
                ("scope", scope),
                ("response_type", "code"),
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
        )
        .unwrap()
        .to_string();

        println!("Open this URL in your browser and authorize:\n{}", auth_url);
        println!("After authorization, copy the 'code' from the redirect URL (e.g., http://localhost:8080/?code=XXXX).");

        // Read code from stdin
        let stdin = std::io::stdin();
        let mut code = String::new();
        stdin.lock().read_line(&mut code).unwrap();
        let code = code.trim().to_string();

        // Exchange code for tokens
        let client = reqwest::Client::new();
        let params = [
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code".to_string()),
        ];

        let response: Value = client
            .post("https://oauth2.googleapis.com/token")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&params)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        println!("Refresh token: {}", response["refresh_token"]);
    }

    #[tokio::test]
    async fn create_markdown_doc() {
        let m = test_module("test_module", true);
        // From client-secret.json and the refresh token you obtained
        let client_id = std::env::var("CLIENT_ID").unwrap();
        let client_secret = std::env::var("CLIENT_SECRET").unwrap();
        let refresh_token = std::env::var("REFRESH_TOKEN").unwrap();
        let folder_id = std::env::var("FOLDER_ID").unwrap();

        // permissions: allow test module to write to folder_id
        let rw = json!({folder_id.clone(): ["test_module"]});
        let rw = from_value::<HashMap<String, HashSet<String>>>(rw).unwrap();

        let config = GoogleDocsConfig {
            client_id,
            client_secret,
            refresh_token,
            rw,
            r: HashMap::new(),
        };
        let docs = GoogleDocs::new(config);

        let content = r#"
# Document 

Markdown test

"#;

        let input = CreateDocFromMarkdownInput {
            folder_id,
            title: "markdown test".to_string(),
            content: content.to_string(),
        };
        let input = serde_json::to_string(&input).unwrap();

        let output = docs.create_doc_from_markdown(&input, m).await.unwrap();
        let output = serde_json::from_str::<Value>(&output).unwrap();
        let document_id = output["document_id"].as_str().unwrap();

        println!(
            "View at: https://docs.google.com/document/d/{}",
            document_id
        );
    }

    #[tokio::test]
    async fn create_csv_sheet() {
        let m = test_module("test_module", true);
        // From client-secret.json and the refresh token you obtained
        let client_id = std::env::var("CLIENT_ID").unwrap();
        let client_secret = std::env::var("CLIENT_SECRET").unwrap();
        let refresh_token = std::env::var("REFRESH_TOKEN").unwrap();
        let folder_id = std::env::var("FOLDER_ID").unwrap();

        // permissions: allow test module to write to folder_id
        let rw = json!({folder_id.clone(): ["test_module"]});
        let rw = from_value::<HashMap<String, HashSet<String>>>(rw).unwrap();

        let config = GoogleDocsConfig {
            client_id,
            client_secret,
            refresh_token,
            rw,
            r: HashMap::new(),
        };
        let docs = GoogleDocs::new(config);

        // CSV content
        let content = r#"Item,Cost,Category
Server,5,Hardware"#;

        let input = CreateSheetFromCsvInput {
            folder_id,
            title: "csv test".to_string(),
            content: content.to_string(),
        };
        let input = serde_json::to_string(&input).unwrap();

        let output = docs.create_sheet_from_csv(&input, m).await.unwrap();
        let output = serde_json::from_str::<Value>(&output).unwrap();
        let document_id = output["document_id"].as_str().unwrap();

        println!(
            "Sheet created: https://docs.google.com/spreadsheets/d/{}",
            document_id
        );
    }
}
