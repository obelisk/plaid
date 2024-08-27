use http::Uri;
use std::{
    collections::{BinaryHeap, HashMap},
    time::{Duration, Instant},
};
use tokio::time::sleep_until;

/// Represents a single URI entry with backoff duration and next attempt time.
///
/// The `UriEntry` struct contains information about a URI, including the duration to wait before
/// retrying a connection (`backoff_duration`) and the time when the next connection attempt
/// should be made.
#[derive(Debug, Eq, PartialEq)]
struct UriEntry {
    name: String,
    /// The URI to connect to.
    uri: Uri,
    /// The duration to wait before retrying a connection.
    backoff_duration: Duration,
    /// The time when the next connection attempt should be made.
    next_attempt: Instant,
}

/// Manages a list of URI entries and handles the selection and retry logic for connection attempts.
///
/// The `UriSelector` struct contains a collection of `UriEntry` instances and manages the logic
/// for selecting the next URI to attempt a connection to, including backoff and retry mechanisms.
#[derive(Debug)]
pub struct UriSelector {
    /// A priority queue of `UriEntry` instances, organized as a min-heap.
    /// The URI with the earliest `next_attempt` time is prioritized. If `next_attempt`
    /// has not yet passed, the process will wait until it has before retrying.
    uris: BinaryHeap<UriEntry>,
    /// The initial duration to wait before retrying a connection.
    initial_retry_after: Duration,
    /// The maximum duration to wait before retrying a connection.
    max_retry_after: Duration,
}

impl Ord for UriEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse the comparison to make the BinaryHeap a min-heap
        self.next_attempt.cmp(&other.next_attempt).reverse()
    }
}

impl PartialOrd for UriEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl UriSelector {
    /// Creates a new `UriSelector` with the given list of URIs, initial retry duration, and maximum retry duration.
    ///
    /// # Arguments
    ///
    /// * `uris` - A vector of `Uri` objects representing the URIs to manage.
    /// * `initial_retry_after` - A `Duration` specifying the initial time to wait before retrying a failed URI.
    /// * `max_retry_after` - A `Duration` specifying the maximum time to wait before retrying a failed URI.
    ///
    /// # Returns
    ///
    /// A `UriSelector` instance with the given URIs and retry durations.
    pub fn new(
        uris: HashMap<String, Uri>,
        initial_retry_after: Duration,
        max_retry_after: Duration,
    ) -> Self {
        let now = Instant::now();
        UriSelector {
            uris: BinaryHeap::from(
                uris.into_iter()
                    .map(|(name, uri)| UriEntry {
                        name,
                        uri,
                        backoff_duration: initial_retry_after,
                        next_attempt: now,
                    })
                    .collect::<Vec<_>>(),
            ),
            initial_retry_after,
            max_retry_after,
        }
    }

    /// Selects the next URI to use, prioritizing URIs with the shortest backoff duration that are ready for another attempt.
    ///
    /// # Returns
    ///
    /// Returns an `Option<(String, Uri)>` where:
    /// - `String` is the name associated with the selected URI.
    /// - `Uri` is the next URI to use.
    ///
    /// If the URIs collection is empty, it returns `None`.
    ///
    /// # Behavior
    ///
    /// - If a URI is ready for its next attempt, i.e., its `next_attempt` time has passed or is equal to the current time,
    ///   this function immediately returns that URI.
    /// - If no URIs are ready, the function calculates the duration until the earliest `next_attempt` time,
    ///   sleeps for that duration, and then returns the URI once it is ready.
    pub async fn next_uri(&self) -> Option<(String, Uri)> {
        // Select the URI with the shortest backoff duration that is ready for the next attempt
        let uri = self.uris.peek()?;
        let now = Instant::now();

        // If the next attempt hasn't passed, sleep until the socket is ready
        if uri.next_attempt < now {
            let sleep_duration = uri.next_attempt - now;
            sleep_until((now + sleep_duration).into()).await;
        }

        Some((uri.name.clone(), uri.uri.clone()))
    }

    /// Marks the currently selected URI as failed, updating its backoff duration and next attempt time.
    pub fn mark_failed(&mut self) {
        if let Some(mut uri) = self.uris.pop() {
            uri.backoff_duration = (uri.backoff_duration * 2).min(self.max_retry_after);
            uri.next_attempt = Instant::now() + uri.backoff_duration;

            // Push back onto heap
            self.uris.push(uri);
        }
    }

    /// Resets the backoff duration and next attempt time for the currently selected URI.
    ///
    /// This function resets the backoff duration to the initial value
    /// and sets the next attempt time to now.
    /// It can be used after a successful connection to a URI to indicate that it is healthy again.
    pub fn reset_failure(&mut self) {
        if let Some(mut uri) = self.uris.pop() {
            uri.backoff_duration = self.initial_retry_after;
            uri.next_attempt = Instant::now();

            // Push back onto heap
            self.uris.push(uri);
        }
    }
}
