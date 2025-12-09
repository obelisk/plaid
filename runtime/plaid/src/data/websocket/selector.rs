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
///
/// This entry must be passed back to `mark_failed()` or `reset_failure()` to ensure
/// the correct URI entry is updated. This design prevents silent bugs where the heap
/// state might change between selection and update.
#[derive(Debug, Eq, PartialEq)]
pub struct UriEntry {
    name: String,
    /// The URI to connect to.
    uri: Uri,
    /// The duration to wait before retrying a connection.
    backoff_duration: Duration,
    /// The time when the next connection attempt should be made.
    next_attempt: Instant,
}

impl UriEntry {
    /// Returns the name associated with this URI.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a reference to the URI.
    pub fn uri(&self) -> &Uri {
        &self.uri
    }
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
    /// Returns an `Option<UriEntry>` containing the selected URI entry.
    /// The entry must be passed back to `mark_failed()` or `reset_failure()` to update its state.
    ///
    /// If the URIs collection is empty, it returns `None`.
    ///
    /// # Behavior
    ///
    /// - Peeks at the URI with the earliest next_attempt time
    /// - If the next attempt hasn't passed, sleeps until it's ready
    /// - Only pops the entry from the heap when it's ready to be used
    /// - Returns the entry for connection attempt
    pub async fn next_uri(&mut self) -> Option<UriEntry> {
        // Peek at the URI with the shortest backoff duration
        let entry = self.uris.peek()?;
        let now = Instant::now();

        // If the next attempt hasn't passed, sleep until the socket is ready
        if entry.next_attempt > now {
            sleep_until((entry.next_attempt).into()).await;
        }

        // Now pop it from the heap since it's ready
        self.uris.pop()
    }

    /// Marks the given URI entry as failed, updating its backoff duration and reinserting it into the heap.
    ///
    /// # Arguments
    ///
    /// * `entry` - The `UriEntry` returned from `next_uri()` that failed to connect.
    ///
    /// # Behavior
    ///
    /// - Doubles the backoff duration (up to `max_retry_after`)
    /// - Sets the next attempt time to `now + backoff duration`
    /// - Reinserts the entry back into the heap
    pub fn mark_failed(&mut self, mut entry: UriEntry) {
        entry.backoff_duration = (entry.backoff_duration * 2).min(self.max_retry_after);
        entry.next_attempt = Instant::now() + entry.backoff_duration;

        // Push back onto heap
        self.uris.push(entry);
    }

    /// Resets the backoff duration and next attempt time for the given URI entry.
    ///
    /// # Arguments
    ///
    /// * `entry` - The `UriEntry` returned from `next_uri()` that successfully connected.
    ///
    /// # Returns
    ///
    /// Returns the entry back so it can be used by the caller. The caller must eventually
    /// pass it back via `mark_failed()` to reinsert it into the heap.
    ///
    /// # Behavior
    ///
    /// This function resets the backoff duration to the initial value
    /// and sets the next attempt time to now, indicating the URI is healthy again.
    /// The entry is NOT reinserted into the heap - it's returned to the caller who
    /// will use it and eventually call `mark_failed()` to reinsert it.
    pub fn reset_failure(&self, mut entry: UriEntry) -> UriEntry {
        entry.backoff_duration = self.initial_retry_after;
        entry.next_attempt = Instant::now();

        entry
    }
}
