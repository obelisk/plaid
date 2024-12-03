#[cfg(feature = "aws")]
pub mod aws;
pub mod general;
pub mod github;
pub mod npm;
pub mod okta;
pub mod pagerduty;
pub mod rustica;
pub mod slack;
pub mod splunk;
pub mod web;
pub mod yubikey;

#[cfg(feature = "aws")]
use aws::{Aws, AwsConfig};
#[cfg(feature = "aws")]
use aws_sdk_kms::operation::get_public_key::GetPublicKeyError;
#[cfg(feature = "aws")]
use aws_sdk_kms::{error::SdkError, operation::sign::SignError};
use crossbeam_channel::Sender;
use general::{General, GeneralConfig};
use github::{Github, GithubConfig};
use npm::{Npm, NpmConfig};
use okta::{Okta, OktaConfig};
use pagerduty::{PagerDuty, PagerDutyConfig};
use plaid_stl::npm::shared_structs::NpmError;
use serde::Deserialize;
use slack::{Slack, SlackConfig};
use splunk::{Splunk, SplunkConfig};
use tokio::runtime::Runtime;
use web::{Web, WebConfig};
use yubikey::{Yubikey, YubikeyConfig};

use crate::{data::DelayedMessage, executor::Message};

use self::rustica::{Rustica, RusticaConfig};

pub struct Api {
    pub runtime: Runtime,
    #[cfg(feature = "aws")]
    pub aws: Option<Aws>,
    pub general: Option<General>,
    pub github: Option<Github>,
    pub npm: Option<Npm>,
    pub okta: Option<Okta>,
    pub pagerduty: Option<PagerDuty>,
    pub rustica: Option<Rustica>,
    pub slack: Option<Slack>,
    pub splunk: Option<Splunk>,
    pub yubikey: Option<Yubikey>,
    pub web: Option<Web>,
}

#[derive(Deserialize)]
pub struct Apis {
    #[cfg(feature = "aws")]
    pub aws: Option<AwsConfig>,
    pub general: Option<GeneralConfig>,
    pub github: Option<GithubConfig>,
    pub npm: Option<NpmConfig>,
    pub okta: Option<OktaConfig>,
    pub pagerduty: Option<PagerDutyConfig>,
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
    #[cfg(feature = "aws")]
    KmsSignError(SdkError<SignError>),
    #[cfg(feature = "aws")]
    KmsGetPublicKeyError(SdkError<GetPublicKeyError>),
    NetworkError(reqwest::Error),
    NpmError(NpmError),
    OktaError(okta::OktaError),
    PagerDutyError(pagerduty::PagerDutyError),
    RusticaError(rustica::RusticaError),
    SlackError(slack::SlackError),
    SplunkError(splunk::SplunkError),
    YubikeyError(yubikey::YubikeyError),
    WebError(web::WebError),
}

impl Api {
    pub async fn new(
        config: Apis,
        log_sender: Sender<Message>,
        delayed_log_sender: Sender<DelayedMessage>,
    ) -> Self {
        #[cfg(feature = "aws")]
        let aws = match config.aws {
            Some(aws) => Some(Aws::new(aws).await),
            _ => None,
        };

        let general = config
            .general
            .map(|gc| General::new(gc, log_sender, delayed_log_sender));

        let github = config.github.map(Github::new);

        let npm = match config.npm {
            Some(npm) => match Npm::new(npm) {
                Ok(npm) => Some(npm),
                Err(_) => {
                    error!("Something went wrong while initializing the npm API: proceeding without. This should be investigated!");
                    None
                }
            },
            _ => None,
        };

        let okta = config.okta.map(Okta::new);

        let pagerduty = config.pagerduty.map(PagerDuty::new);

        let rustica = config.rustica.map(Rustica::new);

        let slack = config.slack.map(Slack::new);

        let splunk = config.splunk.map(Splunk::new);

        let yubikey = config.yubikey.map(Yubikey::new);

        let web = config.web.map(Web::new);

        Self {
            runtime: Runtime::new().unwrap(),
            #[cfg(feature = "aws")]
            aws,
            general,
            github,
            npm,
            okta,
            pagerduty,
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
