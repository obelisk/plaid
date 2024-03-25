use crate::apis::{ApiError, okta::OktaError};

use super::Okta;

impl Okta {
    pub async fn get_user_data(&self, query: &str, _: &str) -> Result<String, ApiError> {
        let res = self.client.get(format!("https://{}/api/v1/users/{}", &self.config.domain, query))
            .header("Authorization", format!("SSWS {}", &self.config.token))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
    
        let response = res.send().await.map_err(|e| ApiError::NetworkError(e))?;
        let data = response.bytes().await.map_err(|e| ApiError::NetworkError(e))?;

        match String::from_utf8(data.to_vec()) {
            Ok(x) => Ok(x),
            Err(e) => {
                error!("Server returned data that was not encoded in a way we understand");
                Err(ApiError::OktaError(OktaError::BadData(e)))
            }
        }
    }
}