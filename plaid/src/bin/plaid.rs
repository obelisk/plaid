#[macro_use]
extern crate log;

use plaid::{config::{CachingMode, GetMode, ResponseMode, WebhookServerConfiguration}, loader::PlaidModule, logging::Logger, *};

use apis::Api;
use data::Data;
use executor::*;
use plaid_stl::messages::LogSource;
use reqwest::header::HeaderMap;
use storage::Storage;
use tokio::{sync::RwLock, task::JoinSet};

use std::{collections::HashMap, convert::Infallible, net::SocketAddr, pin::Pin, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use crossbeam_channel::{bounded, TrySendError};
use warp::{hyper::body::Bytes, path, Filter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    info!("Plaid is booting up, please standby...");

    info!("Reading configuration");
    let config = config::configure()?;
    let (log_sender, log_receiver) = bounded(config.log_queue_size);

    info!("Starting logging subsystem");
    let (els, _logging_handler) = Logger::start(config.logging);
    info!("Logging subsystem started");

    // Create the storage system is one is configured
    let storage = match config.storage {
        Some(config) => Some(Arc::new(Storage::new(config)?)),
        None => None,
    };

    if storage.is_some() {
        info!("Storage system configured");
    } else {
        info!(
            "No persistent storage system configured; unexecuted log backs will be lost on shutdown"
        );
    }

    // This sender provides an internal route to sending logs. This is what
    // powers the logback functions.

    let delayed_log_sender = Data::start(config.data, log_sender.clone(), storage.clone(), els.clone())
        .await
        .expect("The data system failed to start")
        .unwrap();

    info!("Configurating APIs for Modules");
    // Create the API that powers all the wrapped calls that modules can make
    let api = Api::new(config.apis, log_sender.clone(), delayed_log_sender).await;

    // Create an Arc so all the handlers have access to our API object
    let api = Arc::new(api);

    info!("Loading all the modules");
    // Load all the modules that form our Nanoservices and Plaid rules
    let modules = Arc::new(loader::load(config.loading).unwrap());
    let modules_by_name = Arc::new(modules.get_modules());

    info!(
        "Starting the execution threads of which {} were requested",
        config.execution_threads
    );

    // Create the executor that will handle all the logs that come in and immediate
    // requests for handling some configured get requests.
    let executor = Executor::new(
        log_receiver,
        modules.get_channels(),
        api,
        storage,
        config.execution_threads,
        els.clone()
    );

    let _executor = Arc::new(executor);

    info!("Configured Webhook Servers");
    let webhook_server_post_log_sender = log_sender.clone();
    let webhook_servers: Vec<Box<Pin<Box<_>>>> = config
        .webhooks
        .into_iter()
        .map(|(server_name, config)| {
            let webhook_server_post_log_sender = webhook_server_post_log_sender.clone();
            let server_address: SocketAddr = config
                .listen_address
                .parse()
                .expect("A server had an invalid address");

            let webhooks = config.webhooks.clone();
            let post_route = warp::post()
                .and(warp::body::content_length_limit(1024 * 256))
                .and(path!("webhook" / String))
                .and(warp::body::bytes())
                .and(warp::header::headers_cloned())
                .map(move |webhook: String, data: Bytes, headers: HeaderMap| {
                    // If this is a webhook that is configured
                    if let Some(webhook_configuration) = webhooks.get(&webhook) {

                        // If the webhook has a label, use that as the source, otherwise use the webhook address
                        let source = match webhook_configuration.label {
                            Some(ref label) => LogSource::WebhookPost(label.to_string()),
                            None => LogSource::WebhookPost(webhook.to_string()),
                        };

                        let logbacks_allowed = webhook_configuration.logbacks_allowed.clone();

                        // Create the message we're going to send into the execution system.
                        let mut message = Message::new(webhook_configuration.log_type.to_owned(), data[..].to_vec(), source, logbacks_allowed);

                        for requested_header in webhook_configuration.headers.iter() {
                            // TODO: Investigate if this should be get_all?
                            // Without this we don't support receiving multiple headers with the same name
                            // I don't know if this is an issue or not, practicially, or if there are security implications.
                            if let Some(value) = headers.get(requested_header) {
                                message.accessory_data.insert(requested_header.to_string(), value.as_bytes().to_vec());
                            }
                        }

                        // Webhook exists, buffer log
                        if let Err(e) = webhook_server_post_log_sender.try_send(message) {
                            match e {
                                TrySendError::Full(_) => error!("Queue Full! [{}] log dropped!", webhook_configuration.log_type),
                                // TODO: Have this actually cause Plaid to exit
                                TrySendError::Disconnected(_) => panic!("The execution system is no longer accepting messages. Nothing can continue."),
                            }
                        }
                    }
                    // Always Empty Response
                    Box::new(warp::reply())
                });

            // This is a cache for get requests that are configured to be cached
            // Webhook -> (timestamp, response)
            let get_cache: Arc<RwLock<HashMap<String, (u64, String)>>> = Arc::new(RwLock::new(HashMap::new()));
            let webhook_server_get_log_sender = log_sender.clone();
            let webhook_config = Arc::new(config);
            let get_route = warp::get()
                .and(path!("webhook" / String))
                .and(warp::query::<HashMap<String, String>>())
                .and(with(webhook_config.clone()))
                .and(with(modules_by_name.clone()))
                .and(with(get_cache.clone()))
                .and(with(webhook_server_get_log_sender.clone()))
                .and_then(|webhook: String, query: HashMap<String, String>, webhook_config: Arc<WebhookServerConfiguration>, modules: Arc<HashMap<String, Arc<PlaidModule>>>, get_cache: Arc<RwLock<HashMap<String, (u64, String)>>>, log_sender: crossbeam_channel::Sender<Message>| async move {
                    if let Some(webhook_configuration) = webhook_config.webhooks.get(&webhook) {
                        match &webhook_configuration.get_mode {
                            // Note that CacheMode is elided here as there is no caching for static data
                            Some(GetMode{ response_mode: ResponseMode::Static(data), ..}) => {
                                Ok(warp::reply::html(data.clone()))
                            }
                            // Note that CacheMode is elided here as there is no caching possible for
                            // Facebook verification
                            Some(GetMode{ response_mode: ResponseMode::Facebook(secret), ..}) => {
                                if let Some(fb_secret) = query.get("hub.verify_token") {
                                    if fb_secret == secret {
                                        info!("Received a valid get request to: {webhook}");
                                        Ok::<warp::reply::Html<String>, Infallible>(warp::reply::html(query.get("hub.challenge").unwrap_or(&String::new()).to_owned()))
                                    } else {
                                        error!("Got a request that didn't contain the right FB secret");
                                        Ok(warp::reply::html(String::new()))
                                    }
                                } else {
                                    warn!("Got a call that didn't contain the right FB parameters. Webhook leaked?");
                                    Ok(warp::reply::html(String::new()))
                                }
                            },
                            // For rules, we do need to get the cache mode and dealing with it makes this
                            // kind of reponse significantly more complex.
                            Some(GetMode{ response_mode: ResponseMode::Rule(name), caching_mode}) => {
                                // Ensure that the rule configured to generated the GET response actually exists
                                let rule = if let Some(rule) = modules.get(name) {
                                    rule
                                } else {
                                    warn!("Got a get request to {webhook} but the rule [{name}] configured to handle it does not exist");
                                    return Ok(warp::reply::html(String::new()));
                                };

                                info!("Received get request to: {webhook}. Handling with rule [{name}] to generate response");
                                // I'm making the assumption here that getting the system time will never fail
                                let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

                                // Determine if we need to update the cache at the end of this request 
                                let update = match caching_mode {
                                    CachingMode::Timed{validity} => {
                                        let cache = get_cache.read().await;
                                        if let Some(cached_response) = cache.get(&webhook) {
                                            if cached_response.0 + validity > current_time {
                                                info!("Returning cached response (valid for {} more seconds) for get request to: {webhook}", cached_response.0 + validity - current_time);
                                                return Ok(warp::reply::html(cached_response.1.clone()));
                                            }
                                        }
                                        true
                                    },
                                    CachingMode::None => false,
                                    CachingMode::UsePersistentResponse { call_on_none } => {
                                        match rule.get_persistent_response_data() {
                                            Some(data) => {
                                                // There is persistent data available for this rule so we can just return it
                                                info!("Returning persistent response for get request to: {webhook}");
                                                return Ok(warp::reply::html(data));
                                            },
                                            // There is no persistent data. So we continue with the normal calling system
                                            // if call on none is true but do not cache since "caching" is just the persistent data
                                            None => {
                                                // We don't want to call on none so even though there is no persistent response
                                                // we don't run the rule and just return no data
                                                if !call_on_none {
                                                    return Ok(warp::reply::html(String::new()));
                                                }
                                                false
                                            },
                                        }
                                    }, 
                                };

                                // If the webhook has a label, use that as the source, otherwise use the webhook address
                                let source = match webhook_configuration.label {
                                    Some(ref label) => LogSource::WebhookGet(label.to_string()),
                                    None => LogSource::WebhookGet(webhook.to_string()),
                                };

                                let logbacks_allowed = webhook_configuration.logbacks_allowed.clone();

                                let (response_send, response_recv) = tokio::sync::oneshot::channel();

                                // Construct a message to send to the rule
                                let message = Message {
                                    type_: name.to_string(),
                                    data: String::new().into_bytes(),
                                    accessory_data: query.into_iter().map(|(k, v)| (k, v.into_bytes())).collect(),
                                    source,
                                    logbacks_allowed,
                                    response_sender: Some(response_send),
                                    module: Some(rule.clone()),
                                };

                                // Put the message into the standard message queue
                                if let Err(e) = log_sender.try_send(message) {
                                    match e {
                                        TrySendError::Full(_) => error!("Queue Full! [{}] log dropped!", webhook_configuration.log_type),
                                        // TODO: Have this actually cause Plaid to exit
                                        TrySendError::Disconnected(_) => panic!("The execution system is no longer accepting messages. Nothing can continue."),
                                    }
                                }

                                match response_recv.await {
                                    Ok(Some(response))=> {
                                        if update {
                                            info!("Updating cache for get request to: {webhook}");
                                            let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                                            let mut cache = get_cache.write().await;
                                            cache.insert(webhook.clone(), (current_time, response.body.clone()));
                                        }
                                        Ok(warp::reply::html(response.body))
                                    },
                                    Ok(None) => {
                                        warn!("Got a get request to {webhook} but the rule [{name}] configured to handle it did not return a response");
                                        Ok(warp::reply::html(String::new()))
                                    }
                                    Err(e) => {
                                        error!("Got a get request to {webhook} but the rule [{name}] configured to handle it threw an error: {e}");
                                        Ok(warp::reply::html(String::new()))
                                    }
                                }
                            },
                            None => {
                                // This occurs when a webhook receives a get request but there is no configuration for how
                                // GET requests should be handled. Usually this is the result of a service misconfiguration
                                // where it should be sending POSTs.
                                warn!("Got a get request to {webhook}. Are you sure the sending service is configured correctly?");
                                Ok(warp::reply::html(String::new()))
                            },
                        }
                    } else {
                        Ok(warp::reply::html(String::new()))
                    }
                });

            let routes = post_route.or(get_route);

            info!("Web Server [{server_name}]: {server_address}");
            Box::<Pin<Box<_>>>::new(Box::pin(
                warp::serve(routes).run(server_address),
            ))
        })
        .collect();

    info!("Starting servers, boot up complete");

    let mut join_set = JoinSet::from_iter(webhook_servers);

    while let Some(_) = join_set.join_next().await {}
    //futures::future::join_all(webhook_servers).await;

    Ok(())
}

fn with<T>(users: T) -> impl Filter<Extract = (T,), Error = Infallible> + Clone
where
    T: Send + Sync + Clone,
{
    warp::any().map(move || users.clone())
}