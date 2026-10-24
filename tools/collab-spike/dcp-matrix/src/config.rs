//! Tunables of the Matrix binding. Every default is the spec's strawman value
//! (spec 8.1); the measurements in `docs/rfc/0004-spike-a-results.md` say which
//! of them survive.

use std::time::Duration;

/// Where `now_ms` (spec lease rule 1) comes from.
///
/// Matrix has no clock endpoint: `origin_server_ts` exists only on an event the
/// server has just accepted. Both honest options are offered and measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    /// `origin_server_ts` of the newest event this device wrote, advanced by the
    /// local monotonic clock since (ms resolution, no extra request, no extra
    /// rate-limit slot). Falls back to [`ClockSource::ServerDate`] until this
    /// device has written something, so a pure reader gets 1 s resolution.
    Calibrated,
    /// The `Date` header of a cheap homeserver request (1 s resolution, costs no
    /// rate-limit slot, works for read-only members).
    ServerDate,
    /// Send a probe timeline event and read its `origin_server_ts` (ms
    /// resolution, costs one rate-limited event, needs the editor role).
    ProbeEvent,
}

#[derive(Clone, Debug)]
pub struct MatrixConfig {
    /// Base URL of the homeserver, e.g. `http://127.0.0.1:8008`.
    pub homeserver: String,
    /// Sustained timeline/state events per second per room (spec: 3). `<= 0`
    /// disables the limiter, which is how the raw 429 behaviour is measured.
    pub events_per_sec: f64,
    /// Token bucket capacity; how many events may go out back to back.
    pub burst: u32,
    /// Give up on one event after this long of 429 / network retries.
    pub send_deadline: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// Largest inline update payload this store will publish (spec: 34000).
    pub max_inline_payload: usize,
    /// How long a read waits for room keys that have not arrived yet before it
    /// treats still-undecryptable events as unreadable.
    pub utd_wait: Duration,
    /// Run `/sync` in the background. Turn it off to drive `sync_once` by hand
    /// (the latency measurement does).
    pub background_sync: bool,
    /// Long-poll timeout of the background sync.
    pub sync_timeout: Duration,
    /// Put `m.room.encrypted` in the power-level `events` table at the DCP
    /// level (50). Without it the spec's table is not enforced by the server
    /// in an encrypted room; see the results doc.
    pub enforce_encrypted_pl: bool,
    pub clock: ClockSource,
    /// Before encrypting a version or update, make sure this device's member
    /// list no longer contains anyone the homeserver says has left. Without it
    /// a kick made elsewhere reaches matrix-sdk only with the next /sync, and
    /// anything sent in between is encrypted for the removed device too.
    pub guard_stale_members: bool,
    /// Reader rule of spec 8.1: when an encrypted room's power-levels table
    /// does not gate `m.room.encrypted` at the editor level, drop DCP timeline
    /// events (versions and updates) from senders below level 50. A test knob
    /// like `guard_stale_members`: turn it off to see what a reader without the
    /// rule accepts (the negative control of the forged-version journey). It
    /// never disables the `created_by.principal == sender` check on versions.
    pub reader_checks_senders: bool,
    /// Room version for new documents; `None` takes the server default.
    pub room_version: Option<String>,
    /// Keep the SDK's state and crypto store in this directory (needs the
    /// `sqlite` feature). `None` is in-memory.
    pub store_dir: Option<std::path::PathBuf>,
}

impl MatrixConfig {
    pub fn new(homeserver: impl Into<String>) -> Self {
        MatrixConfig {
            homeserver: homeserver.into(),
            events_per_sec: 3.0,
            burst: 3,
            send_deadline: Duration::from_secs(180),
            backoff_min: Duration::from_millis(500),
            backoff_max: Duration::from_secs(30),
            max_inline_payload: 34_000,
            utd_wait: Duration::from_secs(5),
            background_sync: true,
            sync_timeout: Duration::from_secs(10),
            enforce_encrypted_pl: true,
            clock: ClockSource::Calibrated,
            guard_stale_members: true,
            reader_checks_senders: true,
            room_version: None,
            store_dir: None,
        }
    }
}
