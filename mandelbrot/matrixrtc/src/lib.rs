// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure-logic MatrixRTC session and membership management for Mandelbrot.
//!
//! This crate reimplements the session/membership logic of `matrix-js-sdk`'s
//! `matrixrtc` module in Rust. It does not perform any I/O itself: the
//! application provides an implementation of [`RtcClientApi`] on top of its
//! Matrix stack, and feeds room state into [`MatrixRtcSession`].
//!
//! The current scope is membership management only (no encryption manager,
//! no LiveKit integration):
//!
//! - [`CallMembership`] parses and validates `m.call.member` state events in
//!   the [`SessionMembershipData`] format.
//! - [`MembershipManager`] is the join/leave state machine with MSC4140 delayed
//!   leave events.
//! - [`MatrixRtcSession`] builds the membership list of the room call from room
//!   state.

pub mod call_membership;
pub mod call_session;
pub mod client;
pub mod encryption_manager;
pub mod key_transport;
#[cfg(feature = "livekit")]
pub mod livekit_connection;
pub mod membership_data;
pub mod membership_manager;
pub mod outdated_key_filter;
pub mod session;

pub use call_membership::{CallMembership, MemberStateEvent};
pub use call_session::{
    RTC_NOTIFICATION_EVENT_TYPE, RtcCallSession, RtcCallSessionConfig, RtcCallSessionEvent,
    SessionActivity, SessionActivityTracker,
};
pub use client::{
    ClientError, RtcClientApi, SendDelayedEventResponse, SendEventResponse, ToDeviceEvent,
    ToDeviceTarget, UpdateDelayedEventAction,
};
pub use encryption_manager::{
    EncryptionConfig, KeyRingEntry, RtcEncryptionManager, decode_base64, encode_unpadded_base64,
};
pub use key_transport::{
    CALL_ENCRYPTION_KEYS_EVENT_TYPE, CallMembershipIdentity, KeyTransport, MalformedKeyEvent,
    ParticipantDeviceInfo, ReceivedKeyEvent, Statistics, ToDeviceKeyTransport,
    encryption_key_map_key,
};
pub use membership_data::{
    DEFAULT_EXPIRE_DURATION_MS, FocusActive, MembershipParseError, SessionMembershipData,
    Transport, parse_session_membership_data,
};
pub use membership_manager::{
    MembershipConfig, MembershipManager, MembershipManagerEvent, RtcRoom, Status,
};
pub use outdated_key_filter::{InboundEncryptionSession, OutdatedKeyFilter};
pub use session::{MatrixRtcSession, SlotDescription};
// Re-exports for consumers of the `livekit` feature, so that they don't
// need their own compatible versions of these dependencies.
#[cfg(feature = "livekit")]
pub use {livekit, reqwest};
