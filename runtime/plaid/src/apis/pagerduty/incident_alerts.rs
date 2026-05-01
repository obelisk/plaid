use std::sync::Arc;

use super::{PagerDuty, PagerDutyGetIncidentAlertsConfig};
use crate::{apis::ApiError, loader::PlaidModule};
use plaid_stl::pagerduty::{GetIncidentAlertsRequest, IncidentAlertsResponse};

const PAGERDUTY_API_ADDRESS: &str = "https://api.pagerduty.com";

impl PagerDuty {
    /// Get alerts for an existing PagerDuty incident.
    pub async fn get_incident_alerts(
        &self,
        request: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GetIncidentAlertsRequest =
            serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let incident_alerts_config = self.config_for_get_incident_alerts(module.as_ref())?;

        if !valid_pagerduty_incident_id(&request.incident_id) {
            warn!("{module} tried to get PagerDuty incident alerts with an invalid incident id");
            return Err(ApiError::BadRequest);
        }

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
                format!("Token token={}", incident_alerts_config.token),
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

    fn config_for_get_incident_alerts(
        &self,
        module: &PlaidModule,
    ) -> Result<&PagerDutyGetIncidentAlertsConfig, ApiError> {
        let incident_alerts_config = self.config.get_incident_alerts.as_ref().ok_or_else(|| {
            ApiError::ConfigurationError("PagerDuty get_incident_alerts is not configured".into())
        })?;

        if !incident_alerts_config
            .allowed_rules
            .contains(&module.to_string())
        {
            warn!("{module} tried to get PagerDuty incident alerts without permission");
            return Err(ApiError::BadRequest);
        }

        Ok(incident_alerts_config)
    }
}

fn valid_pagerduty_incident_id(incident_id: &str) -> bool {
    !incident_id.is_empty()
        && incident_id.len() <= 64
        && incident_id.bytes().all(|b| b.is_ascii_alphanumeric())
}
