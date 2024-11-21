mod selector;

use crate::{executor::Message, logging::Logger};
use crossbeam_channel::Sender;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use http::{HeaderMap, Uri};
use plaid_stl::messages::{Generator, LogSource, LogbacksAllowed};
use selector::UriSelector;
use serde::Deserialize;
use std::{collections::HashMap, str::FromStr, time::Duration};
use tokio::{net::TcpStream, task::JoinHandle};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, protocol::Message as WSMessage},
    MaybeTlsStream, WebSocketStream,
};

/// Represents errors that a WebSocket data generator can encounter
#[derive(Debug)]
enum Errors {
    SocketCreationFailure,
}

/// Configuration of all WebSocket data generators.
#[derive(Deserialize)]
pub struct WebSocketDataGenerator {
    /// A map of WebSocket configurations, identified by its name.
    websockets: HashMap<String, WebSocket>,
    /// The maximum size of a message received from a socket that will be processed.
    /// If not specified, the default value from `default_message_size` is used (`64 kB`)
    #[serde(default = "default_message_size")]
    max_message_size: usize,
}

/// Returns the default maximum size, in bytes, for a message received from a WebSocket
/// that will be processed. The default value is set to 64 KB (65,536 bytes).
///
/// This default is used if no specific maximum message size is provided in the configuration.
fn default_message_size() -> usize {
    65536
}

/// Represents the configuration for a WebSocket connection.
#[derive(Deserialize)]
pub struct WebSocket {
    /// A map of URIs for the WebSocket endpoint(s). The configuration supports multiple URIs
    /// to allow for failover scenarios. If a connection fails, the system implements exponential
    /// backoff, selecting the URI whose retry period has elapsed. If none are available, the URI
    /// with the shortest remaining retry time is chosen.
    #[serde(deserialize_with = "parse_uris")]
    uris: HashMap<String, Uri>,
    /// A string indicating the type of log associated with the WebSocket.
    log_type: String,
    /// An optional configuration for the message to be sent over the WebSocket connection.
    message_config: Option<SocketMessage>,
    /// An optional field containing a map of headers to be included in the WebSocket request.
    #[serde(deserialize_with = "parse_headers")]
    headers: Option<HeaderMap>,
    /// The number of Logbacks this generator is allowed to trigger
    #[serde(default)]
    logbacks_allowed: LogbacksAllowed,
    /// The minimum amount of time (in milliseconds) to wait before retrying a connection to a WebSocket
    #[serde(default = "min_retry_duration")]
    #[serde(deserialize_with = "parse_duration")]
    min_retry_duration: Duration,
    /// The maximum amount of time (in milliseconds) to wait before retrying a connection to a WebSocket
    #[serde(default = "max_retry_duration")]
    #[serde(deserialize_with = "parse_duration")]
    max_retry_duration: Duration,
}

/// Represents the configuration of a message to be sent over a WebSocket connection.
#[derive(Deserialize)]
struct SocketMessage {
    /// The message content to be sent over the WebSocket.
    /// This could be a command, heartbeat, or any other data that needs to be transmitted to the server.
    message: String,
    /// The duration (in milliseconds) to wait before sending the next message over the WebSocket connection.
    /// This defines the frequency of message dispatches. Typically, you might send periodic messages
    /// to keep the connection alive, monitor connection health, or transmit data at regular intervals.
    sleep_duration: u64,
}

/// The default value for `min_retry_duration` if none is provided in the configuration.
fn min_retry_duration() -> Duration {
    Duration::from_millis(100)
}

/// The default value for `max_retry_duration` if none is provided in the configuration.
fn max_retry_duration() -> Duration {
    Duration::from_millis(60000)
}

/// Custom parser for URI. Returns an error if an invalid URI is provided
fn parse_uris<'de, D>(deserializer: D) -> Result<HashMap<String, Uri>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let uris_raw = <HashMap<String, String>>::deserialize(deserializer)?;

    let uris = uris_raw
        .into_iter()
        .filter_map(|(name, uri)| match Uri::from_str(&uri) {
            Ok(valid_uri) => {
                if let Some(scheme) = valid_uri.scheme() {
                    if scheme != "wss" {
                        warn!(
                            "Insecure protocol detected: [{scheme}] for URI: [{name}]. Consider using 'wss' if possible.",
                        );
                    }
                }

                Some((name, valid_uri))
            },
            Err(e) => {
                error!("Invalid URI provided: {}. Error: {}", uri, e);
                None
            }
        })
        .collect::<HashMap<String, Uri>>();

    if uris.is_empty() {
        Err(serde::de::Error::custom(&format!("No valid URIs provided")))
    } else {
        Ok(uris)
    }
}

