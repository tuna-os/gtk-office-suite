//! WOPI timestamps: `LastModifiedTime` is ISO 8601, and Collabora echoes the
//! exact string back in `X-COOL-WOPI-Timestamp` (verified in
//! `wsd/WopiStorage.cpp`: `setWopiHeader(httpRequest, "Timestamp",
//! getLastModifiedTime())`).

use time::format_description::{well_known::Rfc3339, FormatItem};
use time::macros::format_description;
use time::OffsetDateTime;

const FMT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

/// `ms` since the Unix epoch as `2026-10-24T03:47:00.123Z`.
pub fn iso8601_ms(ms: u64) -> String {
    let nanos = i128::from(ms) * 1_000_000;
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .ok()
        .and_then(|t| t.format(FMT).ok())
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}

/// Parse any RFC 3339 timestamp to whole milliseconds since the epoch.
pub fn parse_iso8601_ms(s: &str) -> Option<u64> {
    let t = OffsetDateTime::parse(s.trim(), &Rfc3339).ok()?;
    u64::try_from(t.unix_timestamp_nanos() / 1_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_milliseconds() {
        for ms in [0u64, 1, 999, 1_761_277_620_123] {
            assert_eq!(parse_iso8601_ms(&iso8601_ms(ms)), Some(ms));
        }
        assert_eq!(iso8601_ms(1_761_277_620_123), "2025-10-24T03:47:00.123Z");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_iso8601_ms("yesterday"), None);
    }
}
