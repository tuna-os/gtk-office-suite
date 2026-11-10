// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire format types for `m.call.member` session membership data.
//!
//! This is the "session" (legacy MSC4143 state event) membership format used
//! by `org.matrix.msc3401.call.member` state events.
//!
//! Ruma's typed [`ruma::events::call::member::SessionMembershipData`] cannot
//! represent everything the reference implementation accepts on the wire (a
//! missing `expires` field, the `membershipID` field, non-`m.call`
//! applications and foci of unknown types), so this module defines its own
//! serde types and reuses ruma's focus/scope types for the typed views.

use ruma::events::call::member::{ActiveLivekitFocus, CallScope, FocusSelection, LivekitFocus};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// The default duration in milliseconds that a membership is considered valid
/// for.
///
/// Ordinarily the client responsible for the session will update the
/// membership before it expires. We use this duration as the fallback case
/// where stale sessions are present for some reason.
pub const DEFAULT_EXPIRE_DURATION_MS: u64 = 1000 * 60 * 60 * 4;

/// A transport (focus) description as found in `foci_preferred`.
///
/// Only the `type` field is required; all other fields are kept as-is so that
/// unknown focus types survive a parse/serialize roundtrip. Use
/// [`Transport::as_livekit`] for a typed view on LiveKit transports.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Transport {
    /// The type of the transport, e.g. `livekit`.
    #[serde(rename = "type")]
    pub transport_type: String,

    /// All other, transport type specific, fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, JsonValue>,
}

impl Transport {
    /// Construct a LiveKit transport from ruma's [`LivekitFocus`].
    pub fn from_livekit(focus: &LivekitFocus) -> Self {
        let mut extra = serde_json::Map::new();
        extra.insert("livekit_alias".to_owned(), focus.alias.clone().into());
        extra.insert(
            "livekit_service_url".to_owned(),
            focus.service_url.clone().into(),
        );
        Self {
            transport_type: "livekit".to_owned(),
            extra,
        }
    }

    /// A typed view of this transport as ruma's [`LivekitFocus`], if it is a
    /// complete LiveKit focus description.
    pub fn as_livekit(&self) -> Option<LivekitFocus> {
        if self.transport_type != "livekit" {
            return None;
        }
        let alias = self.extra.get("livekit_alias")?.as_str()?;
        let service_url = self.extra.get("livekit_service_url")?.as_str()?;
        Some(LivekitFocus::new(alias.to_owned(), service_url.to_owned()))
    }
}

/// The `focus_active` field of a session membership.
///
/// The reference implementation only requires `type` to be a string; the
/// selection method is optional and can be an unknown value. The selection
/// method reuses ruma's [`FocusSelection`] string enum, which preserves
/// unknown values.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FocusActive {
    /// The type of the active focus, e.g. `livekit`.
    #[serde(rename = "type")]
    pub focus_type: String,

    /// How the focus is selected, e.g. `oldest_membership`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_selection: Option<FocusSelection>,

    /// All other, focus type specific, fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, JsonValue>,
}

impl FocusActive {
    /// The default `oldest_membership` LiveKit focus selection.
    pub fn livekit_oldest_membership() -> Self {
        Self::from(&ActiveLivekitFocus::new())
    }
}

impl From<&ActiveLivekitFocus> for FocusActive {
    fn from(focus: &ActiveLivekitFocus) -> Self {
        Self {
            focus_type: "livekit".to_owned(),
            focus_selection: Some(focus.focus_selection.clone()),
            extra: serde_json::Map::new(),
        }
    }
}

/// (MatrixRTC) session membership data.
///
/// Represents the content of an `org.matrix.msc3401.call.member` state event
/// as it is on the wire.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionMembershipData {
    /// The RTC application, defining the type of the RTC session, e.g.
    /// `m.call`.
    pub application: String,

    /// The ID of this session. A room-wide session uses `""`.
    pub call_id: String,

    /// The Matrix device ID of this session.
    pub device_id: String,

    /// The focus selection system this membership is using.
    pub focus_active: FocusActive,

    /// A list of possible foci this user knows about.
    #[serde(default)]
    pub foci_preferred: Vec<Transport>,

    /// The creation time of the session, in milliseconds since the Unix
    /// epoch. If it is `None` the creation time is the `origin_server_ts` of
    /// the event itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_ts: Option<u64>,

    /// If the application is `m.call`, whether it is a room or user owned
    /// call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<CallScope>,

    /// A delta, in milliseconds, to `created_ts` that defines when the
    /// membership is expired/invalid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<u64>,

    /// The intent of the call from the perspective of this user, e.g.
    /// `audio` or `video`.
    #[serde(rename = "m.call.intent", skip_serializing_if = "Option::is_none")]
    pub call_intent: Option<String>,

    /// The ID used on the media backend. Other clients treat a missing value
    /// as `{user_id}:{device_id}`.
    #[serde(rename = "membershipID", skip_serializing_if = "Option::is_none")]
    pub membership_id: Option<String>,
}

/// An error that occurred while parsing membership data.
#[derive(Clone, Debug, thiserror::Error)]
pub enum MembershipParseError {
    /// The event has no sender.
    #[error("event is missing the sender field")]
    MissingSender,

    /// The content is not valid [`SessionMembershipData`].
    #[error("invalid SessionMembershipData: {0}")]
    InvalidContent(String),

    /// The content is empty, i.e. this is a "leave" state event.
    #[error("the membership content is empty (left membership)")]
    Empty,

    /// The content uses the long-deprecated `memberships` array format.
    #[error("the membership content uses the deprecated memberships array format")]
    DeprecatedFormat,
}

/// Parse and validate the content of an `m.call.member` state event.
///
/// This mirrors the reference implementation's quick filter (empty and
/// deprecated `memberships` array contents are rejected with dedicated
/// errors) plus `checkSessionsMembershipData`.
pub fn parse_session_membership_data(
    content: &JsonValue,
) -> Result<SessionMembershipData, MembershipParseError> {
    let object = content
        .as_object()
        .ok_or_else(|| MembershipParseError::InvalidContent("content is not an object".into()))?;

    if object.is_empty() {
        return Err(MembershipParseError::Empty);
    }
    if object.len() == 1 && object.contains_key("memberships") {
        return Err(MembershipParseError::DeprecatedFormat);
    }

    serde_json::from_value(content.clone())
        .map_err(|error| MembershipParseError::InvalidContent(error.to_string()))
}
