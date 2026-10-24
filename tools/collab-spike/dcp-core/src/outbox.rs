//! Spec section 10, last row: "Binding rate limit: back off and queue. Never
//! drop a version." And spec 8.1: send in the order created, at most 3
//! timeline events per second to one room, back off when rate limited.
//!
//! [`Outbox`] is the queue. It never discards an item: an item leaves only
//! after the binding accepted it, or when the caller explicitly gives up on
//! one the binding refused for good ([`Outbox::discard_head`]).

use std::collections::VecDeque;
use std::future::Future;

use crate::StoreError;

/// Spec 8.1's budget (NeoBoard's measurement; to be confirmed in Spike A).
pub const DEFAULT_EVENTS_PER_SECOND: u64 = 3;
const FIRST_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 60_000;

/// What one call to [`Outbox::step`] did.
#[derive(Debug)]
pub enum Step {
    /// Nothing queued.
    Idle,
    /// Too early: pacing or a back-off. Call again at `until_ms`.
    Wait { until_ms: u64 },
    /// The head was accepted and removed.
    Sent,
    /// The binding asked to slow down (or the transport failed): the item
    /// stays at the head and nothing behind it is sent. Retry at `until_ms`.
    BackedOff { until_ms: u64 },
    /// The binding refused the head for a reason retrying will not fix
    /// (forbidden, invalid, conflict, not found). The item stays at the head,
    /// blocking the queue so order is kept, until the caller decides.
    Refused(StoreError),
}

/// `StoreError` has no equality; two refusals are equal when they read the same.
impl PartialEq for Step {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Step::Idle, Step::Idle) | (Step::Sent, Step::Sent) => true,
            (Step::Wait { until_ms: a }, Step::Wait { until_ms: b })
            | (Step::BackedOff { until_ms: a }, Step::BackedOff { until_ms: b }) => a == b,
            (Step::Refused(a), Step::Refused(b)) => a.to_string() == b.to_string(),
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct Outbox<T> {
    queue: VecDeque<T>,
    min_interval_ms: u64,
    not_before_ms: u64,
    backoff_ms: u64,
}

impl<T> Outbox<T> {
    pub fn new(max_events_per_second: u64) -> Self {
        assert!(
            max_events_per_second > 0,
            "an outbox that never sends is a bug"
        );
        Outbox {
            queue: VecDeque::new(),
            min_interval_ms: 1_000_u64.div_ceil(max_events_per_second),
            not_before_ms: 0,
            backoff_ms: 0,
        }
    }

