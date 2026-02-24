use serde::Deserialize;

use bigquery::{BigQuery, BigQueryConfig};
use google_docs::{GoogleDocs, GoogleDocsConfig};

pub mod bigquery;
pub mod google_docs;

#[derive(Deserialize)]
pub struct GcpConfig {
    pub google_docs: Option<GoogleDocsConfig>,
    pub bigquery: Option<BigQueryConfig>,
}

/// Contains all GCP services that Plaid implements APIs for
pub struct Gcp {
    /// Google Docs
    pub google_docs: Option<GoogleDocs>,
    /// BigQuery
    pub bigquery: Option<BigQuery>,
}

impl Gcp {
    pub async fn new(config: GcpConfig) -> Self {
        let google_docs = match config.google_docs {
            Some(conf) => Some(GoogleDocs::new(conf)),
            None => None,
        };

        let bigquery = match config.bigquery {
            Some(conf) => Some(
                BigQuery::new(conf)
                    .await
                    .expect("Failed to initialize BigQuery client"),
            ),
            None => None,
        };

        Gcp {
            google_docs,
            bigquery,
        }
    }
}
