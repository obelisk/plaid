use serde::Deserialize;

use google_docs::{GoogleDocs, GoogleDocsConfig};

pub mod google_docs;

#[derive(Deserialize)]
pub struct GcpConfig {
    pub google_docs: Option<GoogleDocsConfig>,
}

/// Contains all GCP services that Plaid implements APIs for
pub struct Gcp {
    /// Google Docs
    pub google_docs: Option<GoogleDocs>,
}

impl Gcp {
    pub async fn new(config: GcpConfig) -> Self {
        let google_docs = match config.google_docs {
            Some(conf) => Some(GoogleDocs::new(conf)),
            None => None,
        };

        Gcp { google_docs }
    }
}
