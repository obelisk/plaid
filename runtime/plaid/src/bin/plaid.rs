#[macro_use]
extern crate log;

use futures_util::{Stream, StreamExt};
use jsonwebtoken::crypto::{self, CryptoProvider};
use performance::ModulePerformanceMetadata;
use plaid::{
    apis::ApiError,
    cache::Cache,
    config::{
        CachingMode, ConfigurationWithRoles, GetMode, ResponseMode, WebhookConfig,
        WebhookServerConfiguration,
    },
    loader::PlaidModule,
    logging::Logger,
    *,
};

use apis::Api;
use data::Data;
use executor::*;
use plaid_stl::messages::LogSource;
use storage::Storage;
use tokio::{
    signal::{
        self,
        unix::{signal, SignalKind},
    },
    spawn,
};
use tokio::{sync::RwLock, task::JoinSet};
use tokio_util::{bytes::Buf, sync::CancellationToken};

use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::TrySendError;
use warp::{
    http::{HeaderMap, StatusCode},
    path, Filter,
};

#[derive(Debug)]
enum Errors {
    FailedToStartApiSystem(ApiError),
    FailedToLoadModules,
}

impl std::fmt::Display for Errors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Errors::FailedToStartApiSystem(e) => write!(f, "Failed to start API system: {:?}", e),
            Errors::FailedToLoadModules => write!(f, "Failed to load modules"),
        }
    }
}

impl std::error::Error for Errors {}

async fn post_handler(
    webhook: String,
    body: impl Stream<Item = Result<impl Buf, warp::Error>> + Unpin + Send + Sync,
    headers: HeaderMap,
    webhooks: HashMap<String, WebhookConfig>,
    exec: Arc<Executor>,
) -> impl warp::Reply {
    // If this is a webhook that is configured
    if let Some(webhook_configuration) = webhooks.get(&webhook) {
        // If the webhook has a label, use that as the source, otherwise use the webhook address
        let source = match webhook_configuration.label {
            Some(ref label) => LogSource::WebhookPost(label.to_string()),
            None => LogSource::WebhookPost(webhook.to_string()),
        };

        let logbacks_allowed = webhook_configuration.logbacks_allowed.clone();

        // Read the body with size limit
        let full_body = match read_body_with_limit(body, webhook_configuration.max_body_size).await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Error reading body for webhook: {webhook}: {e}");
                // We still return a 200 to avoid leaking information
                return Box::new(warp::reply());
            }
        };

        // Create the message we're going to send into the execution system.
        let mut message = Message::new(
            webhook_configuration.log_type.to_owned(),
            full_body,
            source,
            logbacks_allowed,
        );

        for requested_header in webhook_configuration.headers.iter() {
            // TODO: Investigate if this should be get_all?
            // Without this we don't support receiving multiple headers with the same name
            // I don't know if this is an issue or not, practically, or if there are security implications.
            if let Some(value) = headers.get(requested_header) {
                message
                    .headers
                    .insert(requested_header.to_string(), value.as_bytes().to_vec());
            }
        }

        // Webhook exists, buffer log
        if let Err(e) = exec.execute_webhook_message(message) {
            match e {
                TrySendError::Full(_) => error!(
                    "Queue Full! [{}] log dropped!",
                    webhook_configuration.log_type
                ),
                // TODO: Have this actually cause Plaid to exit
                TrySendError::Disconnected(_) => panic!(
                    "The execution system is no longer accepting messages. Nothing can continue."
                ),
            }
        }
    }
    // Always Empty Response
    Box::new(warp::reply())
}

/// Read the body of a request with a maximum size limit
async fn read_body_with_limit(
    mut body: impl Stream<Item = Result<impl Buf, warp::Error>> + Unpin,
    max_size: usize,
) -> Result<Vec<u8>, String> {
    // Keep a vector of references to avoid doing too many allocations
    // before doing a final copy into a single buffer
    let mut buffers = Vec::new();
    // We reserve space for 32 chunk pointers to also avoid reallocating this pointer
    // buffer
    buffers.reserve(32);
    let mut total_bytes_count = 0usize;

    // Read a maximum of max_size from the request
    // I'm trying to find a source to get proof that this
    // next() call is not going to read possibly gigabytes into memory but for now
    // I'm going to trust it.
    while let Some(buf) = body.next().await {
        match buf {
            Ok(buf) => {
                // Immediately exit if this chunk is going to exceed our maximum allowed size
                if buf.remaining() + total_bytes_count > max_size {
                    return Err(format!(
                        "Body exceeded maximum allowed size of {max_size} bytes"
                    ));
                }
                // Consider these bytes read
                total_bytes_count += buf.remaining();

                // Get all the pieces of this buffer into our vec of vecs
                buffers.push(buf);
            }
            Err(e) => {
                return Err(format!("Error reading body: {e}"));
            }
        }
    }

    let mut full_body = Vec::with_capacity(total_bytes_count);

    let total_buffers = buffers.len();
    let mut total_chunks = 0;
    for mut buffer in buffers {
        while buffer.remaining() > 0 {
            let chunk = buffer.chunk();
            let chunk_len = chunk.len();
            full_body.extend_from_slice(chunk);
            buffer.advance(chunk_len);
            total_chunks += 1;
        }
    }
    trace!(
        "Read {total_bytes_count} bytes from webhook body across {total_buffers} buffers and {total_chunks} chunks"
    );

    Ok(full_body)
}

