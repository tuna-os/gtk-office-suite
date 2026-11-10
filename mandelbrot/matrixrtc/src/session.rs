// SPDX-License-Identifier: GPL-3.0-or-later

//! Minimal MatrixRTC session: builds the membership list of a room call
//! from room state.

use tracing::{info, warn};

use crate::{
    call_membership::{CallMembership, MemberStateEvent},
    membership_data::{FocusActive, MembershipParseError, Transport},
};

/// The virtual address of a MatrixRTC session inside a room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotDescription {
    /// The application of the session, e.g. `m.call`.
    pub application: String,
    /// The application specific ID of the session. `ROOM` for room calls.
    pub id: String,
}

impl SlotDescription {
    /// The description of the room-wide `m.call` session.
    pub fn room_call() -> Self {
        Self {
            application: "m.call".to_owned(),
            id: "ROOM".to_owned(),
        }
    }
}

/// A minimal MatrixRTC session scoped to the room-wide call (`m.call`
/// application, `call_id` `""`).
///
/// This class doesn't deal with media at all, just the membership of a
/// session.
#[derive(Clone, Debug, Default)]
pub struct MatrixRtcSession {
    /// The current memberships of the session, oldest first.
    pub memberships: Vec<CallMembership>,
}

impl MatrixRtcSession {
    /// Compute the session membership list of the room-wide call from the
    /// `org.matrix.msc3401.call.member` state events of a room.
    ///
    /// * `member_events` - The current `m.call.member` state events.
    /// * `is_joined_room_member` - Whether the given user ID is a joined member
    ///   of the room.
    /// * `now` - The current time in milliseconds since the Unix epoch, used to
    ///   filter expired memberships.
    ///
    /// Invalid, expired, empty and foreign-slot memberships are ignored. The
    /// result is ordered by `created_ts`, oldest first.
    pub fn room_call_memberships(
        member_events: &[MemberStateEvent],
        is_joined_room_member: impl Fn(&str) -> bool,
        now: u64,
    ) -> Vec<CallMembership> {
        let mut memberships = Vec::new();

        for event in member_events {
            let membership = match CallMembership::parse_from_event(event) {
                Ok(membership) => membership,
                Err(MembershipParseError::Empty | MembershipParseError::DeprecatedFormat) => {
                    continue;
                }
                Err(error) => {
                    warn!("Couldn't construct call membership: {error}");
                    continue;
                }
            };

            if membership.application() != "m.call" || !membership.call_id().is_empty() {
                info!(
                    "Ignoring membership of user {} for a different slot",
                    membership.user_id()
                );
                continue;
            }

            if membership.is_expired(now) {
                info!(
                    "Ignoring expired device membership {}/{}",
                    membership.user_id(),
                    membership.device_id()
                );
                continue;
            }

            if !is_joined_room_member(membership.user_id()) {
                info!(
                    "Ignoring membership of user {} who is not in the room",
                    membership.user_id()
                );
                continue;
            }

            memberships.push(membership);
        }

        memberships.sort_by_key(CallMembership::created_ts);
        memberships
    }

    /// Construct a session for the room-wide call.
    ///
    /// See [`Self::room_call_memberships`] for the arguments.
    pub fn new_room_call(
        member_events: &[MemberStateEvent],
        is_joined_room_member: impl Fn(&str) -> bool,
        now: u64,
    ) -> Self {
        Self {
            memberships: Self::room_call_memberships(member_events, is_joined_room_member, now),
        }
    }

    /// The oldest membership of the session, if any.
    pub fn get_oldest_membership(&self) -> Option<&CallMembership> {
        self.memberships.first()
    }

    /// The focus (transport) in use by the session, from the perspective of
    /// a member using `own_focus_active` as its focus selection system.
    ///
    /// With the `oldest_membership` selection method this is the transport of
    /// the oldest membership. Unknown selection methods yield `None`.
    pub fn get_active_focus(&self, own_focus_active: &FocusActive) -> Option<Transport> {
        let selection = own_focus_active.focus_selection.as_ref()?;
        match selection.as_str() {
            "oldest_membership" => {
                let oldest = self.get_oldest_membership()?;
                oldest.get_transport(oldest)
            }
            _ => None,
        }
    }
}
