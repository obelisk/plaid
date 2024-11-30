pub mod github;
mod internal;
mod interval;
mod okta;
mod webhook;
mod websocket;

use crate::{
    executor::Message,
    logging::Logger,
    storage::{Storage, StorageError},
};

use std::{pin::Pin, sync::Arc, time::Duration};

use crossbeam_channel::Sender;

use serde::Deserialize;
use tokio::task::{AbortHandle, JoinError, JoinSet};

pub use self::internal::DelayedMessage;

// Configure data sources that Plaid will use fetch data itself and
// send to modules
#[derive(Deserialize)]
pub struct DataConfig {
    github: Option<github::GithubConfig>,
    okta: Option<okta::OktaConfig>,
    internal: Option<internal::InternalConfig>,
    interval: Option<interval::IntervalConfig>,
    websocket: Option<websocket::WebSocketDataGenerator>,
    webhooks: webhook::WebhookServerConfigurations,
}

struct DataInternal {
    github: Option<github::Github>,
    okta: Option<okta::Okta>,
    // Perhaps in the future there will be a reason to explicitly disallow
    // sending logs from one rule to another but for now we keep it always
    // enabled.
    internal: internal::Internal,
    /// Interval manages tracking and execution of jobs that are executed on a defined interval
    interval: Option<interval::Interval>,
    /// Websocket manages the creation and maintenance of WebSockets that provide data to the executor
    websocket_external: Option<websocket::WebsocketGenerator>,
    /// The configuration of webhooks attached to the various servers
    webhooks: Vec<Box<Pin<Box<dyn futures_util::Future<Output = ()> + Send>>>>,
}

pub struct Data {}

#[derive(Debug)]
pub enum DataError {
    StorageError(StorageError),
    TokioRuntimeError(JoinError),
    UnableToConfigureDataSystem,
}

impl DataInternal {
    async fn new(
        config: DataConfig,
        logger: Sender<Message>,
        storage: Option<Arc<Storage>>,
        els: Logger,
    ) -> Result<Self, DataError> {
        let github = config
            .github
            .map(|gh| github::Github::new(gh, logger.clone()));

        let okta = config
            .okta
            .map(|okta| okta::Okta::new(okta, logger.clone()));

        let internal = match config.internal {
            Some(internal) => {
                internal::Internal::new(internal, logger.clone(), storage.clone()).await
            }
            None => {
                internal::Internal::new(
                    internal::InternalConfig::default(),
                    logger.clone(),
                    storage.clone(),
                )
                .await
            }
        };

        let interval = config
            .interval
            .map(|config| interval::Interval::new(config, logger.clone()));

        let websocket_external = config
            .websocket
            .map(|ws| websocket::WebsocketGenerator::new(ws, logger.clone(), els));

        let webhooks = webhook::configure_webhooks(config.webhooks, logger.clone());

        Ok(Self {
            github,
            okta,
            internal: internal?,
            interval,
            websocket_external,
            webhooks,
        })
    }
}

impl Data {
    pub async fn start(
        config: DataConfig,
        sender: Sender<Message>,
        storage: Option<Arc<Storage>>,
        els: Logger,
    ) -> Result<(JoinSet<()>, Sender<DelayedMessage>), DataError> {
        let mut data_internal = DataInternal::new(config, sender, storage, els).await?;

        // Collect all the abort handles to shut the tasks down when Plaid shuts down
        let mut data_generator_join_handles = JoinSet::new();

        for webhook_server in data_internal.webhooks {
            data_generator_join_handles.spawn(webhook_server);
        }

        // Start the Github Audit task if there is one
        if let Some(mut gh) = data_internal.github {
            data_generator_join_handles.spawn(async move {
                loop {
                    if let Err(e) = gh.fetch_audit_logs().await {
                        error!("GitHub Data Fetch Error: {}", e)
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
        }

        // Start the Okta System Logs task if there is one
        if let Some(mut okta) = data_internal.okta {
            data_generator_join_handles.spawn(async move {
                loop {
                    if let Err(e) = okta.fetch_system_logs().await {
                        error!("Okta Data Fetch Error: {:?}", e)
                    }

                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
        }

        // Start the interval job processor
        if let Some(mut interval) = data_internal.interval {
            data_generator_join_handles.spawn(async move {
                loop {
                    let time_until_next_execution = interval.fetch_interval_jobs().await;
                    tokio::time::sleep(Duration::from_secs(time_until_next_execution)).await;
                }
            });
        }

        let internal_sender = (&data_internal.internal).get_sender();
        // Start the internal log processor. This doesn't need to be a tokio task,
        // but we make it one incase we need the runtime in the future. Perhaps it
        // will make sense to convert it to a standard thread but I don't see a benefit
        // to that now. As long as we don't block.
        data_generator_join_handles.spawn(async move {
            loop {
                if let Err(e) = data_internal.internal.fetch_internal_logs().await {
                    error!("Internal Data Fetch Error: {:?}", e)
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });

        if let Some(websocket) = data_internal.websocket_external {
            data_generator_join_handles.spawn(async move {
                websocket.start().await;
            });
        }

        info!("Started Data Generators");
        Ok((data_generator_join_handles, internal_sender))
    }
}
