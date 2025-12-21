use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use pulldown_cmark::{html, Options, Parser};
use reqwest::{multipart, Client};
use serde::Deserialize;
use serde_json::{json, Value};
use tera::{Context, Tera};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Deserialize)]
pub struct GoogleDocsConfig {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    // Mapping for readers: folder_id -> list of rules
    r: HashMap<String, HashSet<String>>,
    // Mapping for writers: folder_id -> list of rules
    wr: HashMap<String, HashSet<String>>,
}

pub struct GoogleDocs {
    client: Client,
    client_id: String,
    client_secret: String,
    refresh_token: String,
    access_token: Mutex<Option<(String, Instant)>>,
    // Mapping for readers: folder_id -> list of rules
    r: HashMap<String, HashSet<String>>,
    // Mapping for writers: folder_id -> list of rules
    wr: HashMap<String, HashSet<String>>,
}

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
    #[error("Template error: {0}")]
    Template(String),
}

// Google API Endpoints
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
// we use the 'upload' subdomain for multipart uploads
const DRIVE_UPLOAD_URL: &str =
    "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";

impl GoogleDocs {
    pub fn new(config: GoogleDocsConfig) -> Self {
        let GoogleDocsConfig {
            client_id,
            client_secret,
            refresh_token,
            r,
            wr,
        } = config;

        let client = Client::new();

        Self {
            client,
            client_id,
            client_secret,
            refresh_token,
            r,
            wr,
            access_token: Mutex::new(None),
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
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("refresh_token", self.refresh_token.as_str()),
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

    /// Converts Markdown to HTML and uploads it as a Google Doc
    pub async fn create_doc_from_markdown(
        &self,
        folder_id: &str,
        doc_title: &str,
        template: &str,
        variables: Value,
    ) -> Result<String, GoogleDocsError> {
        let rendered_template = render_template(template, variables).unwrap();
        let html_output = markdown_to_html(&rendered_template);
        self.upload_file(
            folder_id,
            doc_title,
            html_output,
            "text/html",
            "application/vnd.google-apps.document",
        )
        .await
    }

    /// Uploads CSV content and instructs Drive to convert it to a Google Sheet
    pub async fn create_sheet_from_csv(
        &self,
        folder_id: &str,
        sheet_title: &str,
        template: &str,
        variables: Value,
    ) -> Result<String, GoogleDocsError> {
        let rendered_template = render_template(template, variables).unwrap();
        self.upload_file(
            folder_id,
            sheet_title,
            rendered_template,
            "text/csv",
            "application/vnd.google-apps.spreadsheet",
        )
        .await
    }
}

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

fn render_template(template: &str, data: serde_json::Value) -> Result<String, GoogleDocsError> {
    // Initialize Tera and render
    let mut tera = Tera::default();
    tera.add_raw_template("template", template)
        .map_err(|e| GoogleDocsError::Template(e.to_string()))?;

    let mut context = Context::new();

    // Loop through the JSON object and insert each key-value pair into the Tera context
    if let Some(map) = data.as_object() {
        for (key, value) in map {
            context.insert(key, value);
        }
    }

    let rendered = tera
        .render("template", &context)
        .map_err(|e| GoogleDocsError::Template(e.to_string()))?;

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::CONTENT_TYPE;
    use serde_json::Value;
    use std::io::BufRead;

    #[tokio::test]
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
        // From client-secret.json and the refresh token you obtained
        let client_id = std::env::var("CLIENT_ID").unwrap();
        let client_secret = std::env::var("CLIENT_SECRET").unwrap();
        let refresh_token = std::env::var("REFRESH_TOKEN").unwrap();
        let folder_id = std::env::var("FOLDER_ID").unwrap();

        let config = GoogleDocsConfig {
            client_id,
            client_secret,
            refresh_token,
            r: HashMap::new(),
            wr: HashMap::new(),
        };
        let docs = GoogleDocs::new(config);

        // Define Markdown Template with Tera placeholders
        let template_input = r#"
# Project: {{ project_name }}

This document was created by **{{ author }}** on {{ date }}.

## Status Report
* **Priority**: {{ priority }}
* **Department**: {{ department }}

## Financials

| Item | Cost |
|------|------|
| Server | ${{ cost_server }} |
| License | ${{ cost_license }} |
| **Total** | **${{ cost_server + cost_license }}** |

## Conclusion
{{ conclusion_text }}
"#;

        // Create a JSON object to store all parameters
        let data = json!({
            "project_name": "Omega Red",
            "author": "Rust Bot",
            "date": "2023-10-27",
            "priority": "Critical",
            "department": "Engineering",
            "cost_server": 1500,
            "cost_license": 500,
            "conclusion_text": "Automated template rendering successful. Math operations in table verified."
        });

        // let rendered_markdown = render_template(template_input, data).unwrap();
        // Create document
        let doc_id = docs
            .create_doc_from_markdown(&folder_id, "markdown test", &template_input, data)
            .await
            .unwrap();
        println!("Success! Document ID: {}", doc_id);
        println!("View at: https://docs.google.com/document/d/{}", doc_id);
    }

    #[tokio::test]
    async fn create_csv_sheet() {
        // From client-secret.json and the refresh token you obtained
        let client_id = std::env::var("CLIENT_ID").unwrap();
        let client_secret = std::env::var("CLIENT_SECRET").unwrap();
        let refresh_token = std::env::var("REFRESH_TOKEN").unwrap();
        let folder_id = std::env::var("FOLDER_ID").unwrap();

        let config = GoogleDocsConfig {
            client_id,
            client_secret,
            refresh_token,
            r: HashMap::new(),
            wr: HashMap::new(),
        };
        let docs = GoogleDocs::new(config);

        // CSV Template
        let csv_template = r#"Item,Cost,Category
Server,{{ cost_server }},Hardware
License,{{ cost_license }},Software
Total,{{ cost_server + cost_license }},"#;

        let data = json!({
            "project_name": "Omega Red",
            "author": "Rust Bot",
            "date": "2023-10-27",
            "priority": "Critical",
            "department": "Engineering",
            "cost_server": 1500,
            "cost_license": 500,
        });

        let sheet_id = docs
            .create_sheet_from_csv(&folder_id, "Rust Financials Sheet", &csv_template, data)
            .await
            .unwrap();

        println!(
            "Sheet created: https://docs.google.com/spreadsheets/d/{}",
            sheet_id
        );
    }
}
