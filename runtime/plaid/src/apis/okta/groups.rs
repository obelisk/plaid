use std::collections::HashMap;

use crate::apis::ApiError;

use super::{Okta, OktaError};

impl Okta {
    pub async fn remove_user_from_group(&self, params: &str, _: &str) -> Result<u32, ApiError> {
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
                    .map_err(|e| ApiError::OktaError(e))?,
            )
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        match res.send().await {
            Ok(r) => {
                if r.status() == 204 {
                    Ok(0)
                } else {
                    Err(ApiError::OktaError(OktaError::UnexpectedStatusCode(
                        r.status().as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }
}
