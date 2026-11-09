// SPDX-License-Identifier: GPL-3.0-or-later

//! The client API surface required by the MatrixRTC logic.
//!
//! This mirrors the subset of `matrix-js-sdk`'s `MatrixClient` that the
//! reference `MembershipManager` implementation uses. The application is
//! expected to implement [`RtcClientApi`] on top of its Matrix stack.

use std::time::Duration;

use ruma::{OwnedEventId, RoomId, events::StateEventType};
use serde_json::Value as JsonValue;

/// The response to sending an event.
#[derive(Clone, Debug)]
pub struct SendEventResponse {
    /// The ID of the sent event.
    pub event_id: OwnedEventId,
}

/// The response to scheduling a delayed event (MSC4140).
#[derive(Clone, Debug)]
pub struct SendDelayedEventResponse {
    /// The ID of the scheduled delayed event.
    pub delay_id: String,
}

/// The possible actions on a scheduled delayed event (MSC4140).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateDelayedEventAction {
    /// Restart the delay timer of the event.
    Restart,
    /// Cancel the scheduled event.
    Cancel,
    /// Send the scheduled event now.
    Send,
}

/// An error returned by an [`RtcClientApi`] implementation.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ClientError {
    /// The homeserver does not support the MSC4140 delayed events endpoint.
    #[error("server does not support the delayed events endpoint")]
    UnsupportedDelayedEventsEndpoint,

    /// A standard Matrix error response.
    #[error("Matrix error {errcode} (HTTP status {http_status:?})")]
    Matrix {
        /// The Matrix `errcode`, e.g. `M_NOT_FOUND`.
        errcode: String,
        /// The HTTP status code, if known.
        http_status: Option<u16>,
        /// The `Retry-After` duration for rate limit errors, if provided.
        retry_after: Option<Duration>,
        /// The `org.matrix.msc4140.max_delay` value for
        /// `M_MAX_DELAY_EXCEEDED` errors.
        max_delay: Option<Duration>,
    },

    /// A non-Matrix HTTP error.
    #[error("HTTP error with status {status}")]
    Http {
        /// The HTTP status code.
        status: u16,
    },

    /// A network connection error.
    #[error("network connection error")]
    Connection,

    /// A local timeout while waiting for the server response.
    #[error("local timeout while waiting for the server response")]
    LocalTimeout,

    /// Any other error.
    #[error("{0}")]
    Other(String),
}

impl ClientError {
    /// Construct an `M_NOT_FOUND` Matrix error.
    pub fn not_found() -> Self {
        Self::Matrix {
            errcode: "M_NOT_FOUND".to_owned(),
            http_status: Some(404),
            retry_after: None,
            max_delay: None,
        }
    }

    /// Construct an `M_LIMIT_EXCEEDED` Matrix error with an optional
    /// `Retry-After` duration.
    pub fn rate_limited(retry_after: Option<Duration>) -> Self {
        Self::Matrix {
            errcode: "M_LIMIT_EXCEEDED".to_owned(),
            http_status: Some(429),
            retry_after,
            max_delay: None,
        }
    }

    /// Construct an MSC4140 `M_MAX_DELAY_EXCEEDED` error.
    pub fn max_delay_exceeded(max_delay: Duration) -> Self {
        Self::Matrix {
            errcode: "M_UNKNOWN".to_owned(),
            http_status: Some(400),
            retry_after: None,
            max_delay: Some(max_delay),
        }
    }

    /// Whether this is an `M_NOT_FOUND` error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Matrix { errcode, .. } if errcode == "M_NOT_FOUND")
    }

    /// Whether this is a rate limit error.
    pub fn is_rate_limit(&self) -> bool {
        match self {
            Self::Matrix {
                errcode,
                http_status,
                ..
            } => errcode == "M_LIMIT_EXCEEDED" || *http_status == Some(429),
            Self::Http { status } => *status == 429,
            _ => false,
        }
    }

    /// The `Retry-After` duration of a rate limit error, if provided.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Matrix { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// The maximum allowed delay of an `M_MAX_DELAY_EXCEEDED` error.
    pub fn max_delay(&self) -> Option<Duration> {
        match self {
            Self::Matrix {
                errcode, max_delay, ..
            } if errcode == "M_UNKNOWN" => *max_delay,
            _ => None,
        }
    }

    /// Whether this error should be retried as a transient network error.
    pub fn is_network_error(&self) -> bool {
        match self {
            Self::Connection | Self::LocalTimeout => true,
            Self::Matrix {
                http_status: Some(status),
                ..
            }
            | Self::Http { status } => (500..600).contains(status),
            _ => false,
        }
    }
}

/// A target device for an encrypted to-device message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToDeviceTarget {
    /// The user ID of the target.
    pub user_id: String,
    /// The device ID of the target.
    pub device_id: String,
}

/// An incoming to-device event, as fed into the key transport by the
/// application.
#[derive(Clone, Debug)]
pub struct ToDeviceEvent {
    /// The (verified) sender of the event.
    pub sender: String,
    /// The type of the event.
    pub event_type: String,
    /// The (decrypted) content of the event.
    pub content: serde_json::Value,
}

/// The client API used to send MatrixRTC membership events.
///
/// This mirrors the mock surface used by the `matrix-js-sdk` MatrixRTC test
/// suite: `sendStateEvent`, `_unstable_sendDelayedStateEvent`,
/// `_unstable_updateDelayedEvent` (restart/cancel/send), `sendEvent` and
/// `encryptAndSendToDevice`.
#[async_trait::async_trait]
pub trait RtcClientApi: Send + Sync {
    /// Send a state event to the given room.
    async fn send_state_event(
        &self,
        room_id: &RoomId,
        event_type: StateEventType,
        state_key: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError>;

    /// Schedule a delayed state event (MSC4140) in the given room.
    async fn send_delayed_state_event(
        &self,
        room_id: &RoomId,
        delay: Duration,
        event_type: StateEventType,
        state_key: &str,
        content: JsonValue,
    ) -> Result<SendDelayedEventResponse, ClientError>;

    /// Update a scheduled delayed event (MSC4140).
    async fn update_delayed_event(
        &self,
        delay_id: &str,
        action: UpdateDelayedEventAction,
    ) -> Result<(), ClientError>;

    /// Send a message-like event to the given room.
    async fn send_event(
        &self,
        room_id: &RoomId,
        event_type: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError>;

    /// Encrypt and send a to-device event to the given target devices.
    async fn encrypt_and_send_to_device(
        &self,
        event_type: &str,
        targets: &[ToDeviceTarget],
        content: JsonValue,
    ) -> Result<(), ClientError>;
}