    pub fn push(&mut self, item: T) {
        self.queue.push_back(item);
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn head(&self) -> Option<&T> {
        self.queue.front()
    }

    /// Give up on the head after [`Step::Refused`]. The only way an item
    /// leaves without having been accepted.
    pub fn discard_head(&mut self) -> Option<T> {
        self.queue.pop_front()
    }

    /// Try to send the head at `now_ms`.
    /// `send` gets a clone of the head (wrap large items in an `Arc`); the
    /// original stays queued until the binding accepts the clone.
    pub async fn step<F, Fut>(&mut self, now_ms: u64, mut send: F) -> Step
    where
        T: Clone,
        F: FnMut(T) -> Fut,
        Fut: Future<Output = Result<(), StoreError>>,
    {
        let Some(head) = self.queue.front() else {
            return Step::Idle;
        };
        if now_ms < self.not_before_ms {
            return Step::Wait {
                until_ms: self.not_before_ms,
            };
        }
        match send(head.clone()).await {
            Ok(()) => {
                self.queue.pop_front();
                self.backoff_ms = 0;
                self.not_before_ms = now_ms + self.min_interval_ms;
                Step::Sent
            }
            Err(StoreError::RateLimited { retry_after_ms }) => Step::BackedOff {
                until_ms: self.back_off(now_ms, retry_after_ms),
            },
            Err(StoreError::Transport(_)) => Step::BackedOff {
                until_ms: self.back_off(now_ms, None),
            },
            Err(refused) => Step::Refused(refused),
        }
    }

    /// Honour the server's `retry_after_ms`, else double from one second up
    /// to a minute.
    fn back_off(&mut self, now_ms: u64, retry_after_ms: Option<u64>) -> u64 {
        self.backoff_ms = match self.backoff_ms {
            0 => FIRST_BACKOFF_MS,
            n => (n * 2).min(MAX_BACKOFF_MS),
        };
        let wait = retry_after_ms
            .unwrap_or(self.backoff_ms)
            .max(self.min_interval_ms);
        self.not_before_ms = now_ms + wait;
        self.not_before_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    /// A binding that records what it accepted, and fails as scripted.
    struct Binding {
        accepted: Mutex<Vec<u32>>,
        script: Mutex<VecDeque<Result<(), StoreError>>>,
    }

    impl Binding {
        fn new(script: Vec<Result<(), StoreError>>) -> Self {
            Binding {
                accepted: Mutex::new(vec![]),
                script: Mutex::new(script.into()),
            }
        }

        async fn send(&self, item: u32) -> Result<(), StoreError> {
            let r = self.script.lock().pop_front().unwrap_or(Ok(()));
            if r.is_ok() {
                self.accepted.lock().push(item);
            }
            r
        }
    }

    fn limited(retry: Option<u64>) -> Result<(), StoreError> {
        Err(StoreError::RateLimited {
            retry_after_ms: retry,
        })
    }

    #[tokio::test]
    async fn sends_in_order_at_three_per_second() {
        let b = Binding::new(vec![]);
        let mut o = Outbox::new(3);
        (1..=7).for_each(|i| o.push(i));
        let mut sent_at = vec![];
        let mut now = 0;
        while !o.is_empty() {
            match o.step(now, |i| b.send(i)).await {
                Step::Sent => sent_at.push(now),
                Step::Wait { until_ms } => now = until_ms,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(
            *b.accepted.lock(),
            [1, 2, 3, 4, 5, 6, 7],
            "order of creation"
        );
        assert_eq!(
            sent_at.iter().filter(|t| **t < 1_000).count(),
            3,
            "at most 3 in the first second: {sent_at:?}"
        );
        assert!(sent_at.windows(2).all(|w| w[1] - w[0] >= 334));
    }

    #[tokio::test]
    async fn rate_limited_item_is_kept_retried_and_nothing_overtakes_it() {
        let b = Binding::new(vec![limited(Some(5_000)), limited(None)]);
        let mut o = Outbox::new(3);
        o.push(1);
        o.push(2);
        assert_eq!(
            o.step(0, |i| b.send(i)).await,
            Step::BackedOff { until_ms: 5_000 }
        );
        assert_eq!(o.len(), 2, "nothing dropped");
        assert_eq!(
            o.step(4_999, |i| b.send(i)).await,
            Step::Wait { until_ms: 5_000 }
        );
        // second refusal, no retry-after: exponential from the first back-off
        assert_eq!(
            o.step(5_000, |i| b.send(i)).await,
            Step::BackedOff { until_ms: 7_000 }
        );
        assert_eq!(o.step(7_000, |i| b.send(i)).await, Step::Sent);
        assert_eq!(o.step(8_000, |i| b.send(i)).await, Step::Sent);
        assert_eq!(*b.accepted.lock(), [1, 2]);
    }

    #[tokio::test]
    async fn backoff_doubles_to_a_cap_and_resets_on_success() {
        let b = Binding::new((0..10).map(|_| limited(None)).collect());
        let mut o = Outbox::new(3);
        o.push(1);
        let mut now = 0;
        let mut waits = vec![];
        for _ in 0..8 {
            let Step::BackedOff { until_ms } = o.step(now, |i| b.send(i)).await else {
                panic!()
            };
            waits.push(until_ms - now);
            now = until_ms;
        }
        assert_eq!(
            waits,
            [1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000]
        );
        b.script.lock().clear();
        assert_eq!(o.step(now, |i| b.send(i)).await, Step::Sent);
        o.push(2);
        b.script.lock().push_back(limited(None));
        let Step::BackedOff { until_ms } = o.step(now + 400, |i| b.send(i)).await else {
            panic!()
        };
        assert_eq!(
            until_ms - (now + 400),
            1_000,
            "back-off restarts after a success"
        );
    }

    #[tokio::test]
    async fn a_transport_failure_also_backs_off_and_keeps_the_item() {
        let b = Binding::new(vec![Err(StoreError::Transport("reset".into()))]);
        let mut o = Outbox::new(3);
        o.push(9);
        assert!(matches!(
            o.step(0, |i| b.send(i)).await,
            Step::BackedOff { .. }
        ));
        assert_eq!(o.head(), Some(&9));
    }

    #[tokio::test]
    async fn a_permanent_refusal_blocks_the_head_until_the_caller_decides() {
        let b = Binding::new(vec![Err(StoreError::Forbidden("viewer".into()))]);
        let mut o = Outbox::new(3);
        o.push(1);
        o.push(2);
        assert!(matches!(
            o.step(0, |i| b.send(i)).await,
            Step::Refused(StoreError::Forbidden(_))
        ));
        assert_eq!(o.len(), 2, "kept, not silently dropped");
        assert_eq!(o.discard_head(), Some(1));
        assert_eq!(o.step(0, |i| b.send(i)).await, Step::Sent);
        assert_eq!(*b.accepted.lock(), [2]);
    }

    #[tokio::test]
    async fn an_empty_outbox_is_idle() {
        let b = Binding::new(vec![]);
        let mut o: Outbox<u32> = Outbox::new(3);
        assert_eq!(o.step(0, |i| b.send(i)).await, Step::Idle);
    }
}
