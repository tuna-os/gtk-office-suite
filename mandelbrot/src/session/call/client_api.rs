//! Implementation of the `MatrixRTC` client API over the Matrix Rust SDK.

use std::time::Duration;

use mandelbrot_matrixrtc::{
    ClientError, RtcClientApi, SendDelayedEventResponse, SendEventResponse, ToDeviceTarget,
    UpdateDelayedEventAction,
};
use matrix_sdk::Client;
use matrix_sdk_base::crypto::CollectStrategy;
use ruma::{
    OwnedDeviceId, RoomId, UserId,
    api::{
        client::delayed_events::{DelayParameters, delayed_state_event, update_delayed_event},
        error::{ErrorKind, RetryAfter},
    },
    events::{AnyStateEventContent, AnyToDeviceEventContent, StateEventType},
    serde::Raw,
};
use serde_json::Value as JsonValue;
use tracing::warn;

/// [`RtcClientApi`] implementation over the Matrix Rust SDK [`Client`].
#[derive(Debug, Clone)]
pub(crate) struct SdkRtcClientApi {
    client: Client,
}

impl SdkRtcClientApi {
    /// Construct the client API for the given SDK client.
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// Convert an error of the client API into a [`ClientError`], mapping
    /// the error kinds the `MatrixRTC` engine makes decisions on.
    fn map_api_error(error: Option<&ruma::api::error::Error>, fallback: &str) -> ClientError {
        let Some(error) = error else {
            // No structured Matrix error: most likely a network problem.
            return ClientError::Other(fallback.to_owned());
        };

        let status = error.status_code.as_u16();
        let (errcode, retry_after) = match error.error_kind() {
            Some(ErrorKind::NotFound) => ("M_NOT_FOUND".to_owned(), None),
            Some(ErrorKind::LimitExceeded(data)) => (
                "M_LIMIT_EXCEEDED".to_owned(),
                match &data.retry_after {
                    Some(RetryAfter::Delay(delay)) => Some(*delay),
                    _ => None,
                },
            ),
            Some(ErrorKind::Unrecognized) => ("M_UNRECOGNIZED".to_owned(), None),
            Some(kind) => (format!("{kind:?}"), None),
            None => ("M_UNKNOWN".to_owned(), None),
        };

        ClientError::Matrix {
            errcode,
            http_status: Some(status),
            retry_after,
            max_delay: None,
        }
    }

    /// Convert an SDK error into a [`ClientError`].
    fn map_error(error: &matrix_sdk::Error) -> ClientError {
        Self::map_api_error(error.as_client_api_error(), &error.to_string())
    }

    /// Convert an SDK HTTP error into a [`ClientError`].
    fn map_http_error(error: &matrix_sdk::HttpError) -> ClientError {
        Self::map_api_error(error.as_client_api_error(), &error.to_string())
    }

    /// Convert an SDK HTTP error from an MSC4140 endpoint into a
    /// [`ClientError`], detecting unsupported endpoints.
    fn map_delayed_error(error: &matrix_sdk::HttpError) -> ClientError {
        if error.is_endpoint_not_implemented() {
            return ClientError::UnsupportedDelayedEventsEndpoint;
        }
        Self::map_http_error(error)
    }
}

#[async_trait::async_trait]
impl RtcClientApi for SdkRtcClientApi {
    async fn send_state_event(
        &self,
        room_id: &RoomId,
        event_type: StateEventType,
        state_key: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError> {
        let room = self
            .client
            .get_room(room_id)
            .ok_or_else(|| ClientError::Other(format!("unknown room {room_id}")))?;

        let response = room
            .send_state_event_raw(&event_type.to_string(), state_key, content)
            .await
            .map_err(|error| Self::map_error(&error))?;

        Ok(SendEventResponse {
            event_id: response.event_id,
        })
    }

    async fn send_delayed_state_event(
        &self,
        room_id: &RoomId,
        delay: Duration,
        event_type: StateEventType,
        state_key: &str,
        content: JsonValue,
    ) -> Result<SendDelayedEventResponse, ClientError> {
        let raw_content = serde_json::value::to_raw_value(&content)
            .map_err(|error| ClientError::Other(error.to_string()))?;

        let request = delayed_state_event::unstable::Request::new_raw(
            room_id.to_owned(),
            state_key.to_owned(),
            event_type,
            DelayParameters::Timeout { timeout: delay },
            Raw::<AnyStateEventContent>::from_json(raw_content),
        );

        let response = self
            .client
            .send(request)
            .await
            .map_err(|error| Self::map_delayed_error(&error))?;

        Ok(SendDelayedEventResponse {
            delay_id: response.delay_id,
        })
    }

    async fn update_delayed_event(
        &self,
        delay_id: &str,
        action: UpdateDelayedEventAction,
    ) -> Result<(), ClientError> {
        let action = match action {
            UpdateDelayedEventAction::Restart => update_delayed_event::UpdateAction::Restart,
            UpdateDelayedEventAction::Cancel => update_delayed_event::UpdateAction::Cancel,
            UpdateDelayedEventAction::Send => update_delayed_event::UpdateAction::Send,
        };
        // `unstable_v1` is the same MSC4140 endpoint (action in the body, access token
        // required) that ruma <0.17 exposed as `update_delayed_event::unstable`.
        let request = update_delayed_event::unstable_v1::Request::new(delay_id.to_owned(), action);

        self.client
            .send(request)
            .await
            .map_err(|error| Self::map_delayed_error(&error))?;

        Ok(())
    }

    async fn send_event(
        &self,
        room_id: &RoomId,
        event_type: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError> {
        let room = self
            .client
            .get_room(room_id)
            .ok_or_else(|| ClientError::Other(format!("unknown room {room_id}")))?;

        let result = room
            .send_raw(event_type, content)
            .await
            .map_err(|error| Self::map_error(&error))?;

        Ok(SendEventResponse {
            event_id: result.response.event_id,
        })
    }

    async fn encrypt_and_send_to_device(
        &self,
        event_type: &str,
        targets: &[ToDeviceTarget],
        content: JsonValue,
    ) -> Result<(), ClientError> {
        let encryption = self.client.encryption();

        let mut devices = Vec::new();
        for target in targets {
            let Ok(user_id) = UserId::parse(&target.user_id) else {
                warn!("Invalid user ID for to-device target: {}", target.user_id);
                continue;
            };
            let device_id = OwnedDeviceId::from(target.device_id.as_str());

            match encryption.get_device(&user_id, &device_id).await {
                Ok(Some(device)) => devices.push(device),
                Ok(None) => {
                    warn!("Unknown device {user_id}:{device_id}, cannot send call keys to it");
                }
                Err(error) => {
                    warn!("Failed to look up device {user_id}:{device_id}: {error}");
                }
            }
        }

        if devices.is_empty() {
            return Ok(());
        }

        let raw_content = serde_json::value::to_raw_value(&content)
            .map_err(|error| ClientError::Other(error.to_string()))?;

        let failures = encryption
            .encrypt_and_send_raw_to_device(
                devices.iter().collect(),
                event_type,
                Raw::<AnyToDeviceEventContent>::from_json(raw_content),
                CollectStrategy::AllDevices,
            )
            .await
            .map_err(|error| ClientError::Other(error.to_string()))?;

        for (user_id, device_id) in failures {
            warn!("Failed to send call keys to {user_id}:{device_id}");
        }

        Ok(())
    }
}
