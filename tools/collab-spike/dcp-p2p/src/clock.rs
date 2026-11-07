//! The clock a binding uses. A peer-to-peer binding has no clock of its own (no server stamps
//! anything), so the store takes one as a parameter and the skew tests can inject a wrong one.

use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync + 'static {
    fn now_ms(&self) -> u64;

    fn now_us(&self) -> u64 {
        self.now_ms().saturating_mul(1000)
    }
}

/// The local wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.now_us() / 1000
    }

    fn now_us(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0)
    }
}

/// The local wall clock plus a fixed error: a device whose clock is wrong.
#[derive(Debug, Clone, Copy)]
pub struct SkewedClock {
    pub offset_ms: i64,
}

impl Clock for SkewedClock {
    fn now_ms(&self) -> u64 {
        let base = SystemClock.now_ms() as i64;
        base.saturating_add(self.offset_ms).max(0) as u64
    }

    fn now_us(&self) -> u64 {
        self.now_ms().saturating_mul(1000)
    }
}
