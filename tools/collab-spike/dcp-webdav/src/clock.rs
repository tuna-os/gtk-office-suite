//! The server's clock, to the millisecond.
//!
//! HTTP gives whole seconds: `Date`, `Last-Modified` and WebDAV's
//! `getlastmodified` are all floored to the second, so two instants up to 999 ms
//! apart look identical. DCP's lease rules (4.3) take every time from the binding,
//! and the conformance journeys use a 4 second lease renewed at 2 seconds, which a
//! one second resolution cannot honour without error. [`ServerClock`] finds the
//! instant at which the server's `Date` ticks over, once, and counts forward with
//! the local monotonic clock. That is a calibrated estimate of the binding's clock,
//! not a reading of it; its error is the poll interval plus the round trip (a few
//! milliseconds on loopback) and the drift over the run, which this spike does not
//! measure.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcp_core::StoreError;

use crate::dav::Dav;

pub struct ServerClock {
    /// Server time, in milliseconds since the epoch, at `anchor_local`.
    anchor_server_ms: u64,
    anchor_local: Instant,
    /// How long the calibration took, for the results document.
    pub calibration: Duration,
    /// The poll interval, which bounds the calibration error from one side.
    pub poll_ms: u64,
}

impl ServerClock {
    pub async fn calibrate(dav: &Dav) -> Result<Arc<ServerClock>, StoreError> {
        const POLL_MS: u64 = 5;
        let started = Instant::now();
        let mut previous: Option<u64> = None;
        loop {
            let t0 = Instant::now();
            let r = dav.request("HEAD", "/robots.txt", &[], None).await?;
            let t1 = Instant::now();
            let date = r
                .header("date")
                .and_then(|d| httpdate::parse_http_date(d).ok())
                .and_then(|d| d.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .ok_or_else(|| StoreError::Transport("no usable Date header".into()))?;
            if let Some(p) = previous {
                if date != p {
                    // The server's second changed somewhere since the previous
                    // sample; take the middle of this request as the instant.
                    let mid = t0 + (t1 - t0) / 2;
                    return Ok(Arc::new(ServerClock {
                        anchor_server_ms: date * 1000,
                        anchor_local: mid,
                        calibration: started.elapsed(),
                        poll_ms: POLL_MS,
                    }));
                }
            }
            previous = Some(date);
            if started.elapsed() > Duration::from_secs(10) {
                return Err(StoreError::Transport("the server's Date never ticked in 10 s".into()));
            }
            tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
        }
    }

    pub fn now_ms(&self) -> u64 {
        self.anchor_server_ms + self.anchor_local.elapsed().as_millis() as u64
    }
}
