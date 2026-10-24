//! Per-room ordered send queue with a token-bucket limiter and backoff
//! (spec 8.1 "Ordering and rate").
//!
//! One worker task per room drains an unbounded channel, so events leave in
//! the order `send` was called. The worker spends one token per event, waits
//! when the bucket is empty, and on an `M_LIMIT_EXCEEDED` answer sleeps for
//! the server's `retry_after_ms` (or an exponential backoff when it gave none)
//! and retries *the same transaction ID*, so a retry after a lost response
//! cannot publish twice. Network failures are retried the same way until the
//! deadline. matrix-sdk's own retry is switched off for the client (see
//! `store.rs`) so every 429 is visible here and counted.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dcp_core::StoreError;
use matrix_sdk::ruma::{OwnedTransactionId, TransactionId};
use matrix_sdk::Room;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::config::MatrixConfig;
use crate::error::{classify, to_store, Class};

/// Counters shared by every queue of one store.
#[derive(Default, Debug)]
pub struct Stats {
    pub sent: AtomicU64,
    /// HTTP 429 answers seen (each one a retry).
    pub rate_limited: AtomicU64,
    /// Requests that got no answer and were retried.
    pub network_retries: AtomicU64,
    /// Milliseconds spent waiting on the limiter or on backoff.
    pub waited_ms: AtomicU64,
    /// Events a reader ignored because their content did not validate.
    pub ignored_invalid: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatsSnapshot {
    pub sent: u64,
    pub rate_limited: u64,
    pub network_retries: u64,
    pub waited_ms: u64,
    pub ignored_invalid: u64,
}

impl Stats {
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            sent: self.sent.load(Ordering::Relaxed),
            rate_limited: self.rate_limited.load(Ordering::Relaxed),
            network_retries: self.network_retries.load(Ordering::Relaxed),
            waited_ms: self.waited_ms.load(Ordering::Relaxed),
            ignored_invalid: self.ignored_invalid.load(Ordering::Relaxed),
        }
    }
}

pub(crate) enum Job {
    Timeline { event_type: String, content: Value },
    State { event_type: String, state_key: String, content: Value },
}

struct Queued {
    job: Job,
    txn: OwnedTransactionId,
    reply: oneshot::Sender<Result<String, StoreError>>,
}

pub(crate) struct RoomQueue {
    tx: mpsc::UnboundedSender<Queued>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RoomQueue {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Bucket {
    rate: f64,
    cap: f64,
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn new(rate: f64, burst: u32) -> Option<Bucket> {
        if rate <= 0.0 {
            return None;
        }
        let cap = f64::from(burst.max(1));
        Some(Bucket { rate, cap, tokens: cap, last: Instant::now() })
    }

    /// How long to wait before one token is available; spends it.
    fn take(&mut self) -> Duration {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * self.rate).min(self.cap);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Duration::ZERO
        } else {
            let wait = (1.0 - self.tokens) / self.rate;
            self.tokens -= 1.0;
            Duration::from_secs_f64(wait)
        }
    }
}

impl RoomQueue {
    pub(crate) fn spawn(room: Room, cfg: MatrixConfig, stats: Arc<Stats>) -> RoomQueue {
        let (tx, mut rx) = mpsc::unbounded_channel::<Queued>();
        let task = tokio::spawn(async move {
            let mut bucket = Bucket::new(cfg.events_per_sec, cfg.burst);
            while let Some(q) = rx.recv().await {
                if let Some(b) = bucket.as_mut() {
                    let wait = b.take();
                    if !wait.is_zero() {
                        stats.waited_ms.fetch_add(wait.as_millis() as u64, Ordering::Relaxed);
                        tokio::time::sleep(wait).await;
                    }
                }
                let res = send_with_retry(&room, &cfg, &stats, &q).await;
                if res.is_ok() {
                    stats.sent.fetch_add(1, Ordering::Relaxed);
                }
                let _ = q.reply.send(res);
            }
        });
        RoomQueue { tx, task }
    }

    /// Enqueue and wait until the homeserver accepted the event; returns its ID.
    pub(crate) async fn send(&self, job: Job) -> Result<String, StoreError> {
        let (reply, rx) = oneshot::channel();
        let queued = Queued { job, txn: TransactionId::new(), reply };
        self.tx.send(queued).map_err(|_| StoreError::Transport("send queue closed".into()))?;
        rx.await.map_err(|_| StoreError::Transport("send queue dropped the event".into()))?
    }
}

async fn send_with_retry(
    room: &Room,
    cfg: &MatrixConfig,
    stats: &Stats,
    q: &Queued,
) -> Result<String, StoreError> {
    let start = Instant::now();
    let mut backoff = cfg.backoff_min;
    loop {
        let attempt = match &q.job {
            Job::Timeline { event_type, content } => room
                .send_raw(event_type, content.clone())
                .with_transaction_id(&q.txn)
                .await
                .map(|r| r.response.event_id.to_string()),
            Job::State { event_type, state_key, content } => room
                .send_state_event_raw(event_type, state_key, content.clone())
                .await
                .map(|r| r.event_id.to_string()),
        };
        let err = match attempt {
            Ok(id) => return Ok(id),
            Err(e) => e,
        };
        let wait = match classify(&err) {
            Class::RateLimited(suggested) => {
                stats.rate_limited.fetch_add(1, Ordering::Relaxed);
                let w = suggested.unwrap_or(backoff);
                backoff = (backoff * 2).min(cfg.backoff_max);
                w
            }
            Class::Network => {
                stats.network_retries.fetch_add(1, Ordering::Relaxed);
                let w = backoff;
                backoff = (backoff * 2).min(cfg.backoff_max);
                w
            }
            _ => return Err(to_store(err, "send event")),
        };
        if start.elapsed() + wait > cfg.send_deadline {
            return Err(match classify(&err) {
                Class::RateLimited(_) => StoreError::RateLimited { retry_after_ms: Some(wait.as_millis() as u64) },
                _ => to_store(err, "send event (deadline exceeded)"),
            });
        }
        stats.waited_ms.fetch_add(wait.as_millis() as u64, Ordering::Relaxed);
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_a_burst_then_paces() {
        let mut b = Bucket::new(3.0, 3).unwrap();
        for _ in 0..3 {
            assert_eq!(b.take(), Duration::ZERO, "burst of 3 goes out at once");
        }
        let w = b.take();
        assert!(w > Duration::from_millis(300) && w <= Duration::from_millis(340), "4th waits about 1/3 s, got {w:?}");
        // Taking again before the wait elapses queues behind the previous one.
        let w2 = b.take();
        assert!(w2 > Duration::from_millis(600) && w2 <= Duration::from_millis(680), "5th waits about 2/3 s, got {w2:?}");
    }

    #[test]
    fn non_positive_rate_disables_the_limiter() {
        assert!(Bucket::new(0.0, 3).is_none());
        assert!(Bucket::new(-1.0, 3).is_none());
    }
}
