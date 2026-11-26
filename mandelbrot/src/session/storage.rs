//! The storage limits of a session: how much media the SDK may keep.
//!
//! GTK-free so the arithmetic is unit-tested without a display.

use matrix_sdk::media::MediaRetentionPolicy;

const MIB: u64 = 1024 * 1024;

/// The default limit of the media cache, in MiB.
///
/// Matches the SDK's own default (400 MiB) so existing accounts keep their
/// behaviour until the person picks something else.
pub(crate) const DEFAULT_MEDIA_CACHE_LIMIT_MIB: u32 = 400;

/// The limits offered in the settings, in MiB, in ascending order.
pub(crate) const MEDIA_CACHE_LIMIT_CHOICES_MIB: &[u32] = &[50, 100, 200, 400, 1024, 2048, 4096];

/// The media retention policy for the given media cache limit, in MiB.
///
/// Only the cache size is customized; file size, expiry and cleanup frequency
/// keep the SDK defaults.
pub(crate) fn media_retention_policy(limit_mib: u32) -> MediaRetentionPolicy {
    MediaRetentionPolicy::default().with_max_cache_size(Some(u64::from(limit_mib) * MIB))
}

/// A policy that removes every media, for "Clear media cache".
pub(crate) fn clear_all_policy() -> MediaRetentionPolicy {
    MediaRetentionPolicy::default().with_max_cache_size(Some(0))
}

/// The index in [`MEDIA_CACHE_LIMIT_CHOICES_MIB`] of the choice closest to
/// the given limit, so a hand-edited stored value still selects something.
pub(crate) fn choice_index(limit_mib: u32) -> usize {
    MEDIA_CACHE_LIMIT_CHOICES_MIB
        .iter()
        .enumerate()
        .min_by_key(|(_, choice)| choice.abs_diff(limit_mib))
        .map(|(index, _)| index)
        .unwrap_or_default()
}

/// The limit for the choice at the given index, falling back to the default.
pub(crate) fn choice_limit(index: usize) -> u32 {
    MEDIA_CACHE_LIMIT_CHOICES_MIB
        .get(index)
        .copied()
        .unwrap_or(DEFAULT_MEDIA_CACHE_LIMIT_MIB)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_a_choice() {
        assert_eq!(
            choice_limit(choice_index(DEFAULT_MEDIA_CACHE_LIMIT_MIB)),
            DEFAULT_MEDIA_CACHE_LIMIT_MIB
        );
    }

    #[test]
    fn choices_ascend() {
        assert!(MEDIA_CACHE_LIMIT_CHOICES_MIB.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn policy_limit_is_in_bytes() {
        assert_eq!(
            media_retention_policy(50).max_cache_size,
            Some(50 * 1024 * 1024)
        );
        // The default limit is the SDK default, so nothing changes for existing accounts.
        assert_eq!(
            media_retention_policy(DEFAULT_MEDIA_CACHE_LIMIT_MIB),
            MediaRetentionPolicy::default()
        );
    }

    #[test]
    fn effective_file_size_is_capped_by_the_cache_limit() {
        // The SDK's default max file size is 20 MiB; a smaller cache lowers it.
        assert_eq!(
            media_retention_policy(50).computed_max_file_size(),
            Some(20 * 1024 * 1024)
        );
        assert_eq!(clear_all_policy().computed_max_file_size(), Some(0));
    }

    #[test]
    fn nearest_choice() {
        assert_eq!(choice_index(0), 0);
        assert_eq!(choice_index(390), choice_index(400));
        assert_eq!(choice_index(u32::MAX), MEDIA_CACHE_LIMIT_CHOICES_MIB.len() - 1);
        assert_eq!(choice_limit(999), DEFAULT_MEDIA_CACHE_LIMIT_MIB);
    }
}