fn probe_routes(
    is_ready: Arc<AtomicBool>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let ready = warp::path!("ready").and(warp::get()).map(move || {
        if is_ready.load(Ordering::SeqCst) {
            warp::reply::with_status("ready", StatusCode::OK)
        } else {
            warp::reply::with_status("not ready", StatusCode::SERVICE_UNAVAILABLE)
        }
    });

    let live = warp::path!("live")
        .and(warp::get())
        .map(|| warp::reply::with_status("live", StatusCode::OK));

    ready.or(live).unify()
}

async fn wait_for_shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");

    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("SIGINT received");
        }

        _ = sigterm.recv() => {
            info!("SIGTERM received");
        }
    }
}

fn log_join_result(task_type: &str, result: Result<(), tokio::task::JoinError>) {
    if let Err(e) = result {
        error!("{task_type} task failed during shutdown: {e}");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install default rustls crypto provider");

    // Manually specify we are going to use RustCrypto.
    CryptoProvider::install_default(&crypto::rust_crypto::DEFAULT_PROVIDER)
        .expect("Failed to install RustCrypto provider for JWT signing");

    info!("Plaid is booting up, please standby...");

    info!("Reading configuration");
    let ConfigurationWithRoles { config, roles } = config::configure()?;
    info!("This is what this instance is running: {roles:?}");

    // Create thread pools for log execution
    let exec_thread_pools = thread_pools::ExecutionThreadPools::new(&config.executor);

    // For convenience, keep a sender for the general channel, so that we can quickly clone it around
    let log_sender = exec_thread_pools.general_pool.sender.clone();

    info!("Starting logging subsystem");
    let (els, logging_handler) = Logger::start(config.logging);
    info!("Logging subsystem started");

    // Create the storage system if one is configured
    let storage = match config.storage {
        Some(config) => {
            info!("Storage system configured");
            match config.db {
                None => {
                    info!("No DB configured");
                }
                #[cfg(feature = "aws")]
                Some(storage::DatabaseConfig::DynamoDb(_)) => {
                    info!("Using DynamoDB");
                }
                #[cfg(feature = "sled")]
                Some(storage::DatabaseConfig::Sled(_)) => {
                    info!("Using Sled");
                }
                Some(storage::DatabaseConfig::InMemory) => {
                    info!("Using an in-memory DB");
                    warn!(
                        "!!! This is just an in-memory storage and is NOT persisted across reboots !!!"
                    )
                }
            }
            let s = Arc::new(Storage::new(config).await?);
            match &s.shared_dbs {
                None => info!("No shared DBs configured"),
                Some(dbs) => {
                    info!(
                        "Configured shared DBs: {:?}",
                        dbs.keys().collect::<Vec<&String>>()
                    );
                }
            }
            Some(s)
        }
        None => {
            info!("No persistent storage system configured; unexecuted log backs will be lost on shutdown");
            None
        }
    };

    // The internal system always gets a storage: if we don't have a persistent one, we create an in-memory one
    let internal_storage = match &storage {
        Some(s) => s.clone(),
        None => Arc::new(Storage::new_in_memory()),
    };

    // Graceful shutdown handling
    let cancellation_token = CancellationToken::new();
    let performance_cancellation_token = CancellationToken::new();
    let is_ready = Arc::new(AtomicBool::new(false));
    let mut server_tasks = JoinSet::new();

    if let Some(probe_listen_address) = config.loading.probe_listen_address.clone() {
        let routes = probe_routes(is_ready.clone());

        info!("Started probe server at: {probe_listen_address}");
        spawn(async move {
            warp::serve(routes).bind(probe_listen_address).await;
            error!("Probe server running at [{probe_listen_address}] shut down");
        });
    } else {
        warn!("No probe_listen_address configured; readiness and liveness endpoints are disabled");
    }

    let (performance_sender, performance_handle) = match config.performance_monitoring {
        Some(perf) => {
            warn!("Plaid is running with performance monitoring enabled - this is NOT recommended for production deployments. Metadata about rule execution will be logged to a channel that aggregates and reports metrics.");
            let (sender, rx) = crossbeam_channel::bounded::<ModulePerformanceMetadata>(4096);

            let token = performance_cancellation_token.clone();
            let handle = tokio::task::spawn(async move {
                perf.start(rx, token).await;
            });

            (Some(sender), Some(handle))
        }
        None => (None, None),
    };

    info!("Loading all the modules");
    // Load all the modules that form our Nanoservices and Plaid rules
    let modules = Arc::new(
        loader::load(&config.loading, storage.clone())
            .await
            .map_err(|_| Errors::FailedToLoadModules)?,
    );
    let modules_by_name = Arc::new(modules.get_modules());

    let modules_and_logtypes = modules.get_module_logtypes();

    let cache = Cache::new(modules_and_logtypes, config.cache).await?;
    let cache = Arc::new(cache);

    // Print information about the threads we are starting
    info!(
        "Starting {} execution threads for general execution. Log queue size = {}",
        exec_thread_pools.general_pool.num_threads,
        exec_thread_pools
            .general_pool
            .sender
            .capacity()
            .unwrap_or_default()
    );
    for (log_type, tp) in &exec_thread_pools.dedicated_pools {
        let thread_or_threads = if tp.num_threads == 1 {
            "thread"
        } else {
            "threads"
        };
        info!("Starting {} {thread_or_threads} dedicated to log type [{log_type}]. Log queue size = {}", tp.num_threads, tp.sender.capacity().unwrap_or_default());
    }
    // This sender provides an internal route to sending logs. This is what
    // powers the logback functions.
    let (delayed_log_sender, delayed_log_persister, mut dg_tasks) = Data::start(
        config.data,
        log_sender.clone(),
        internal_storage.clone(),
        els.clone(),
        &roles,
        cancellation_token.clone(),
    )
    .await?;
    info!("Configuring APIs for Modules");
    // Create the API that powers all the wrapped calls that modules can make
    let api = Api::new(config.apis)
        .await
        .map_err(|e| Errors::FailedToStartApiSystem(e))?;

    // Create an Arc so all the handlers have access to our API object
    let api = Arc::new(api);

    // Workers upgrade this weak ref per message so idle threads hold no Message senders.
    let immediate_dispatch = Arc::new(exec_thread_pools.general_pool.sender.clone());

    // Create the executor that will handle all the logs that come in and immediate
    // requests for handling some configured get requests.
    let (executor, executor_threads) = Executor::new(
        exec_thread_pools.clone(),
        modules.get_channels(),
        api,
        storage,
        Some(cache),
        els.clone(),
        performance_sender.clone(),
        Arc::downgrade(&immediate_dispatch),
        delayed_log_sender.clone(),
        cancellation_token.clone(),
    );

    let executor = Arc::new(executor);

    if roles.webhooks {
        info!("Configured Webhook Servers");
        for (server_name, config) in config.webhooks {
            let server_address: SocketAddr = config
                .listen_address
                .parse()
                .expect("A server had an invalid address");

            let webhooks = config.webhooks.clone();
            let exec = executor.clone();
            let post_route = warp::post()
                .and(path!("webhook" / String))
                .and(warp::body::stream())
                .and(warp::header::headers_cloned())
                .and(with(webhooks))
                .and(with(exec.clone()))
                .then(post_handler);

            // This is a cache for get requests that are configured to be cached
            // Webhook -> (timestamp, response)
            let get_cache: Arc<RwLock<HashMap<String, (u64, String)>>> =
                Arc::new(RwLock::new(HashMap::new()));
            let webhook_server_get_log_sender = log_sender.clone();
            let webhook_config = Arc::new(config);
            let get_route = warp::get()
                .and(path!("webhook" / String))
                .and(warp::query::<HashMap<String, String>>())
                .and(warp::body::stream())
                .and(warp::header::headers_cloned())
                .and(with(webhook_config.clone()))
                .and(with(modules_by_name.clone()))
                .and(with(get_cache.clone()))
                .and(with(webhook_server_get_log_sender.clone()))
                .and_then(|webhook: String, query: HashMap<String, String>, body, headers: HeaderMap, webhook_config: Arc<WebhookServerConfiguration>, modules: Arc<HashMap<String, Arc<PlaidModule>>>, get_cache: Arc<RwLock<HashMap<String, (u64, String)>>>, log_sender: crossbeam_channel::Sender<Message>| async move {
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
                                    }
                                };

                                // If the webhook has a label, use that as the source, otherwise use the webhook address
                                let source = match webhook_configuration.label {
                                    Some(ref label) => LogSource::WebhookGet(label.to_string()),
                                    None => LogSource::WebhookGet(webhook.to_string()),
                                };

                                let logbacks_allowed = webhook_configuration.logbacks_allowed.clone();

                                let (response_send, response_recv) = tokio::sync::oneshot::channel();

                                // Read the body with size limit
                                let body_bytes = match read_body_with_limit(body, webhook_configuration.max_body_size).await {
                                    Ok(bytes) => bytes,
                                    Err(e) => {
                                        error!("Error reading body for get request to {webhook}: {e}");
                                        return Ok(warp::reply::html(String::new()));
                                    }
                                };

                                // Construct a message to send to the rule
                                let mut message = Message::new_detailed(
                                    name.to_string(),
                                    body_bytes,
                                    source,
                                    logbacks_allowed,
                                    query.into_iter().map(|(k, v)| (k, v.into_bytes())).collect(),
                                    Some(response_send),
                                    Some(rule.clone()));

                                // Configure headers
                                for requested_header in webhook_configuration.headers.iter() {
                                    if let Some(value) = headers.get(requested_header) {
                                        message.headers.insert(requested_header.to_string(), value.as_bytes().to_vec());
                                    }
                                }

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
                                            // I'm making the assumption here that getting the system time will never fail
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
            let token = cancellation_token.clone();
            server_tasks.spawn(async move {
                let (_, server) =
                    warp::serve(routes).bind_with_graceful_shutdown(server_address, async move {
                        token.cancelled().await;
                    });

                server.await;
                info!("Web server [{server_name}] shut down");
            });
        }
    } else {
        info!("This instance is NOT running webhooks");
    }

    info!("Starting servers, boot up complete");
    is_ready.store(true, Ordering::SeqCst);

    // Block until SIGINT/SIGTERM, or until a server/data generator task exits unexpectedly.
    tokio::select! {
        _ = wait_for_shutdown_signal() => {}

        server_result = server_tasks.join_next(), if !server_tasks.is_empty() => {
            warn!("A server task exited before a shutdown signal was received");
            if let Some(result) = server_result {
                log_join_result("webhook server", result);
            }
        }

        dg_result = dg_tasks.join_next(), if !dg_tasks.is_empty() => {
            warn!("A data generator task exited before a shutdown signal was received");
            if let Some(result) = dg_result {
                log_join_result("data generator", result);
            }
        }
    }

    // Tell the orchestrator to stop routing traffic before we tear down producers.
    is_ready.store(false, Ordering::SeqCst);
    info!("Sending cancellation notice to all listening tasks.");
    cancellation_token.cancel();

    // Data generators are the main source of new logs; wait for them to exit first.
    info!("Waiting for data generators to shutdown...");
    let mut dg_tasks = dg_tasks;
    while let Some(result) = dg_tasks.join_next().await {
        log_join_result("data generator", result);
    }

    // Webhook/probe servers stop accepting new requests once cancelled; join any in-flight work.
    info!("Waiting for server tasks to shutdown...");
    while let Some(result) = server_tasks.join_next().await {
        log_join_result("webhook server", result);
    }

    // Drop every Sender<Message> so worker threads exit once the queues drain.
    info!("Waiting for executor threads to drain...");
    drop(log_sender);
    drop(exec_thread_pools);
    drop(immediate_dispatch);
    drop(executor);
    executor_threads.join();

    // Persist any delayed logbacks still in the in-memory channel.
    info!("Flushing delayed logbacks to storage...");
    delayed_log_persister.flush_pending().await;
    drop(delayed_log_sender);

    // Performance loop exits when cancelled and its sender disconnects.
    drop(performance_sender);
    performance_cancellation_token.cancel();

    if let Some(handle) = performance_handle {
        info!("Waiting for performance monitoring system to shutdown...");
        // Await here so the metrics report is written before we exit.
        if let Err(e) = handle.await {
            error!("Performance monitoring task failed during shutdown: {e}");
        }
    }

    // Executor threads hold Logger clones; drop ours so the logging channel can disconnect.
    drop(els);
    if let Err(e) = logging_handler.join() {
        error!("Logging thread panicked during shutdown: {e:?}");
    }

    info!("Plaid shutdown complete.");
    Ok(())
}

fn with<T>(users: T) -> impl Filter<Extract = (T,), Error = Infallible> + Clone
where
    T: Send + Sync + Clone,
{
    warp::any().map(move || users.clone())
}