/// Custom parser to convert user provided `HashMap<String, String>` of headers to include in the request
/// to a `http::HeaderMap`. Returns an error if the conversion to `HeaderMap` fails.
fn parse_headers<'de, D>(deserializer: D) -> Result<Option<HeaderMap>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let headers: Option<HashMap<String, String>> = Option::deserialize(deserializer)?;

    if let Some(ref headers) = headers {
        match headers.try_into() {
            Ok(map) => Ok(Some(map)),
            Err(e) => Err(serde::de::Error::custom(format!(
                "Invalid headers provided: {e}"
            ))),
        }
    } else {
        Ok(None)
    }
}

/// Custom parser to convert user provided duration (in milliseconds) to a `Duration`.
/// Returns an error if deserialization to `u64` fails.
fn parse_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let duration: u64 = u64::deserialize(deserializer)?;

    Ok(Duration::from_millis(duration))
}

/// A generator that manages multiple WebSocket clients for data generation.
///
/// The `WebsocketGenerator` struct holds a collection of `WebSocketClient` instances and provides
/// methods to initialize and start these clients.
pub struct WebsocketGenerator {
    /// A list of configured data generators that will fetch data from sockets and forward to rules.
    clients: Vec<WebSocketClient>,
    /// Logs unexpected socket drops using Plaid's external logging system.
    /// This data is sent to Splunk and other configured sources to help identify consistently unhealthy sockets.
    logger: Logger,
}

/// Creates a new `WebsocketGenerator` instance with the specified configuration and message sender.
///
/// This function initializes the WebSocket clients based on the provided configuration and returns
/// a `WebsocketGenerator` instance containing these clients.
///
/// # Parameters
/// - `config`: The configuration for the WebSocket data generator, containing a list of WebSocket
///   configurations.
/// - `sender`: A channel sender for sending messages to the executor.
///
/// # Returns
/// A new `WebsocketGenerator` instance.
impl WebsocketGenerator {
    pub fn new(config: WebSocketDataGenerator, sender: Sender<Message>, logger: Logger) -> Self {
        let clients = config
            .websockets
            .into_iter()
            .map(|(name, socket_config)| {
                WebSocketClient::new(socket_config, sender.clone(), name, config.max_message_size)
            })
            .collect();

        Self { clients, logger }
    }

    /// Starts all WebSocket clients managed by this generator.
    ///
    /// This function initializes the WebSocket clients, logs the number of clients being initialized,
    /// and then spawns an asynchronous task for each client. Each task runs in a loop, attempting to
    /// start the client and reopening the connection with a new URI if an error occurs.
    pub async fn start(self) {
        info!(
            "Initializing {} WebSocket data generators...",
            self.clients.len()
        );

        for mut client in self.clients {
            info!("Starting [{}]", client.name);
            let logger = self.logger.clone();
            tokio::spawn(async move {
                loop {
                    // This will only return if an error occurred - indicating that we need to reopen the connection with a new URI
                    let Some(socket_name) = client.start().await else {
                        // If this ever returns None - it means that there are no remaining URIs in the heap. In this case, we can exit as there is no point in
                        // trying to reconnect.
                        return;
                    };

                    logger.log_websocket_dropped(socket_name).unwrap();

                    client.uri_selector.mark_failed();
                }
            });
        }
    }
}

/// Represents a WebSocket client responsible for generating and sending logs.
struct WebSocketClient {
    /// The configuration of the client
    configuration: WebSocket,
    /// The sending channel to send logs to the executor.
    sender: Sender<Message>,
    /// The name of the WebSocket as defined in the configuration.
    name: String,
    /// Manages a list of URI entries and handles the selection and retry logic for connection attempts.
    uri_selector: UriSelector,
    /// The maximum size of a message received from a socket that will be processed.
    /// If not specified, the default value from `default_message_size` is used (`64 kB`)
    max_message_size: usize,
}

