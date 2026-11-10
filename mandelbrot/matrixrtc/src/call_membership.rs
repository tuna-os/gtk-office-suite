// SPDX-License-Identifier: GPL-3.0-or-later

//! A single MatrixRTC call membership.

use serde_json::Value as JsonValue;

use crate::membership_data::{
    DEFAULT_EXPIRE_DURATION_MS, MembershipParseError, SessionMembershipData, Transport,
    parse_session_membership_data,
};

/// The subset of a Matrix state event needed to construct a
/// [`CallMembership`].
#[derive(Clone, Debug)]
pub struct MemberStateEvent {
    /// The ID of the event.
    pub event_id: String,
    /// The sender of the event.
    pub sender: String,
    /// The `origin_server_ts` of the event, in milliseconds since the Unix
    /// epoch.
    pub origin_server_ts: u64,
    /// The state key of the event.
    pub state_key: String,
    /// The content of the event.
    pub content: JsonValue,
}

/// A parsed and validated MatrixRTC call membership.
#[derive(Clone, Debug)]
pub struct CallMembership {
    event_id: String,
    sender: String,
    origin_server_ts: u64,
    data: SessionMembershipData,
    /// See [`Self::rtc_backend_identity`]. `None` means the legacy
    /// `{user_id}:{device_id}` form.
    rtc_backend_identity: Option<String>,
}

impl CallMembership {
    /// Parse a [`CallMembership`] from a state event.
    pub fn parse_from_event(event: &MemberStateEvent) -> Result<Self, MembershipParseError> {
        if event.sender.is_empty() {
            return Err(MembershipParseError::MissingSender);
        }
        let data = parse_session_membership_data(&event.content)?;

        Ok(Self {
            event_id: event.event_id.clone(),
            sender: event.sender.clone(),
            origin_server_ts: event.origin_server_ts,
            data,
            rtc_backend_identity: None,
        })
    }

    /// Construct a membership directly from its parts. Mostly useful in
    /// tests.
    pub fn new(
        event_id: String,
        sender: String,
        origin_server_ts: u64,
        data: SessionMembershipData,
    ) -> Self {
        Self {
            event_id,
            sender,
            origin_server_ts,
            data,
            rtc_backend_identity: None,
        }
    }

    /// The identity used to represent this member on the RTC backend (e.g.
    /// the participant identity on a LiveKit SFU).
    ///
    /// For legacy session memberships this is `{user_id}:{device_id}`. The
    /// pseudonymous (hashed) identity of the MSC4354 sticky event format can
    /// be injected with [`Self::set_rtc_backend_identity`]; computing it is
    /// out of scope for now.
    pub fn rtc_backend_identity(&self) -> String {
        self.rtc_backend_identity
            .clone()
            .unwrap_or_else(|| format!("{}:{}", self.sender, self.data.device_id))
    }

    /// Override the RTC backend identity of this member.
    ///
    /// This is the seam for the pseudonymous identity of the sticky event
    /// membership format.
    pub fn set_rtc_backend_identity(&mut self, identity: String) {
        self.rtc_backend_identity = Some(identity);
    }

    /// Whether two memberships have equal membership data.
    ///
    /// This deliberately ignores the event metadata, like the reference
    /// implementation's `CallMembership.equal`.
    pub fn data_equal(a: &Self, b: &Self) -> bool {
        a.data == b.data
    }

    /// The parsed membership data.
    pub fn data(&self) -> &SessionMembershipData {
        &self.data
    }

    /// The user ID of the member.
    pub fn user_id(&self) -> &str {
        &self.sender
    }

    /// The ID of the event this membership is based on.
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    /// The device ID of the member.
    pub fn device_id(&self) -> &str {
        &self.data.device_id
    }

    /// The application of the session, e.g. `m.call`.
    pub fn application(&self) -> &str {
        &self.data.application
    }

    /// The call ID of the session. `""` for room-wide calls.
    pub fn call_id(&self) -> &str {
        &self.data.call_id
    }

    /// The scope of the call, if any.
    pub fn scope(&self) -> Option<&ruma::events::call::member::CallScope> {
        self.data.scope.as_ref()
    }

    /// The call intent advertised by the member, if any.
    pub fn call_intent(&self) -> Option<&str> {
        self.data.call_intent.as_deref()
    }

    /// The membership ID of the member.
    ///
    /// Falls back to `{user_id}:{device_id}` when the event does not carry an
    /// explicit `membershipID`.
    pub fn membership_id(&self) -> String {
        self.data
            .membership_id
            .clone()
            .unwrap_or_else(|| format!("{}:{}", self.sender, self.data.device_id))
    }

    /// The creation timestamp of the membership, in milliseconds since the
    /// Unix epoch.
    ///
    /// Uses `created_ts` if present, else the `origin_server_ts` of the
    /// event.
    pub fn created_ts(&self) -> u64 {
        self.data.created_ts.unwrap_or(self.origin_server_ts)
    }

    /// The absolute expiry timestamp of the membership, in milliseconds
    /// since the Unix epoch.
    pub fn get_absolute_expiry(&self) -> u64 {
        self.created_ts() + self.data.expires.unwrap_or(DEFAULT_EXPIRE_DURATION_MS)
    }

    /// The number of milliseconds until the membership expires, negative if
    /// it is already expired.
    ///
    /// `now` is the current time in milliseconds since the Unix epoch. The
    /// reference implementation reads the system clock; taking the timestamp
    /// explicitly keeps this crate's logic pure and testable.
    pub fn get_ms_until_expiry(&self, now: u64) -> i64 {
        i64::try_from(self.get_absolute_expiry()).unwrap_or(i64::MAX)
            - i64::try_from(now).unwrap_or(i64::MAX)
    }

    /// Whether the membership is expired at time `now`, in milliseconds
    /// since the Unix epoch.
    pub fn is_expired(&self, now: u64) -> bool {
        self.get_ms_until_expiry(now) <= 0
    }

    /// The list of transports this membership proposes (`foci_preferred`).
    pub fn transports(&self) -> &[Transport] {
        &self.data.foci_preferred
    }

    /// The transport this membership uses to publish media, or `None` if no
    /// transport is available.
    ///
    /// With the `oldest_membership` selection method, this is the first
    /// preferred transport of the oldest membership in the session. With
    /// `multi_sfu`, it is this membership's own first preferred transport.
    /// Unknown selection methods yield `None`.
    pub fn get_transport(&self, oldest_membership: &CallMembership) -> Option<Transport> {
        let selection = self.data.focus_active.focus_selection.as_ref()?;
        match selection.as_str() {
            "oldest_membership" => {
                if Self::data_equal(self, oldest_membership) {
                    self.data.foci_preferred.first().cloned()
                } else {
                    oldest_membership.get_transport(oldest_membership)
                }
            }
            "multi_sfu" => self.data.foci_preferred.first().cloned(),
            _ => None,
        }
    }
}
