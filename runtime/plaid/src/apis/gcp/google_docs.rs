use chrono::{Duration, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::Read;

// Structs for service account key JSON (partial—add fields if needed)
#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
    token_uri: String,
}

// Struct for JWT claims
#[derive(Serialize)]
struct Claims {
    iss: String,
    scope: String,
    aud: String,
    iat: i64,
    exp: i64,
}

// Struct for token response
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

// Struct for Drive file creation response
#[derive(Deserialize)]
struct FileResponse {
    id: String,
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, Value};

    use super::*;

    #[tokio::test]
    async fn create_doc() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("it works {}/runtime/plaid/src/apis/gcp", dir);

        // Constants—update these
        let key_file = format!("{dir}/src/apis/gcp/sa.json");
        let folder_id = "";
        // From folder URL
        let doc_title = "New Document";
        let scopes =
            "https://www.googleapis.com/auth/drive https://www.googleapis.com/auth/documents";

        // Load service account key (sync for simplicity)
        let mut file = File::open(key_file).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        let key: ServiceAccountKey = serde_json::from_str(&contents).unwrap();

        // Create JWT claims
        let now = Utc::now().timestamp();
        let claims = Claims {
            iss: key.client_email,
            scope: scopes.to_string(),
            aud: key.token_uri,
            iat: now,
            exp: now + Duration::hours(1).num_seconds(),
        };

        // Sign JWT using ring internally via from_rsa_pem (handles PKCS8 PEM)
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        let jwt = encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(key.private_key.as_bytes()).unwrap(),
        )
        .unwrap();

        // Create async HTTP client
        let client = Client::new();

        // Exchange JWT for access token
        let token_res = client
            .post("https://oauth2.googleapis.com/token")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!(
                "grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion={}",
                jwt
            ))
            .send()
            .await
            .unwrap();
        let token: TokenResponse = token_res.json().await.unwrap();

        // Create new Google Doc via Drive API
        let create_body = json!({
            "name": doc_title,
            "mimeType": "application/vnd.google-apps.document",
            "parents": [folder_id]
        });
        let create_res = client
            .post("https://www.googleapis.com/drive/v3/files")
            .header(AUTHORIZATION, format!("Bearer {}", token.access_token))
            .header(CONTENT_TYPE, "application/json")
            .json(&create_body)
            .send()
            .await
            .unwrap();
        let file: Value = create_res.json().await.unwrap();
        println!("{file}");
        let file: FileResponse = from_value(file).unwrap();
        println!("Created new Doc with ID: {}", file.id);

        // Add sample text via Docs API
        let update_body = json!({
            "requests": [{
                "insertText": {
                    "location": { "index": 1 },
                    "text": "Hello, world!"
                }
            }]
        });
        let update_res = client
            .post(format!(
                "https://docs.googleapis.com/v1/documents/{}:batchUpdate",
                file.id
            ))
            .header(AUTHORIZATION, format!("Bearer {}", token.access_token))
            .header(CONTENT_TYPE, "application/json")
            .json(&update_body)
            .send()
            .await
            .unwrap();
        if update_res.status().is_success() {
            println!("Added sample text to the Doc.");
        } else {
            println!("Error updating Doc: {}", update_res.text().await.unwrap());
        }
    }
}