impl WebSocketClient {
    /// Establishes a WebSocket connection to the given URI and initializes the struct
    /// with the provided parameters.
    fn new(
        configuration: WebSocket,
        sender: Sender<Message>,
        name: String,
        max_message_size: usize,
    ) -> Self {
        let uri_selector = UriSelector::new(
            configuration.uris.clone(),
            configuration.min_retry_duration,
            configuration.max_retry_duration,
        );

        Self {
            configuration,
            sender,
            name,
            uri_selector,
            max_message_size,
        }
    }

    /// Establishes a WebSocket connection and manages the read and write tasks.
    ///
    /// This function attempts to establish a WebSocket connection using the URI provided by the
    /// `uri_selector`. If the connection is successful, the WebSocket is marked as healthy. The function
    /// then splits the WebSocket into write and read halves and spawns separate tasks to handle
    /// writing messages to and reading messages from the WebSocket. Finally, it waits for these tasks
    /// to complete.
    ///
    /// # Tasks
    /// - **Write Task**: Periodically sends a predefined message to the WebSocket.
    /// - **Read Task**: Reads messages from the WebSocket and forwards them to the executor.
    ///
    /// # Behavior
    /// - If the WebSocket connection is established successfully, the WebSocket is marked as healthy.
    /// - The WebSocket is split into write and read halves.
    /// - Separate asynchronous tasks are spawned to handle writing to and reading from the WebSocket.
    /// - The function waits for both tasks to complete, handling any unexpected terminations.
    async fn start(&mut self) -> Option<String> {
        let Some((uri_name, uri)) = self.uri_selector.next_uri().await else {
            error!("No URIs found in heap for: {}", self.name);
            return None;
        };

        if let Ok(socket) = establish_connection(&uri, &self.configuration.headers).await {
            // Mark the WebSocket as healthy again
            self.uri_selector.reset_failure();

            let (write, read) = socket.split();

            let write_handle = self
                .spawn_write_task(write, uri.clone(), uri_name.clone())
                .await;

            let read_handle = self
                .spawn_read_task(read, self.sender.clone(), uri.clone(), uri_name.clone())
                .await;

            self.await_tasks(write_handle, read_handle, &uri_name).await;
        }

        Some(uri_name)
    }

    /// Spawns a task to periodically send a predefined message to the WebSocket.
    ///
    /// This function creates a task that periodically sends a message to the
    /// WebSocket. The message to be sent and the interval between sends are defined in the
    /// configuration of the current instance. If an error occurs while sending the message, the task
    /// logs the error and terminates.
    ///
    /// # Parameters
    /// - `self`: A reference to the current instance.
    /// - `write`: The write half of the split WebSocket stream.
    /// - `uri`: The URI of the WebSocket.
    ///
    /// # Returns
    /// An optional `JoinHandle` for the spawned task. If no message is configured, it returns `None`.
    ///
    /// # Errors
    /// If an error occurs while sending a message to the WebSocket, the task logs the error and
    /// terminates.
    async fn spawn_write_task(
        &self,
        mut write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, WSMessage>,
        uri: Uri,
        uri_name: String,
    ) -> Option<JoinHandle<()>> {
        if let Some(message_config) = &self.configuration.message_config {
            let socket_msg = message_config.message.clone();
            let sleep_duration = message_config.sleep_duration;

            Some(tokio::spawn(async move {
                loop {
                    if let Err(e) = write.send(WSMessage::Text(socket_msg.clone())).await {
                        error!("Failed to send message to WS: [{uri_name}] at [{uri}]. Error: {e}",);
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(sleep_duration)).await;
                }
            }))
        } else {
            None
        }
    }

