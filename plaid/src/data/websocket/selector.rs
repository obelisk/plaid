use http::Uri;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Represents a single URI entry with backoff duration and next attempt time.
///
/// The `UriEntry` struct contains information about a URI, including the duration to wait before
/// retrying a connection (`backoff_duration`) and the time when the next connection attempt
/// should be made.
#[derive(Debug)]
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
    /// A list of `UriEntry` instances.
    uris: Vec<UriEntry>,
    /// The index of the currently selected URI.
    current_index: usize,
    /// The initial duration to wait before retrying a connection.
    initial_retry_after: Duration,
    /// The maximum duration to wait before retrying a connection.
    max_retry_after: Duration,
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
            uris: uris
                .into_iter()
                .map(|(name, uri)| UriEntry {
                    name,
                    uri,
                    backoff_duration: initial_retry_after,
                    next_attempt: now,
                })
                .collect(),
            current_index: 0,
            initial_retry_after,
            max_retry_after,
        }
    }

    /// Selects the next URI to use, prioritizing URIs with the shortest backoff duration.
    ///
    /// This function sorts the URIs by their next attempt time,
    /// and returns a tuple containing the name and URI of the entry with the shortest backoff duration that is ready for the next attempt.
    /// If no URIs are ready, it selects and returns the name and URI of the entry with the earliest next attempt time.
    ///
    /// # Returns
    ///
    /// A tuple `(String, Uri)` where the `String` is the name of the URI and the `Uri` is the next URI to use.
    ///
    /// # Panics
    ///
    /// Panics if the URI list is empty, although in practice this should never be possible.
    pub fn next_uri(&mut self) -> (String, Uri) {
        // Sort URIs by next attempt time
        self.uris.sort_by_key(|entry| entry.next_attempt);

        // Select the URI with the shortest backoff duration that is ready for the next attempt
        let now = Instant::now();
        for (index, entry) in self.uris.iter().enumerate() {
            if now >= entry.next_attempt {
                self.current_index = index;
                return (entry.name.clone(), entry.uri.clone());
            }
        }

        // If no URIs are ready, select the one with the earliest next attempt time
        self.current_index = 0;
        let earliest_entry = self.uris.first().expect("URI list should never be empty");
        (earliest_entry.name.clone(), earliest_entry.uri.clone())
    }

    /// Marks the currently selected URI as failed, updating its backoff duration and next attempt time.
    pub fn mark_failed(&mut self) {
        if let Some(entry) = self.uris.get_mut(self.current_index) {
            entry.backoff_duration = (entry.backoff_duration * 2).min(self.max_retry_after);
            entry.next_attempt = Instant::now() + entry.backoff_duration;
        }
    }

    /// Resets the backoff duration and next attempt time for the currently selected URI.
    ///
    /// This function resets the backoff duration to the initial value
    /// and sets the next attempt time to now.
    /// It can be used after a successful connection to a URI to indicate that it is healthy again.
    pub fn reset_failure(&mut self) {
        if let Some(entry) = self.uris.get_mut(self.current_index) {
            entry.backoff_duration = self.initial_retry_after;
            entry.next_attempt = Instant::now();
        }
    }
}
