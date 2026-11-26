//! What the UI shows for each [`RecoveryState`].
//!
//! The SDK exposes one state machine, `Encryption::recovery().state_stream()`
//! (`Unknown`, `Disabled`, `Enabled`, `Incomplete`). [`SessionSecurity`]
//! subscribes to it, and every surface asks the functions below instead of
//! deciding for itself, so the screens cannot drift apart. They are pure, so
//! the transitions can be tested without a homeserver.
//!
//! | State        | Meaning                                    | Banner               | Setup flow             | Recovery dialog opens on |
//! |--------------|--------------------------------------------|----------------------|------------------------|--------------------------|
//! | `Unknown`    | SDK has not reported yet                   | none (wait)          | wait                   | `Enable`, or `Reset` if a backup exists on the server |
//! | `Disabled`   | no secret storage / recovery key           | "Enable"             | recovery page          | `Enable`, or `Reset` if a backup exists on the server |
//! | `Incomplete` | backup on, secrets not stored on this device | "Recover"          | recovery page, unless enable-only | `Recover`         |
//! | `Enabled`    | all keys present                           | none (if verified)   | done                   | `Reset`                  |
//!
//! [`SessionSecurity`]: super::SessionSecurity

use super::RecoveryState;

/// The page the recovery dialog opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDialogPage {
    /// Create a recovery key.
    Enable,
    /// Enter the recovery key to fetch the missing secrets.
    Recover,
    /// Replace the existing recovery key.
    Reset,
}

/// The action offered by the security banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryBannerAction {
    /// Offer to enable recovery.
    Enable,
    /// Offer to recover the missing secrets.
    Recover,
}

/// The page of the recovery dialog to open for `state`.
///
/// `backup_exists_on_server` matters when recovery is off but a key backup
/// exists: creating a new recovery key would then silently replace it, so the
/// user is sent to the reset page, which says so.
pub fn dialog_page(state: RecoveryState, backup_exists_on_server: bool) -> RecoveryDialogPage {
    match state {
        RecoveryState::Unknown | RecoveryState::Disabled if !backup_exists_on_server => {
            RecoveryDialogPage::Enable
        }
        RecoveryState::Unknown | RecoveryState::Disabled | RecoveryState::Enabled => {
            RecoveryDialogPage::Reset
        }
        RecoveryState::Incomplete => RecoveryDialogPage::Recover,
    }
}

/// The banner action for `state`, if the recovery state alone needs one.
///
/// `Unknown` returns `None`: it resolves by itself once the SDK reports.
pub fn banner_action(state: RecoveryState) -> Option<RecoveryBannerAction> {
    match state {
        RecoveryState::Unknown | RecoveryState::Enabled => None,
        RecoveryState::Disabled => Some(RecoveryBannerAction::Enable),
        RecoveryState::Incomplete => Some(RecoveryBannerAction::Recover),
    }
}

/// Whether the session setup flow must show the recovery page.
///
/// With `enable_only`, `Incomplete` is skipped: the user just verified with
/// another device and can recover later from the banner.
pub fn setup_needs_recovery_page(state: RecoveryState, enable_only: bool) -> bool {
    match state {
        RecoveryState::Disabled => true,
        RecoveryState::Incomplete => !enable_only,
        RecoveryState::Unknown | RecoveryState::Enabled => false,
    }
}

/// Whether a recovery key can be entered to restore secrets.
pub fn can_recover(state: RecoveryState) -> bool {
    state != RecoveryState::Disabled
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [RecoveryState; 4] = [
        RecoveryState::Unknown,
        RecoveryState::Disabled,
        RecoveryState::Enabled,
        RecoveryState::Incomplete,
    ];

    #[test]
    fn every_state_has_a_dialog_page() {
        for state in ALL {
            for backup in [false, true] {
                let _ = dialog_page(state, backup);
            }
        }
    }

    #[test]
    fn disabled_without_backup_enables() {
        assert_eq!(
            dialog_page(RecoveryState::Disabled, false),
            RecoveryDialogPage::Enable
        );
        assert_eq!(
            dialog_page(RecoveryState::Unknown, false),
            RecoveryDialogPage::Enable
        );
    }

    #[test]
    fn disabled_with_existing_backup_resets_instead_of_overwriting() {
        assert_eq!(
            dialog_page(RecoveryState::Disabled, true),
            RecoveryDialogPage::Reset
        );
    }

    #[test]
    fn incomplete_recovers_regardless_of_backup() {
        for backup in [false, true] {
            assert_eq!(
                dialog_page(RecoveryState::Incomplete, backup),
                RecoveryDialogPage::Recover
            );
        }
    }

    #[test]
    fn enabled_resets() {
        assert_eq!(
            dialog_page(RecoveryState::Enabled, true),
            RecoveryDialogPage::Reset
        );
    }

    #[test]
    fn banner_only_for_disabled_and_incomplete() {
        assert_eq!(banner_action(RecoveryState::Unknown), None);
        assert_eq!(banner_action(RecoveryState::Enabled), None);
        assert_eq!(
            banner_action(RecoveryState::Disabled),
            Some(RecoveryBannerAction::Enable)
        );
        assert_eq!(
            banner_action(RecoveryState::Incomplete),
            Some(RecoveryBannerAction::Recover)
        );
    }

    #[test]
    fn setup_page_transitions() {
        assert!(setup_needs_recovery_page(RecoveryState::Disabled, false));
        assert!(setup_needs_recovery_page(RecoveryState::Disabled, true));
        assert!(setup_needs_recovery_page(RecoveryState::Incomplete, false));
        assert!(!setup_needs_recovery_page(RecoveryState::Incomplete, true));
        assert!(!setup_needs_recovery_page(RecoveryState::Enabled, false));
        assert!(!setup_needs_recovery_page(RecoveryState::Unknown, false));
    }

    /// The stream moves a session Disabled -> Enabled when the user enables
    /// recovery, and Incomplete -> Enabled after recovering. Neither endpoint
    /// leaves a banner or a setup page behind.
    #[test]
    fn completing_either_flow_clears_prompts() {
        for from in [RecoveryState::Disabled, RecoveryState::Incomplete] {
            assert!(banner_action(from).is_some());
            assert!(banner_action(RecoveryState::Enabled).is_none());
            assert!(!setup_needs_recovery_page(RecoveryState::Enabled, false));
        }
    }

    #[test]
    fn only_disabled_cannot_recover() {
        assert!(!can_recover(RecoveryState::Disabled));
        assert!(can_recover(RecoveryState::Incomplete));
        assert!(can_recover(RecoveryState::Enabled));
    }
}
