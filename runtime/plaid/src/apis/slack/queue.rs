//! Host-managed, persisted, per-channel outbound Slack queue.
//!
//! Rules hand alerts to the runtime via the non-blocking `slack_enqueue_message`
//! host function instead of posting inline. Entries are persisted to storage and a
//! single background task drains them, posting at most one message per channel per
//! second (Slack's `chat.postMessage` per-channel limit), in FIFO order, honoring
//! `Retry-After`. Nothing is dropped: the queue survives restarts (reloaded from
//! storage on startup) and only leaves the queue on success or after exhausting its
//! attempt budget (then dead-lettered + paged).
//!
//! This replaces the earlier rules-side emulation (a shared-storage queue + an
//! interval-triggered drainer rule): pacing, ordering, retry, and persistence all
//! live here in one native component.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use crate::storage::Storage;

use super::SLACK_POST_MESSAGE_URL;

/// Storage namespace for pending outbound posts. Host-internal (the runtime owns
/// it directly), so it needs no shared-DB configuration.
pub const QUEUE_NAMESPACE: &str = "__slack_outbound_queue__";
/// Storage namespace for posts that exhausted their attempt budget.
pub const DLQ_NAMESPACE: &str = "__slack_outbound_dlq__";
/// Failed post attempts before an entry is dead-lettered. Only genuine post
/// failures count; entries waiting their turn in line do not.
pub const MAX_ATTEMPTS: u32 = 15;
/// Minimum spacing between posts to the same channel.
const PER_CHANNEL_INTERVAL: Duration = Duration::from_secs(1);
/// How often the drainer wakes to check for due work.
const TICK: Duration = Duration::from_millis(200);

/// One queued outbound Slack post.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct QueuedPost {
    /// Configured bot name whose token is used to post.
    pub bot: String,
    /// Channel id — drives per-channel queueing and pacing.
    pub channel: String,
    /// Fully rendered `chat.postMessage` JSON body (channel, blocks, attachments…).
    pub body: String,
    /// The calling rule's private storage namespace (== its module name), where the
    /// posted message ref is written on success.
    pub ref_namespace: String,
    /// Key under `ref_namespace` to write the `{ok, channel, ts}` ref to.
    pub ref_key: String,
    /// Failed post attempts so far.
    pub attempts: u32,
    /// Enqueue timestamp (epoch nanos) — orders the per-channel FIFO via the key.
    pub seq: u128,
}

/// Storage key: `{channel}:{seq}` zero-padded so lexicographic order is FIFO.
pub fn storage_key(post: &QueuedPost) -> String {
    format!("{}:{:039}", post.channel, post.seq)
}

/// Monotonic-ish ordering value for a new entry (epoch nanos).
pub fn next_seq() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[derive(Deserialize)]
struct PostResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    ts: String,
}

/// Parse a Slack `Retry-After` header value (seconds), defaulting to 1.
fn parse_retry_after(value: Option<&str>) -> u64 {
    value.and_then(|v| v.parse::<u64>().ok()).unwrap_or(1)
}

enum Outcome {
    /// Posted; carries the channel/ts to write back as the message ref.
    Posted { channel: String, ts: String },
    /// Rate limited; wait this many seconds before retrying this channel.
    RateLimited(u64),
    /// Transient or permanent failure; counts against the attempt budget.
    Failed,
}

/// Persist (or overwrite) an entry under its stable key.
pub async fn persist(storage: &Storage, post: &QueuedPost) {
    match serde_json::to_vec(post) {
        Ok(value) => {
            if let Err(e) = storage
                .insert(QUEUE_NAMESPACE.to_string(), storage_key(post), value)
                .await
            {
                error!("[slack-queue] failed to persist post for {}: {e}", post.channel);
            }
        }
        Err(e) => error!("[slack-queue] failed to serialize queued post: {e}"),
    }
}

async fn remove(storage: &Storage, post: &QueuedPost) {
    if let Err(e) = storage.delete(QUEUE_NAMESPACE, &storage_key(post)).await {
        error!("[slack-queue] failed to delete post for {}: {e}", post.channel);
    }
}

async fn dead_letter(storage: &Storage, post: &QueuedPost) {
    error!(
        "[slack-queue] dead-lettering post to {} after {} attempts",
        post.channel, post.attempts
    );
    if let Ok(value) = serde_json::to_vec(post) {
        let _ = storage
            .insert(DLQ_NAMESPACE.to_string(), storage_key(post), value)
            .await;
    }
    remove(storage, post).await;
}

/// Write the posted message ref into the calling rule's private storage, where its
/// ack/resolve handling reads it.
async fn write_ref(storage: &Storage, post: &QueuedPost, channel: &str, ts: &str) {
    let value = serde_json::json!({ "ok": true, "channel": channel, "ts": ts });
    if let Ok(bytes) = serde_json::to_vec(&value) {
        if let Err(e) = storage
            .insert(post.ref_namespace.clone(), post.ref_key.clone(), bytes)
            .await
        {
            error!(
                "[slack-queue] failed to write message ref to {}:{}: {e}",
                post.ref_namespace, post.ref_key
            );
        }
    }
}

