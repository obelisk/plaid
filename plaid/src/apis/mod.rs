pub mod general;
pub mod github;
pub mod okta;
pub mod pagerduty;
pub mod quorum;
pub mod rustica;
pub mod slack;
pub mod splunk;
pub mod web;
pub mod yubikey;

use crossbeam_channel::Sender;
use serde::Deserialize;
use tokio::runtime::Runtime;

use general::{General, GeneralConfig};
use github::{Github, GithubConfig};
use okta::{Okta, OktaConfig};
use pagerduty::{PagerDuty, PagerDutyConfig};
use quorum::{Quorum, QuorumConfig};
use slack::{Slack, SlackConfig};
use splunk::{Splunk, SplunkConfig};
use web::{Web, WebConfig};
use yubikey::{Yubikey, YubikeyConfig};

use crate::{data::DelayedMessage, executor::Message};

use self::rustica::{Rustica, RusticaConfig};

pub struct Api {
    pub runtime: Runtime,
    pub general: Option<General>,
    pub github: Option<Github>,
    pub okta: Option<Okta>,
    pub pagerduty: Option<PagerDuty>,
    pub quorum: Option<Quorum>,
    pub rustica: Option<Rustica>,
    pub slack: Option<Slack>,
    pub splunk: Option<Splunk>,
    pub yubikey: Option<Yubikey>,
    pub web: Option<Web>,
}

#[derive(Deserialize)]
pub struct Apis {
    pub general: Option<GeneralConfig>,
    pub github: Option<GithubConfig>,
    pub okta: Option<OktaConfig>,
    pub pagerduty: Option<PagerDutyConfig>,
    pub quorum: Option<QuorumConfig>,
    pub rustica: Option<RusticaConfig>,
    pub slack: Option<SlackConfig>,
    pub splunk: Option<SplunkConfig>,
    pub yubikey: Option<YubikeyConfig>,
    pub web: Option<WebConfig>,
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest,
    ImpossibleError,
    ConfigurationError(String),
    MissingParameter(String),

    GitHubError(github::GitHubError),
    NetworkError(reqwest::Error),
    OktaError(okta::OktaError),
    PagerDutyError(pagerduty::PagerDutyError),
    QuorumError(quorum::QuorumError),
    RusticaError(rustica::RusticaError),
    SlackError(slack::SlackError),
    SplunkError(splunk::SplunkError),
    YubikeyError(yubikey::YubikeyError),
    WebError(web::WebError),
}

impl Api {
    pub fn new(
        config: Apis,
        log_sender: Sender<Message>,
        delayed_log_sender: Sender<DelayedMessage>,
    ) -> Self {
        let general = match config.general {
            Some(gc) => Some(General::new(gc, log_sender, delayed_log_sender)),
            _ => None,
        };

        let github = match config.github {
            Some(gh) => Some(Github::new(gh)),
            _ => None,
        };

        let okta = match config.okta {
            Some(oc) => Some(Okta::new(oc)),
            _ => None,
        };

        let pagerduty = match config.pagerduty {
            Some(pd) => Some(PagerDuty::new(pd)),
            _ => None,
        };

        #[cfg(feature = "quorum")]
        let quorum = match config.quorum {
            Some(q) => Some(Quorum::new(q)),
            _ => None,
        };
        #[cfg(not(feature = "quorum"))]
        let quorum = None;

        let rustica = match config.rustica {
            Some(q) => Some(Rustica::new(q)),
            _ => None,
        };

        let slack = match config.slack {
            Some(sc) => Some(Slack::new(sc)),
            _ => None,
        };

        let splunk = match config.splunk {
            Some(sp) => Some(Splunk::new(sp)),
            _ => None,
        };

        let yubikey = match config.yubikey {
            Some(yk) => Some(Yubikey::new(yk)),
            _ => None,
        };

        let web = match config.web {
            Some(web) => Some(Web::new(web)),
            _ => None,
        };

        Self {
            runtime: Runtime::new().unwrap(),
            general,
            github,
            okta,
            pagerduty,
            quorum,
            rustica,
            slack,
            splunk,
            yubikey,
            web,
        }
    }
}

/// This function provides the default timeout value in seconds.
/// It is used as the default value for deserialization of various API configs,
/// in the event that no value is provided.
fn default_timeout_seconds() -> u64 {
    5
}