    /// Spawns a task to read messages from the WebSocket and process them.
    ///
    /// This function creates an asynchronous task that reads messages from the WebSocket. For each
    /// message read, it creates a log message and sends it to a specified channel. If an error occurs
    /// while reading from the WebSocket, the task logs the error and terminates.
    ///
    /// # Parameters
    /// - `self`: A reference to the current instance.
    /// - `read`: The read half of the split WebSocket stream.
    /// - `sender`: A channel sender to which log messages are sent.
    /// - `uri`: The URI of the WebSocket.
    ///
    /// # Returns
    /// A `JoinHandle` for the spawned task.
    ///
    /// # Errors
    /// If an error occurs while reading a message from the WebSocket or sending a log message to the
    /// channel, the task logs the error and terminates.
    async fn spawn_read_task(
        &self,
        mut read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
        sender: Sender<Message>,
        uri: Uri,
        uri_name: String,
    ) -> JoinHandle<()> {
        let generator_name = self.name.clone();
        let log_type = self.configuration.log_type.clone();
        let log_source = LogSource::Generator(Generator::WebSocketExternal(generator_name.clone()));
        let logbacks_allowed = self.configuration.logbacks_allowed.clone();
        let max_message_size = self.max_message_size.clone();

        tokio::spawn(async move {
            while let Some(message) = read.next().await {
                match message {
                    Ok(msg) => {
                        if msg.len() > max_message_size {
                            warn!("Message of size {} bytes exceeded the maximum allowed size of {max_message_size} bytes and was not processed.", msg.len());
                            continue;
                        }

                        let log_message = Message::new(
                            log_type.clone(),
                            msg.into_data(),
                            log_source.clone(),
                            logbacks_allowed.clone(),
                        );

                        if sender.send(log_message).is_err() {
                            error!("Failed to send log to executor");
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to read from WebSocket: [{uri_name}] at [{uri}]. Error: {e}"
                        );
                        return;
                    }
                }
            }
        })
    }

    /// Waits for the read and write tasks to complete, handling any unexpected terminations.
    ///
    /// This function waits for the completion of the read and write tasks. If either task terminates
    /// unexpectedly, it logs an error message. If no write task is spawned, it only waits for the
    /// read task.
    ///
    /// # Parameters
    /// - `self`: A reference to the current instance.
    /// - `write_handle`: An optional `JoinHandle` for the write task.
    /// - `read_handle`: A `JoinHandle` for the read task.
    ///
    /// # Behavior
    /// - If both write and read tasks are provided, it waits for both tasks to complete.
    /// - If only the read task is provided, it waits for the read task to complete.
    /// - Logs an error message if either task finishes unexpectedly.
    async fn await_tasks(
        &self,
        write_handle: Option<JoinHandle<()>>,
        read_handle: JoinHandle<()>,
        uri_name: &str,
    ) {
        match write_handle {
            Some(write_handle) => {
                tokio::select! {
                    _ = write_handle => {
                        error!("Write task for WebSocket: [{}] using socket [{}] finished unexpectedly", &self.name, uri_name);
                    },
                    _ = read_handle => {
                        error!("Read task for WebSocket: [{}] using socket [{}] finished unexpectedly", &self.name, uri_name);
                    },
                }
            }
            None => {
                tokio::select! {
                    _ = read_handle => {
                        error!("Read task for WebSocket: [{}] using socket [{}] finished unexpectedly", &self.name, uri_name);
                    },
                }
            }
        }
    }
}

/// Establishes a WebSocket connection to the specified URI with optional custom headers.
///
/// This function attempts to establish a WebSocket connection to the given URI using `connect_async`.
/// If successful, it returns the WebSocket stream. If the connection attempt fails, it logs an error
/// message and returns `Errors::SocketCreationFailure`. Optionally, custom headers can be provided
/// to be included in the connection request.
///
/// # Parameters
/// - `uri`: A reference to the URI of the WebSocket.
/// - `user_configured_headers`: An optional reference to a hashmap containing custom headers to be included in the request.
///
/// # Returns
/// A `Result` containing the WebSocket stream on success, or an `Errors` enum variant on failure.
///
/// # Errors
/// - `Errors::SocketCreationFailure`: Returned if the connection attempt fails.
/// - `Errors::MisconfiguredHeaders`: Returned if the provided headers are misconfigured.
///
/// # Note
/// If `user_configured_headers` contains headers required for WebSocket connections (e.g., `sec-websocket-key`),
/// they will be overwritten with the user-provided values, which may cause the request to fail.
async fn establish_connection(
    uri: &Uri,
    user_configured_headers: &Option<HeaderMap>,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Errors> {
    let mut request = uri
        .into_client_request()
        .map_err(|_| Errors::SocketCreationFailure)?;

    if let Some(headers) = user_configured_headers {
        let request_headers = request.headers_mut();
        for (key, value) in headers {
            request_headers.entry(key).or_insert(value.clone());
        }
    }

    let (socket, _) = connect_async(request).await.map_err(|e| {
        error!("Failed to establish connection to [{uri}]. Error: {e}");
        Errors::SocketCreationFailure
    })?;

    Ok(socket)
}
