use serde::Serialize;

use crate::PlaidFunctionError;

#[derive(Serialize)]
struct TriggerRequest {
    service: String,
    description: String,
}

pub enum TriggerIncidentResult {
    Success,
    BadRequest,
    UnknownService,
    TriggerFailed,
    Unknown(u32),
}

impl core::fmt::Display for TriggerIncidentResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TriggerIncidentResult::Success => write!(f, "Successfully triggered"),
            TriggerIncidentResult::BadRequest => write!(f, "Request was not encoded correctly"),
            TriggerIncidentResult::UnknownService => write!(f, "Requested service is unknown"),
            TriggerIncidentResult::TriggerFailed => write!(
                f,
                "PagerDuty returned a failure when trigger attempt was sent"
            ),
            TriggerIncidentResult::Unknown(x) => {
                write!(f, "Plaid gave an unknown status code from PagerDuty: {x}")
            }
        }
    }
}

// These functions are for compatibility and will be deprecated
pub fn trigger_incident(service: &str, description: &str) -> Result<(), i32> {
    if let Ok(TriggerIncidentResult::Success) = trigger_incident_detailed(service, description) {
        Ok(())
    } else {
        Err(-100)
    }
}

pub fn trigger_incident_detailed(
    service: &str,
    description: &str,
) -> Result<TriggerIncidentResult, PlaidFunctionError> {
    extern "C" {
        // Trigger a PagerDuty incident for a given service
        new_host_function!(pagerduty, trigger_incident);
    }

    let request = TriggerRequest {
        service: service.to_owned(),
        description: description.to_owned(),
    };

    // There shouldn't be anyway for this to fail because none of the
    // unserializable types are possible here
    let request = serde_json::to_string(&request).unwrap();
    let res = unsafe { pagerduty_trigger_incident(request.as_ptr(), request.len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    match res {
        0 => Ok(TriggerIncidentResult::Success),
        // This should never happen because the STL handles creating the request
        1 => Ok(TriggerIncidentResult::BadRequest),
        2 => Ok(TriggerIncidentResult::UnknownService),
        3 => Ok(TriggerIncidentResult::TriggerFailed),
        n => Ok(TriggerIncidentResult::Unknown(n as u32)),
    }
}
