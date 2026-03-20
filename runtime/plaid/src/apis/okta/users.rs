use std::sync::Arc;

use http::StatusCode;

use crate::{
    apis::{okta::OktaError, ApiError},
    loader::PlaidModule,
};

use super::{Okta, OktaOperation};

impl Okta {
    /// Get user data by querying the Okta API
    pub async fn get_user_data(
        &self,
        query: &str,
        _: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let res = self
            .client
            .get(format!(
                "https://{}/api/v1/users/{query}",
                &self.config.domain
            ))
            .header(
                "Authorization",
                self.get_authorization_header(&OktaOperation::GetUserInfo)
                    .await
                    .map_err(ApiError::OktaError)?,
            )
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        let response = res.send().await.map_err(ApiError::NetworkError)?;
        let status = response.status();
        let data = response.text().await.map_err(ApiError::NetworkError)?;

        if status != StatusCode::OK {
            return Err(ApiError::OktaError(OktaError::UnexpectedStatusCode(
                status.as_u16(),
            )));
        }

        Ok(data)
    }
}
