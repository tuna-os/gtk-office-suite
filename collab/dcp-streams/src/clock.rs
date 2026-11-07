//! `now_ms`: the only time the protocol gives a client is the HTTP `Date`
//! header, in whole seconds. Durable Streams stamps no message and has no
//! clock endpoint, so this binding builds one:
//!
//! - every `Date` seen is a LOWER bound on the server's time at that instant
//!   (the header is the second the server's response began in, floored);
//!   advanced by the local monotonic clock it stays a lower bound;
//! - the estimate is the greatest lower bound seen, so it tightens with every
//!   sample that lands on a different phase of the second;
//! - [`Http::calibrate`](crate::http::Http::calibrate) pins the phase once by
//!   polling `HEAD` until the header ticks over: right after the tick the
//!   lower bound is within one poll interval of the truth.
//!
//! The error after calibration is the poll interval plus a round trip plus
//! monotonic drift (measured in the results document); without it, up to one
//! second, which is the figure to assume. No wall-clock time of this machine
//! is ever used.

use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Default)]
struct Inner {
    /// (lower bound on server time in ms at `at`, `at`)
    best: Option<(u64, Instant)>,
    last_date_ms: Option<u64>,
    calibrated: bool,
}

#[derive(Default)]
pub struct ServerClock {
    inner: Mutex<Inner>,
}

impl ServerClock {
    pub fn observe_http_date(&self, date: &str, at: Instant) {
        if let Ok(t) = httpdate::parse_http_date(date) {
            if let Ok(d) = t.duration_since(UNIX_EPOCH) {
                self.observe_ms(d.as_millis() as u64, at);
            }
        }
    }

    pub fn observe_ms(&self, date_ms: u64, at: Instant) {
        let mut g = self.inner.lock().unwrap();
        g.last_date_ms = Some(date_ms);
        let current = g
            .best
            .map(|(ms, t)| ms + at.saturating_duration_since(t).as_millis() as u64);
        if current.is_none_or(|c| date_ms > c) {
            g.best = Some((date_ms, at));
        }
    }

    /// The greatest lower bound on the server's current time, if any response
    /// has been seen.
    pub fn estimate(&self) -> Option<u64> {
        let g = self.inner.lock().unwrap();
        g.best.map(|(ms, t)| ms + t.elapsed().as_millis() as u64)
    }

    pub fn last_date_ms(&self) -> Option<u64> {
        self.inner.lock().unwrap().last_date_ms
    }

    pub fn is_calibrated(&self) -> bool {
        self.inner.lock().unwrap().calibrated
    }

    pub(crate) fn set_calibrated(&self) {
        self.inner.lock().unwrap().calibrated = true;
    }
}

/// This machine's wall clock in ms. Used ONLY by tests and measurements, to
/// compare against a server on the same host; never by the binding.
pub fn local_wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before 1970")
        .as_millis() as u64
}
