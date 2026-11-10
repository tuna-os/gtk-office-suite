// SPDX-License-Identifier: GPL-3.0-or-later

//! LiveKit media connection for MatrixRTC calls (feature `livekit`).
//!
//! This module bridges the pure MatrixRTC logic of this crate to the LiveKit
//! Rust SDK: it fetches the SFU JWT from an MSC4195 `lk-jwt-service`,
//! connects to the LiveKit room with end-to-end encryption enabled, and
//! wires the key ring of the [`RtcEncryptionManager`] into LiveKit's frame
//! cryptor.
//!
//! [`RtcEncryptionManager`]: crate::encryption_manager::RtcEncryptionManager

use livekit::{
    Room, RoomEvent, RoomOptions,
    e2ee::{
        E2eeOptions, EncryptionType,
        key_provider::{KeyProvider, KeyProviderOptions},
    },
    id::{ParticipantIdentity, TrackSid},
    options::TrackPublishOptions,
    track::{LocalAudioTrack, LocalTrack, LocalVideoTrack, TrackSource},
    webrtc::{audio_source::RtcAudioSource, video_source::RtcVideoSource},
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;

use crate::{encryption_manager::KeyRingEntry, key_transport::CallMembershipIdentity};

/// An error from the LiveKit connection layer.
#[derive(Debug, thiserror::Error)]
pub enum LivekitError {
    /// Fetching the SFU configuration from the JWT service failed.
    #[error("failed to fetch the SFU configuration: {0}")]
    SfuConfig(String),

    /// An HTTP error while talking to the JWT service.
    #[error("HTTP error while talking to the JWT service: {0}")]
    Http(#[from] reqwest::Error),

    /// An error from the LiveKit SDK.
    #[error("LiveKit error: {0}")]
    Room(#[from] livekit::RoomError),
}

/// A Matrix OpenID token, as returned by
/// `POST /_matrix/client/v3/user/{userId}/openid/request_token`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenIdToken {
    /// The access token.
    pub access_token: String,
    /// The token type, always `Bearer`.
    pub token_type: String,
    /// The homeserver domain the token belongs to.
    pub matrix_server_name: String,
    /// The number of seconds until the token expires.
    pub expires_in: u64,
}

/// The SFU access configuration returned by the MSC4195 JWT service.
#[derive(Clone, Debug, Deserialize)]
pub struct SfuConfig {
    /// The websocket URL of the LiveKit SFU.
    pub url: String,
    /// The JWT to authenticate with the SFU.
    pub jwt: String,
}

/// Fetch the SFU configuration (URL + JWT) from an MSC4195 `lk-jwt-service`.
///
/// This uses the legacy `POST {service_url}/sfu/get` endpoint, sending the
/// Matrix OpenID token, the Matrix room ID and our device ID, matching
/// Element Call's `getLiveKitJWT`. The service validates the OpenID token
/// against the homeserver and maps the Matrix room ID to a LiveKit room
/// alias.
///
/// The Matrix 2.0 `POST {service_url}/get_token` endpoint (hashed member
/// identities, delayed-event delegation) is not implemented yet; it only
/// matters together with MSC4354 sticky events.
pub async fn fetch_sfu_config(
    http: &reqwest::Client,
    service_url: &str,
    room_id: &str,
    device_id: &str,
    openid_token: &OpenIdToken,
) -> Result<SfuConfig, LivekitError> {
    let response = http
        .post(format!("{}/sfu/get", service_url.trim_end_matches('/')))
        .json(&serde_json::json!({
            "room": room_id,
            "openid_token": openid_token,
            "device_id": device_id,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(LivekitError::SfuConfig(format!(
            "SFU config fetch failed with status code {status}: {body}"
        )));
    }

    Ok(response.json().await?)
}

/// A connection to a LiveKit SFU for a MatrixRTC call.
///
/// The connection owns the LiveKit room and the per-participant key
/// provider. Media keys managed by the
/// [`RtcEncryptionManager`](crate::encryption_manager::RtcEncryptionManager)
/// are injected with [`Self::set_participant_key`], keyed by the RTC backend
/// identity (the LiveKit participant identity).
pub struct LivekitCallConnection {
    room: Room,
    key_provider: KeyProvider,
}

impl LivekitCallConnection {
    /// Connect to the LiveKit room described by `sfu_config`, with
    /// end-to-end encryption (external key provider) enabled.
    ///
    /// Returns the connection and the stream of LiveKit room events
    /// (including `TrackSubscribed` for remote tracks).
    #[allow(clippy::large_futures)]
    pub async fn connect(
        sfu_config: &SfuConfig,
    ) -> Result<(Self, mpsc::UnboundedReceiver<RoomEvent>), LivekitError> {
        // MatrixRTC keys are full encryption keys, not ratcheting material:
        // match Element Call's external key provider configuration.
        let key_provider = KeyProvider::new(KeyProviderOptions {
            ratchet_window_size: 0,
            ..Default::default()
        });

        // `RoomOptions` is non-exhaustive, so it cannot be built with a
        // struct expression.
        let mut options = RoomOptions::default();
        options.encryption = Some(E2eeOptions {
            encryption_type: EncryptionType::Gcm,
            key_provider: key_provider.clone(),
        });

        let (room, events) = Room::connect(&sfu_config.url, &sfu_config.jwt, options).await?;
        info!("Connected to LiveKit room {}", room.name());

        Ok((Self { room, key_provider }, events))
    }

    /// The underlying LiveKit room.
    pub fn room(&self) -> &Room {
        &self.room
    }

    /// The LiveKit participant identity of the local participant.
    pub fn local_identity(&self) -> String {
        self.room.local_participant().identity().to_string()
    }

    /// Set the media key of a participant, keyed by their RTC backend
    /// identity (which is the LiveKit participant identity).
    ///
    /// Call this for every
    /// [`EncryptionKeyChanged`](crate::call_session::RtcCallSessionEvent::EncryptionKeyChanged)
    /// event of the session, including our own keys.
    pub fn set_participant_key(&self, rtc_backend_identity: &str, key_index: u32, key: Vec<u8>) {
        self.key_provider.set_key(
            &ParticipantIdentity(rtc_backend_identity.to_owned()),
            i32::try_from(key_index).unwrap_or(0),
            key,
        );
    }

    /// Apply a whole key ring (e.g. from
    /// [`RtcEncryptionManager::get_encryption_keys`](crate::encryption_manager::RtcEncryptionManager::get_encryption_keys))
    /// at once. Useful when the connection is established after keys have
    /// already been received.
    pub fn apply_key_ring<'a>(&self, entries: impl IntoIterator<Item = &'a KeyRingEntry>) {
        for entry in entries {
            self.set_participant_key(
                &entry.rtc_backend_identity,
                entry.key_index,
                entry.key.clone(),
            );
        }
    }

    /// Publish a microphone audio track backed by the given source.
    ///
    /// Capturing from an actual audio device is the application's
    /// responsibility: feed PCM frames into the source.
    pub async fn publish_microphone_track(
        &self,
        source: RtcAudioSource,
    ) -> Result<(), LivekitError> {
        let track = LocalAudioTrack::create_audio_track("microphone", source);
        self.room
            .local_participant()
            .publish_track(
                LocalTrack::Audio(track),
                TrackPublishOptions {
                    source: TrackSource::Microphone,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    /// Publish a camera video track backed by the given source.
    ///
    /// Capturing from an actual camera is the application's responsibility:
    /// feed video frames into the source.
    ///
    /// Returns the SID of the published track, to stop publishing it later
    /// with [`Self::unpublish_track`].
    pub async fn publish_camera_track(
        &self,
        source: RtcVideoSource,
    ) -> Result<TrackSid, LivekitError> {
        let track = LocalVideoTrack::create_video_track("camera", source);
        let publication = self
            .room
            .local_participant()
            .publish_track(
                LocalTrack::Video(track),
                TrackPublishOptions {
                    source: TrackSource::Camera,
                    ..Default::default()
                },
            )
            .await?;
        Ok(publication.sid())
    }

    /// Stop publishing the local track with the given SID.
    ///
    /// This removes the track from the SFU, so that other participants stop
    /// receiving it, unlike muting it locally.
    pub async fn unpublish_track(&self, sid: &TrackSid) -> Result<(), LivekitError> {
        self.room.local_participant().unpublish_track(sid).await?;
        Ok(())
    }

    /// Disconnect from the LiveKit room.
    pub async fn disconnect(&self) -> Result<(), LivekitError> {
        self.room.close().await?;
        Ok(())
    }
}

/// Helper mapping a session member identity to its expected legacy LiveKit
/// participant identity (`{user_id}:{device_id}`).
pub fn legacy_participant_identity(membership: &CallMembershipIdentity) -> String {
    format!("{}:{}", membership.user_id, membership.device_id)
}
