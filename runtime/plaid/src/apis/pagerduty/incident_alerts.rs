use std::sync::Arc;

use super::{PagerDuty, PagerDutyRestConfig};
use crate::{apis::ApiError, loader::PlaidModule};
use plaid_stl::pagerduty::IncidentAlertsResponse;
use serde::Deserialize;

const PAGERDUTY_API_ADDRESS: &str = "https://api.pagerduty.com";

#[derive(Deserialize)]
struct GetIncidentAlertsRequest {
    incident_id: String,
}

impl PagerDuty {
    /// Get alerts for an existing PagerDuty incident.
    pub async fn get_incident_alerts(
        &self,
        request: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GetIncidentAlertsRequest =
            serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let rest_config = self.rest_config_for_incident_alerts(module)?;
        let url = format!(
            "{PAGERDUTY_API_ADDRESS}/incidents/{}/alerts",
            request.incident_id
        );

        let response = self
            .client
            .get(url)
            .header("Accept", "application/vnd.pagerduty+json;version=2")
            .header(
                "Authorization",
                format!("Token token={}", rest_config.token),
            )
            .send()
            .await
            .map_err(|e| ApiError::PagerDutyError(super::PagerDutyError::NetworkError(e)))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ApiError::PagerDutyError(
                super::PagerDutyError::UnexpectedStatusCode(status.as_u16()),
            ));
        }

        let parsed: IncidentAlertsResponse = serde_json::from_str(&body)
            .map_err(|_| ApiError::PagerDutyError(super::PagerDutyError::UnexpectedPayload))?;

        serde_json::to_string(&parsed).map_err(|_| ApiError::ImpossibleError)
    }

    fn rest_config_for_incident_alerts(
        &self,
        module: Arc<PlaidModule>,
    ) -> Result<&PagerDutyRestConfig, ApiError> {
        let rest_config = self.config.rest.as_ref().ok_or_else(|| {
            ApiError::ConfigurationError("PagerDuty REST API is not configured".into())
        })?;

        if !rest_config
            .incident_alerts
            .allowed_rules
            .contains(&module.to_string())
        {
            error!("{module} tried to get PagerDuty incident alerts without permission");
            return Err(ApiError::BadRequest);
        }

        if module.test_mode && !rest_config.incident_alerts.available_in_test_mode {
            error!("{module} tried to get PagerDuty incident alerts in test mode");
            return Err(ApiError::TestMode);
        }

        Ok(rest_config)
    }
}
