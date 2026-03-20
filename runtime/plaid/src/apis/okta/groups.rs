use std::{collections::HashMap, sync::Arc};

use http::StatusCode;

use crate::{apis::ApiError, loader::PlaidModule};

use super::{Okta, OktaError};

impl Okta {
    /// Remove a user from an Okta group
    pub async fn remove_user_from_group(
        &self,
        params: &str,
        _: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let user_id = request.get("user_id").ok_or(ApiError::BadRequest)?;
        let group_id = request.get("group_id").ok_or(ApiError::BadRequest)?;

        let res = self
            .client
            .delete(format!(
                "https://{}/api/v1/groups/{group_id}/users/{user_id}",
                &self.config.domain
            ))
            .header(
                "Authorization",
                self.get_authorization_header(&super::OktaOperation::RemoveUserFromGroup)
                    .await
                    .map_err(ApiError::OktaError)?,
            )
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        let response = res.send().await.map_err(ApiError::NetworkError)?;

        let status = response.status();
        if status == StatusCode::NO_CONTENT {
            Ok(0)
        } else {
            Err(ApiError::OktaError(OktaError::UnexpectedStatusCode(
                status.as_u16(),
            )))
        }
    }
}
