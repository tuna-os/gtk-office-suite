//! Undecryptable messages (UTD): what to tell the user, and the SDK hook.
//!
//! The SDK's `UtdHookManager` reports each event it could not decrypt. Our
//! hook only writes to the local log: nothing is counted or sent anywhere.

use std::sync::Arc;

use gettextrs::gettext;
use matrix_sdk::Client;
use matrix_sdk_base::crypto::types::events::UtdCause;
use matrix_sdk_ui::unable_to_decrypt_hook::{
    UnableToDecryptHook, UnableToDecryptInfo, UtdHookManager,
};
use tracing::{debug, warn};

/// What the user can do about an undecryptable message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UtdAction {
    /// Nothing to do on this device.
    None,
    /// Open the account settings, where this session can be verified and key
    /// backup can be turned on or recovered.
    OpenEncryptionSettings,
}

impl UtdAction {
    /// The name of the widget action that performs this action, if any.
    pub(crate) fn action_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::OpenEncryptionSettings => Some("session.open-account-settings"),
        }
    }

    /// The label of the button that performs this action, if any.
    pub(crate) fn button_label(self) -> Option<String> {
        match self {
            Self::None => None,
            Self::OpenEncryptionSettings => Some(gettext("Open Encryption Settings")),
        }
    }
}

/// How an undecryptable message is presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UtdAdvice {
    /// The explanation shown in the message row.
    pub(crate) message: String,
    /// The action that can fix it, if any.
    pub(crate) action: UtdAction,
}

impl UtdAdvice {
    /// The advice for the given cause.
    pub(crate) fn for_cause(cause: UtdCause) -> Self {
        let (message, action) = match cause {
            UtdCause::SentBeforeWeJoined => (
                gettext("You were not a member of this room when this message was sent, so it cannot be decrypted."),
                UtdAction::None,
            ),
            UtdCause::HistoricalMessageAndBackupIsDisabled => (
                gettext("This message was sent before this session existed and key backup is turned off. Turn on key backup to read history on new sessions."),
                UtdAction::OpenEncryptionSettings,
            ),
            UtdCause::HistoricalMessageAndDeviceIsUnverified => (
                gettext("This message was sent before this session existed. Verify this session to recover the keys from your key backup."),
                UtdAction::OpenEncryptionSettings,
            ),
            UtdCause::VerificationViolation => (
                gettext("This message comes from a user whose verified identity has changed, so it was not decrypted."),
                UtdAction::None,
            ),
            UtdCause::UnsignedDevice => (
                gettext("This message was sent from a device its owner has not verified, so it was not decrypted."),
                UtdAction::None,
            ),
            UtdCause::UnknownDevice => (
                gettext("The device that sent this message is unknown or has been removed, so it was not decrypted."),
                UtdAction::None,
            ),
            UtdCause::WithheldForUnverifiedOrInsecureDevice => (
                gettext("The sender did not share the key for this message because this session is not verified. Verify this session and ask them to resend it."),
                UtdAction::OpenEncryptionSettings,
            ),
            UtdCause::WithheldBySender => (
                gettext("The sender did not share the key for this message with this session."),
                UtdAction::None,
            ),
            UtdCause::Unknown => (
                gettext("Could not decrypt this message. Other sessions of yours will be asked for the key automatically, and decryption is retried once it arrives. If it does not, check that key backup is on."),
                UtdAction::OpenEncryptionSettings,
            ),
        };

        Self { message, action }
    }
}

/// A stable name for the cause, for logs.
pub(crate) fn cause_name(cause: UtdCause) -> &'static str {
    match cause {
        UtdCause::Unknown => "unknown",
        UtdCause::SentBeforeWeJoined => "sent-before-we-joined",
        UtdCause::VerificationViolation => "verification-violation",
        UtdCause::UnsignedDevice => "unsigned-device",
        UtdCause::UnknownDevice => "unknown-device",
        UtdCause::HistoricalMessageAndBackupIsDisabled => "historical-backup-disabled",
        UtdCause::WithheldForUnverifiedOrInsecureDevice => "withheld-unverified-device",
        UtdCause::WithheldBySender => "withheld-by-sender",
        UtdCause::HistoricalMessageAndDeviceIsUnverified => "historical-device-unverified",
    }
}

/// A hook that only logs, locally. No counters, no telemetry.
#[derive(Debug)]
struct LogUtdHook;

impl UnableToDecryptHook for LogUtdHook {
    fn on_utd(&self, info: UnableToDecryptInfo) {
        match info.time_to_decrypt {
            Some(delay) => debug!(
                event_id = %info.event_id,
                cause = cause_name(info.cause),
                ?delay,
                "Event was decrypted late",
            ),
            None => warn!(
                event_id = %info.event_id,
                cause = cause_name(info.cause),
                "Unable to decrypt event",
            ),
        }
    }
}

/// Create the UTD hook manager to give to a timeline builder.
pub(crate) fn hook_manager(client: Client) -> Arc<UtdHookManager> {
    Arc::new(UtdHookManager::new(Arc::new(LogUtdHook), client))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [UtdCause; 9] = [
        UtdCause::Unknown,
        UtdCause::SentBeforeWeJoined,
        UtdCause::VerificationViolation,
        UtdCause::UnsignedDevice,
        UtdCause::UnknownDevice,
        UtdCause::HistoricalMessageAndBackupIsDisabled,
        UtdCause::WithheldForUnverifiedOrInsecureDevice,
        UtdCause::WithheldBySender,
        UtdCause::HistoricalMessageAndDeviceIsUnverified,
    ];

    #[test]
    fn every_cause_has_a_distinct_message() {
        let messages: std::collections::HashSet<_> =
            ALL.iter().map(|c| UtdAdvice::for_cause(*c).message).collect();
        assert_eq!(messages.len(), ALL.len());
    }

    #[test]
    fn every_cause_has_a_distinct_log_name() {
        let names: std::collections::HashSet<_> = ALL.iter().map(|c| cause_name(*c)).collect();
        assert_eq!(names.len(), ALL.len());
    }

    #[test]
    fn recoverable_causes_offer_the_encryption_settings() {
        for cause in [
            UtdCause::HistoricalMessageAndBackupIsDisabled,
            UtdCause::HistoricalMessageAndDeviceIsUnverified,
            UtdCause::WithheldForUnverifiedOrInsecureDevice,
            UtdCause::Unknown,
        ] {
            assert_eq!(
                UtdAdvice::for_cause(cause).action,
                UtdAction::OpenEncryptionSettings,
                "{cause:?}",
            );
        }
    }

    #[test]
    fn unrecoverable_causes_offer_no_button() {
        for cause in [
            UtdCause::SentBeforeWeJoined,
            UtdCause::VerificationViolation,
            UtdCause::UnsignedDevice,
            UtdCause::UnknownDevice,
            UtdCause::WithheldBySender,
        ] {
            let advice = UtdAdvice::for_cause(cause);
            assert_eq!(advice.action, UtdAction::None, "{cause:?}");
            assert!(advice.action.action_name().is_none());
        }
    }
}
