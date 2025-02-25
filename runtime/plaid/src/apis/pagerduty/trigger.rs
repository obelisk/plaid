use std::sync::Arc;

use super::{PagerDuty, PagerDutyError};
use crate::{apis::ApiError, loader::PlaidModule};

use serde::{Deserialize, Serialize};

const PAGERDUTY_ENQUEUE_ADDRESS: &str = "https://events.pagerduty.com/v2/enqueue";

/// Payload sent to PagerDuty to trigger an incident
#[derive(Serialize)]
struct PagerDutyTriggerPayload {
    summary: String,
    source: String,
    severity: String,
}

/// Payload sent to PagerDuty to trigger an incident
#[derive(Serialize)]
struct PagerDutyTrigger {
    routing_key: String,
    event_action: String,
    payload: PagerDutyTriggerPayload,
}

enum TriggerIncidentResult {
    Success = 0,
    BadRequest = 1,
    UnknownService = 2,
    TriggerFailed = 3,
}

/// Request to trigger a PagerDuty incident
#[derive(Deserialize)]
struct TriggerRequest<'a> {
    service: &'a str,
    description: &'a str,
}

impl PagerDuty {
    /// Trigger a PagerDuty incident
    pub async fn trigger_incident(
        &self,
        request: &str,
        module_name: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: TriggerRequest = match serde_json::from_str(request) {
            Ok(r) => r,
            Err(_) => return Ok(TriggerIncidentResult::BadRequest as u32),
        };

        let service = request.service;
        let description = request.description;

        let routing_key = match self.config.services.get(service) {
            Some(h) => h.to_owned(),
            None => {
                warn!(
                    "A module tried to trigger a PagerDuty service that doesn't exist: {service}"
                );
                return Ok(TriggerIncidentResult::UnknownService as u32);
            }
        };

        let payload = PagerDutyTriggerPayload {
            summary: description.to_string(),
            source: module_name.to_string(),
            severity: "error".to_owned(),
        };

        let trigger = PagerDutyTrigger {
            routing_key,
            event_action: "trigger".to_owned(),
            payload,
        };

        info!("Triggering an incident for {service}");
        match self
            .client
            .post(PAGERDUTY_ENQUEUE_ADDRESS)
            .json(&trigger)
            .send()
            .await
        {
            Ok(r) => {
                debug!("{:?}", r);
                if r.status().as_u16() >= 200 && r.status().as_u16() < 300 {
                    Ok(TriggerIncidentResult::Success as u32)
                } else {
                    Ok(TriggerIncidentResult::TriggerFailed as u32)
                }
            }
            Err(e) => {
                debug!("{:?}", e);
                return Err(ApiError::PagerDutyError(PagerDutyError::NetworkError(e)));
            }
        }
    }
}
