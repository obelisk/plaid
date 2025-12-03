use pulldown_cmark::{html, Options, Parser};
use reqwest::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tera::{Context, Tera};
use thiserror::Error;

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
const DRIVE_API_URL: &str = "https://www.googleapis.com/drive/v3/files";
// we use the 'upload' subdomain for multipart uploads
const DRIVE_UPLOAD_URL: &str =
    "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";
const DOCS_API_URL_BASE: &str = "https://docs.googleapis.com/v1/documents";

/// Swaps a long-lived Refresh Token for a short-lived Access Token
async fn refresh_access_token(
    client: &Client,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<String, GoogleDocsError> {
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let response = client.post(TOKEN_URL).form(&params).send().await?; // Automatically converted to GoogleDocsError::Reqwest

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(GoogleDocsError::OAuth {
            description: format!("Token refresh failed: {}", error_text),
        });
    }

    let json: Value = response.json().await?;

    json.get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or(GoogleDocsError::MissingField("access_token"))
}

/// Creates a blank Google Doc inside a specific folder
async fn create_google_doc(
    client: &Client,
    access_token: &str,
    folder_id: &str,
    doc_title: &str,
) -> Result<String, GoogleDocsError> {
    let body = json!({
        "name": doc_title,
        "mimeType": "application/vnd.google-apps.document",
        "parents": [folder_id]
    });

    let response = client
        .post(DRIVE_API_URL)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(GoogleDocsError::GoogleApi(format!(
            "Drive API request failed: {}",
            error_text
        )));
    }

    let json: Value = response.json().await?;

    let file_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or(GoogleDocsError::MissingField("id"))?;

    println!(
        "Successfully created file: Name='{}', ID='{}'",
        json.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown"),
        file_id
    );

    Ok(file_id.to_string())
}

/// Appends text to an existing Google Doc using the batchUpdate endpoint
async fn update_google_doc(
    client: &Client,
    access_token: &str,
    doc_id: &str,
    content: &str,
) -> Result<(), GoogleDocsError> {
    let url = format!("{}/{}:batchUpdate", DOCS_API_URL_BASE, doc_id);

    let body = json!({
        "requests": [
            {
                "insertText": {
                    "text": content,
                    "endOfSegmentLocation": {
                        "segmentId": ""
                    }
                }
            }
        ]
    });

    let response = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(GoogleDocsError::GoogleApi(format!(
            "Docs API update failed: {}",
            error_text
        )));
    }

    println!("Successfully appended content to document.");
    Ok(())
}

/// Converts Markdown to HTML and uploads it as a Google Doc
async fn create_doc_from_markdown(
    client: &Client,
    access_token: &str,
    folder_id: &str,
    doc_title: &str,
    markdown_content: &str,
) -> Result<String, GoogleDocsError> {
    // Convert Markdown to HTML
    // We enable tables and footnotes for better compatibility
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(markdown_content, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    println!(
        "Converted Markdown to HTML payload ({} bytes)",
        html_output.len()
    );

    // Prepare Multipart Body
    // Part A: JSON Metadata (defines target folder and file name)
    // We set mimeType to Google Docs to trigger the conversion
    let metadata = json!({
        "name": doc_title,
        "mimeType": "application/vnd.google-apps.document",
        "parents": [folder_id]
    });

    // Part B: The Content (HTML)
    // We upload it as text/html, and Drive converts it because of the metadata mimeType above
    let metadata_part =
        multipart::Part::text(metadata.to_string()).mime_str("application/json; charset=UTF-8")?;

    let content_part = multipart::Part::text(html_output).mime_str("text/html")?;

    let form = multipart::Form::new()
        .part("metadata", metadata_part)
        .part("media", content_part);

    // Send Request
    let response = client
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

    let file_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or(GoogleDocsError::MissingField("id"))?;

    Ok(file_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::CONTENT_TYPE;
    use serde_json::Value;
    use std::io::BufRead;
    use urlencoding::encode as url_encode;

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
        let auth_url = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&scope={}&response_type=code&access_type=offline&prompt=consent",
            url_encode(&client_id),
            url_encode(&redirect_uri),
            url_encode(scope)
        );

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
    async fn create_doc() {
        // From client-secret.json and the refresh token you obtained
        let client_id = std::env::var("CLIENT_ID").unwrap();
        let client_secret = std::env::var("CLIENT_SECRET").unwrap();
        let refresh_token = std::env::var("REFRESH_TOKEN").unwrap();
        let folder_id = std::env::var("FOLDER_ID").unwrap();

        let client = reqwest::Client::new();
        let access_token =
            refresh_access_token(&client, &client_id, &client_secret, &refresh_token)
                .await
                .unwrap();
        let doc_id = create_google_doc(&client, &access_token, &folder_id, "hello from plaid 2")
            .await
            .unwrap();

        // 5. Update the Document
        println!("Appending text to document...");
        let content = "Hello! This text was inserted by Rust via the Google Docs API.\n";
        update_google_doc(&client, &access_token, &doc_id, content)
            .await
            .unwrap();

        println!("doc_id {}", doc_id)
    }

    #[tokio::test]
    async fn create_markdown_doc() {
        // From client-secret.json and the refresh token you obtained
        let client_id = std::env::var("CLIENT_ID").unwrap();
        let client_secret = std::env::var("CLIENT_SECRET").unwrap();
        let refresh_token = std::env::var("REFRESH_TOKEN").unwrap();
        let folder_id = std::env::var("FOLDER_ID").unwrap();

        let client = reqwest::Client::new();
        let access_token =
            refresh_access_token(&client, &client_id, &client_secret, &refresh_token)
                .await
                .unwrap();

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

        // Initialize Tera and render
        let mut tera = Tera::default();
        tera.add_raw_template("markdown_doc", template_input)
            .map_err(|e| GoogleDocsError::Template(e.to_string()))
            .unwrap();

        let mut context = Context::new();
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

        // Loop through the JSON object and insert each key-value pair into the Tera context
        if let Some(map) = data.as_object() {
            for (key, value) in map {
                context.insert(key, value);
            }
        }

        let rendered_markdown = tera
            .render("markdown_doc", &context)
            .map_err(|e| GoogleDocsError::Template(e.to_string()))
            .unwrap();

        // Create document
        let doc_id = create_doc_from_markdown(
            &client,
            &access_token,
            &folder_id,
            "markdown test",
            &rendered_markdown,
        )
        .await
        .unwrap();
        println!("Success! Document ID: {}", doc_id);
        println!("View at: https://docs.google.com/document/d/{}", doc_id);
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct TokenResponse {
    access_token: String,
    expires_in: u32,
    refresh_token: Option<String>, // Only on first grant
    scope: String,
    token_type: String,
}
