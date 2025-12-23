#[cfg(feature = "aws")]
pub mod aws;
pub mod blockchain;
pub mod cryptography;
#[cfg(feature = "gcp")]
pub mod gcp;
pub mod general;
pub mod github;
pub mod jira;
pub mod npm;
pub mod okta;
pub mod pagerduty;
pub mod rustica;
pub mod slack;
pub mod splunk;
pub mod web;
pub mod yubikey;

#[cfg(feature = "aws")]
use crate::apis::aws::kms::KmsErrors;
use crate::apis::blockchain::{evm, Blockchain, BlockchainConfig};
#[cfg(feature = "gcp")]
use crate::apis::gcp::{Gcp, GcpConfig};
use crate::apis::jira::{Jira, JiraConfig};
#[cfg(feature = "aws")]
use aws::s3::S3Errors;
#[cfg(feature = "aws")]
use aws::{Aws, AwsConfig};

#[cfg(feature = "aws")]
use aws_sdk_dynamodb::operation::delete_item::DeleteItemError;
#[cfg(feature = "aws")]
use aws_sdk_dynamodb::operation::put_item::PutItemError;
#[cfg(feature = "aws")]
use aws_sdk_dynamodb::operation::query::QueryError;
#[cfg(feature = "aws")]
use aws_sdk_kms::operation::get_public_key::GetPublicKeyError;
#[cfg(feature = "aws")]
use aws_sdk_kms::{error::SdkError, operation::sign::SignError};

use crate::{data::DelayedMessage, executor::Message};
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

use self::rustica::{Rustica, RusticaConfig};
use crate::apis::cryptography::{Cryptography, CryptographyConfig};

/// All the APIs that Plaid can use
pub struct Api {
    pub runtime: Runtime,
    pub cryptography: Option<Cryptography>,
    #[cfg(feature = "aws")]
    pub aws: Option<Aws>,
    #[cfg(feature = "gcp")]
    pub gcp: Option<Gcp>,
    pub general: Option<General>,
    pub github: Option<Github>,
    pub jira: Option<Jira>,
    pub npm: Option<Npm>,
    pub okta: Option<Okta>,
    pub pagerduty: Option<PagerDuty>,
    pub rustica: Option<Rustica>,
    pub slack: Option<Slack>,
    pub splunk: Option<Splunk>,
    pub yubikey: Option<Yubikey>,
    pub web: Option<Web>,
    pub blockchain: Option<Blockchain>,
}

/// Configurations for all the APIs Plaid can use
#[derive(Deserialize)]
pub struct ApiConfigs {
    #[cfg(feature = "aws")]
    pub aws: Option<AwsConfig>,
    #[cfg(feature = "gcp")]
    pub gcp: Option<GcpConfig>,
    pub cryptography: Option<CryptographyConfig>,
    pub general: Option<GeneralConfig>,
    pub github: Option<GithubConfig>,
    pub jira: Option<JiraConfig>,
    pub npm: Option<NpmConfig>,
    pub okta: Option<OktaConfig>,
    pub pagerduty: Option<PagerDutyConfig>,
    pub rustica: Option<RusticaConfig>,
    pub slack: Option<SlackConfig>,
    pub splunk: Option<SplunkConfig>,
    pub yubikey: Option<YubikeyConfig>,
    pub web: Option<WebConfig>,
    pub blockchain: Option<BlockchainConfig>,
}

#[derive(Debug)]
pub enum ApiError {
    CryptographyError(String),
    BadRequest,
    ImpossibleError,
    ConfigurationError(String),
    MissingParameter(String),
    GitHubError(github::GitHubError),
    #[cfg(feature = "aws")]
    SerdeError(String),
    #[cfg(feature = "aws")]
    DynamoDbPutItemError(SdkError<PutItemError>),
    #[cfg(feature = "aws")]
    DynamoDbDeleteItemError(SdkError<DeleteItemError>),
    #[cfg(feature = "aws")]
    DynamoDbQueryError(SdkError<QueryError>),
    #[cfg(feature = "aws")]
    KmsSignError(SdkError<SignError>),
    #[cfg(feature = "aws")]
    KmsGetPublicKeyError(SdkError<GetPublicKeyError>),
    #[cfg(feature = "aws")]
    S3Error(aws::s3::S3Errors),
    #[cfg(feature = "gcp")]
    GoogleDocsError(gcp::google_docs::GoogleDocsError),
    #[cfg(feature = "aws")]
    KmsError(aws::kms::KmsErrors),
    NetworkError(reqwest::Error),
    NpmError(NpmError),
    OktaError(okta::OktaError),
    PagerDutyError(pagerduty::PagerDutyError),
    RusticaError(rustica::RusticaError),
    SlackError(slack::SlackError),
    SplunkError(splunk::SplunkError),
    YubikeyError(yubikey::YubikeyError),
    WebError(web::WebError),
    BlockchainError(blockchain::BlockchainError),
    TestMode,
    JiraError(jira::JiraError),
    NetworkResponseTooLarge,
}

impl From<evm::EvmCallError> for ApiError {
    fn from(e: evm::EvmCallError) -> Self {
        ApiError::BlockchainError(blockchain::BlockchainError::EvmError(e))
    }
}

#[cfg(feature = "aws")]
impl From<S3Errors> for ApiError {
    fn from(e: S3Errors) -> Self {
        Self::S3Error(e)
    }
}

#[cfg(feature = "aws")]
impl From<KmsErrors> for ApiError {
    fn from(e: KmsErrors) -> Self {
        Self::KmsError(e)
    }
}

impl Api {
    pub async fn new(
        config: ApiConfigs,
        log_sender: Sender<Message>,
        delayed_log_sender: Sender<DelayedMessage>,
    ) -> Self {
        let cryptography = match config.cryptography {
            Some(cryptography) => Some(Cryptography::new(cryptography)),
            _ => None,
        };

        #[cfg(feature = "aws")]
        let aws = match config.aws {
            Some(aws) => Some(Aws::new(aws).await),
            _ => None,
        };

        #[cfg(feature = "gcp")]
        let gcp = match config.gcp {
            Some(gcp) => Some(Gcp::new(gcp).await),
            _ => None,
        };

        let blockchain = match config.blockchain {
            Some(blockchain) => Some(Blockchain::new(blockchain)),
            _ => None,
        };

        let general = match config.general {
            Some(gc) => Some(General::new(gc, log_sender, delayed_log_sender)),
            _ => None,
        };

        let github = match config.github {
            Some(gh) => Some(Github::new(gh)),
            _ => None,
        };

        let jira = match config.jira {
            Some(j) => match Jira::new(j) {
                Ok(jira) => Some(jira),
                Err(e) => {
                    error!("Something went wrong while initializing the Jira API: proceeding without. This should be investigated! The error was {e}");
                    None
                }
            },
            _ => None,
        };

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

        let okta = match config.okta {
            Some(oc) => Some(Okta::new(oc)),
            _ => None,
        };

        let pagerduty = match config.pagerduty {
            Some(pd) => Some(PagerDuty::new(pd)),
            _ => None,
        };

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
            #[cfg(feature = "aws")]
            aws,
            #[cfg(feature = "gcp")]
            gcp,
            blockchain,
            cryptography,
            general,
            github,
            jira,
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

#[derive(PartialEq, PartialOrd, Debug)]
/// Represents an access scope for a rule
enum AccessScope {
    Read,
    Write,
}