async fn try_post(client: &Client, tokens: &HashMap<String, String>, post: &QueuedPost) -> Outcome {
    let Some(token) = tokens.get(&post.bot) else {
        error!("[slack-queue] unknown bot '{}', cannot post", post.bot);
        return Outcome::Failed;
    };

    let response = client
        .post(SLACK_POST_MESSAGE_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .body(post.body.clone())
        .send()
        .await;

    let response = match response {
        Ok(response) => response,
        Err(e) => {
            warn!("[slack-queue] request to {} failed: {e}", post.channel);
            return Outcome::Failed;
        }
    };

    if response.status().as_u16() == 429 {
        let retry_after = parse_retry_after(
            response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()),
        );
        return Outcome::RateLimited(retry_after);
    }

    let body = response.text().await.unwrap_or_default();
    match serde_json::from_str::<PostResponse>(&body) {
        Ok(r) if r.ok && !r.channel.is_empty() && !r.ts.is_empty() => Outcome::Posted {
            channel: r.channel,
            ts: r.ts,
        },
        _ => {
            warn!("[slack-queue] non-ok response posting to {}: {body}", post.channel);
            Outcome::Failed
        }
    }
}

/// Load persisted entries into per-channel FIFO queues (called once at startup).
async fn load_persisted(storage: &Storage) -> HashMap<String, VecDeque<QueuedPost>> {
    let mut queues: HashMap<String, VecDeque<QueuedPost>> = HashMap::new();
    let entries = match storage.fetch_all(QUEUE_NAMESPACE, None).await {
        Ok(entries) => entries,
        Err(e) => {
            error!("[slack-queue] failed to load persisted queue: {e}");
            return queues;
        }
    };

    let mut posts: Vec<QueuedPost> = entries
        .into_iter()
        .filter_map(|(_, value)| value)
        .filter_map(|value| serde_json::from_slice::<QueuedPost>(&value).ok())
        .collect();
    // FIFO within a channel == ascending seq.
    posts.sort_by_key(|p| p.seq);
    for post in posts {
        queues.entry(post.channel.clone()).or_default().push_back(post);
    }
    if !queues.is_empty() {
        info!(
            "[slack-queue] reloaded {} channel queue(s) from storage",
            queues.len()
        );
    }
    queues
}

/// The drain loop. Owns the in-memory per-channel queues (storage is the durable
/// backing) and posts due work each tick.
pub async fn run(
    mut rx: UnboundedReceiver<QueuedPost>,
    client: Client,
    tokens: HashMap<String, String>,
    storage: Arc<Storage>,
) {
    let mut queues = load_persisted(&storage).await;
    let mut next_allowed: HashMap<String, Instant> = HashMap::new();
    let mut ticker = tokio::time::interval(TICK);

    loop {
        tokio::select! {
            maybe = rx.recv() => match maybe {
                Some(post) => queues.entry(post.channel.clone()).or_default().push_back(post),
                None => return, // sender dropped; runtime shutting down
            },
            _ = ticker.tick() => {
                let now = Instant::now();
                let channels: Vec<String> = queues.keys().cloned().collect();
                for channel in channels {
                    if next_allowed.get(&channel).is_some_and(|t| *t > now) {
                        continue;
                    }
                    let Some(post) = queues.get(&channel).and_then(|q| q.front()).cloned() else {
                        continue;
                    };
                    match try_post(&client, &tokens, &post).await {
                        Outcome::Posted { channel: ch, ts } => {
                            write_ref(&storage, &post, &ch, &ts).await;
                            remove(&storage, &post).await;
                            if let Some(q) = queues.get_mut(&channel) {
                                q.pop_front();
                            }
                            next_allowed.insert(channel, now + PER_CHANNEL_INTERVAL);
                        }
                        Outcome::RateLimited(secs) => {
                            // Leave the entry in place; just back off this channel.
                            next_allowed.insert(channel, now + Duration::from_secs(secs));
                        }
                        Outcome::Failed => {
                            let mut bumped = post.clone();
                            bumped.attempts += 1;
                            if bumped.attempts >= MAX_ATTEMPTS {
                                dead_letter(&storage, &bumped).await;
                                if let Some(q) = queues.get_mut(&channel) {
                                    q.pop_front();
                                }
                            } else {
                                persist(&storage, &bumped).await; // same key → overwrite
                                if let Some(q) = queues.get_mut(&channel) {
                                    if let Some(front) = q.front_mut() {
                                        *front = bumped;
                                    }
                                }
                            }
                            next_allowed.insert(channel, now + PER_CHANNEL_INTERVAL);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(channel: &str, seq: u128) -> QueuedPost {
        QueuedPost {
            bot: "alertbot".into(),
            channel: channel.into(),
            body: "{}".into(),
            ref_namespace: "rule.wasm".into(),
            ref_key: "ref".into(),
            attempts: 0,
            seq,
        }
    }

    #[test]
    fn keys_are_channel_scoped_and_fifo_by_seq() {
        let a = storage_key(&post("C1", 5));
        let b = storage_key(&post("C1", 1000));
        assert!(a < b, "lower seq must sort first within a channel");
        assert!(a.starts_with("C1:"), "key must be channel-scoped");
    }

    #[test]
    fn retry_after_parsing() {
        assert_eq!(parse_retry_after(Some("30")), 30);
        assert_eq!(parse_retry_after(Some("")), 1);
        assert_eq!(parse_retry_after(None), 1);
        assert_eq!(parse_retry_after(Some("garbage")), 1);
    }

    #[test]
    fn queued_post_round_trips() {
        let p = post("C9", 42);
        let bytes = serde_json::to_vec(&p).unwrap();
        assert_eq!(serde_json::from_slice::<QueuedPost>(&bytes).unwrap(), p);
    }

    #[test]
    fn post_response_requires_ok_channel_and_ts() {
        let bad: PostResponse = serde_json::from_str(r#"{"ok":false}"#).unwrap();
        assert!(!(bad.ok && !bad.channel.is_empty() && !bad.ts.is_empty()));
        let good: PostResponse =
            serde_json::from_str(r#"{"ok":true,"channel":"C1","ts":"1.2"}"#).unwrap();
        assert!(good.ok && !good.channel.is_empty() && !good.ts.is_empty());
    }
}
