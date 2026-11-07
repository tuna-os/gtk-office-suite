// SPDX-License-Identifier: GPL-3.0-or-later

//! Example: join the MatrixRTC room call of a room and connect to the
//! LiveKit SFU.
//!
//! ```sh
//! printf '%s\n' "$ACCESS_TOKEN" | cargo run --features livekit --example join_call -- \
//!     https://matrix.example.org @alice:example.org DEVICEID '!room:example.org' \
//!     [--access-token-file PATH] \
//!     [--focus http://livekit-jwt.example.org] \
//!     [--assert-peer '@bob:example.org:DEVICEID' --assert-timeout 90]
//! ```
//!
//! The access token is read from stdin (the whole input, trimmed), never from
//! the command line, where other local users and shell history could see it.
//!
//! Options:
//! - `--access-token-file PATH`: read the access token from `PATH` instead of
//!   stdin. `-` means stdin.
//! - `--focus URL`: the MSC4195 LiveKit JWT service URL to advertise as our
//!   preferred focus. Required when we are the first participant (otherwise the
//!   focus is taken from the oldest membership of the session).
//! - `--assert-peer MEMBER_ID`: assertion mode for the e2e interop test (see
//!   `tests/e2e/` in the repository root). `MEMBER_ID` is
//!   `{user_id}:{device_id}`. The example prints `ASSERTIONS PASSED` once it
//!   has (a) seen the peer's call membership, (b) subscribed to a media track
//!   of the peer, (c) received an encryption key from the peer, (d) seen the
//!   frame cryptor of the peer reach the `Ok` state, and (e) received audio
//!   frames from the peer. If this does not happen within `--assert-timeout`
//!   seconds (default 90), it prints `ASSERTIONS FAILED` and exits 1. After
//!   passing it keeps running (so the test can exercise graceful/ungraceful
//!   leave) until SIGINT.
//!
//! This is a development harness, not a complete client:
//! - To-device messages are sent WITHOUT encryption (the real client must use
//!   Olm via its crypto stack), so media of other participants will not be
//!   decryptable by clients that require encrypted key transport.
//! - It publishes a microphone track fed with silence instead of capturing a
//!   device.
//! - It polls `/sync` + the room state instead of using a proper sync loop.

#![allow(clippy::too_many_lines, clippy::large_futures)]

use std::{
    collections::HashMap,
    io::IsTerminal,
    process::exit,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::StreamExt;
use mandelbrot_matrixrtc::{
    CallMembershipIdentity, ClientError, MemberStateEvent, RtcCallSession, RtcCallSessionConfig,
    RtcCallSessionEvent, RtcClientApi, RtcRoom, SendDelayedEventResponse, SendEventResponse,
    ToDeviceEvent, ToDeviceTarget, Transport, UpdateDelayedEventAction,
    livekit_connection::{LivekitCallConnection, OpenIdToken, fetch_sfu_config},
};
use ruma::{OwnedEventId, RoomId, events::StateEventType};
use serde_json::{Value as JsonValue, json};
use zeroize::Zeroizing;

#[path = "support/join_call_args.rs"]
mod join_call_args;

use join_call_args::{AccessTokenSource, Args};

/// A minimal, plain-HTTP implementation of [`RtcClientApi`] against the
/// Matrix client-server API. Development only.
struct HttpClientApi {
    http: reqwest::Client,
    homeserver: String,
    access_token: Zeroizing<String>,
    user_id: String,
}

impl HttpClientApi {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.homeserver.trim_end_matches('/'))
    }

    fn txn_id() -> String {
        format!(
            "mandelbrot{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    }

    async fn request(
        &self,
        method: reqwest::Method,
        url: String,
        query: &[(String, String)],
        body: Option<&JsonValue>,
    ) -> Result<JsonValue, ClientError> {
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.access_token.as_str())
            .query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|e| ClientError::Other(e.to_string()))?;

        let status = response.status().as_u16();
        let value: JsonValue = response.json().await.unwrap_or_default();
        if (200..300).contains(&status) {
            return Ok(value);
        }

        let errcode = value
            .get("errcode")
            .and_then(JsonValue::as_str)
            .unwrap_or("M_UNKNOWN");
        if status == 404 && errcode == "M_UNRECOGNIZED" {
            return Err(ClientError::UnsupportedDelayedEventsEndpoint);
        }
        Err(ClientError::Matrix {
            errcode: errcode.to_owned(),
            http_status: Some(status),
            retry_after: value
                .get("retry_after_ms")
                .and_then(JsonValue::as_u64)
                .map(Duration::from_millis),
            max_delay: value
                .get("org.matrix.msc4140.max_delay")
                .and_then(JsonValue::as_u64)
                .map(Duration::from_millis),
        })
    }

    async fn get_openid_token(&self) -> Result<OpenIdToken, ClientError> {
        let value = self
            .request(
                reqwest::Method::POST,
                self.url(&format!(
                    "/_matrix/client/v3/user/{}/openid/request_token",
                    self.user_id
                )),
                &[],
                Some(&json!({})),
            )
            .await?;
        serde_json::from_value(value).map_err(|e| ClientError::Other(e.to_string()))
    }

    async fn get_call_member_events(
        &self,
        room_id: &str,
    ) -> Result<Vec<MemberStateEvent>, ClientError> {
        let value = self
            .request(
                reqwest::Method::GET,
                self.url(&format!("/_matrix/client/v3/rooms/{room_id}/state")),
                &[],
                None,
            )
            .await?;
        let events = value.as_array().cloned().unwrap_or_default();
        Ok(events
            .iter()
            .filter(|e| {
                e.get("type").and_then(JsonValue::as_str) == Some("org.matrix.msc3401.call.member")
            })
            .map(|e| MemberStateEvent {
                event_id: e
                    .get("event_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                sender: e
                    .get("sender")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                origin_server_ts: e
                    .get("origin_server_ts")
                    .and_then(JsonValue::as_u64)
                    .unwrap_or_default(),
                state_key: e
                    .get("state_key")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                content: e.get("content").cloned().unwrap_or_default(),
            })
            .collect())
    }

    /// One `/sync` iteration. Returns the next batch token and the received
    /// to-device events.
    async fn sync_once(
        &self,
        since: Option<&str>,
    ) -> Result<(String, Vec<ToDeviceEvent>), ClientError> {
        let mut query = vec![
            ("timeout".to_owned(), "1500".to_owned()),
            (
                "filter".to_owned(),
                r#"{"room":{"timeline":{"limit":1}}}"#.to_owned(),
            ),
        ];
        if let Some(since) = since {
            query.push(("since".to_owned(), since.to_owned()));
        }
        let value = self
            .request(
                reqwest::Method::GET,
                self.url("/_matrix/client/v3/sync"),
                &query,
                None,
            )
            .await?;

        let next_batch = value
            .get("next_batch")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_owned();
        let to_device = value
            .get("to_device")
            .and_then(|d| d.get("events"))
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|e| ToDeviceEvent {
                sender: e
                    .get("sender")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                event_type: e
                    .get("type")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                content: e.get("content").cloned().unwrap_or_default(),
            })
            .collect();
        Ok((next_batch, to_device))
    }
}

#[async_trait::async_trait]
impl RtcClientApi for HttpClientApi {
    async fn send_state_event(
        &self,
        room_id: &RoomId,
        event_type: StateEventType,
        state_key: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError> {
        let value = self
            .request(
                reqwest::Method::PUT,
                self.url(&format!(
                    "/_matrix/client/v3/rooms/{room_id}/state/{event_type}/{state_key}"
                )),
                &[],
                Some(&content),
            )
            .await?;
        let event_id = value
            .get("event_id")
            .and_then(JsonValue::as_str)
            .unwrap_or("$unknown:example.org");
        Ok(SendEventResponse {
            event_id: OwnedEventId::try_from(event_id)
                .map_err(|e| ClientError::Other(e.to_string()))?,
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
        let value = self
            .request(
                reqwest::Method::PUT,
                self.url(&format!(
                    "/_matrix/client/v3/rooms/{room_id}/state/{event_type}/{state_key}"
                )),
                &[(
                    "org.matrix.msc4140.delay".to_owned(),
                    delay.as_millis().to_string(),
                )],
                Some(&content),
            )
            .await?;
        Ok(SendDelayedEventResponse {
            delay_id: value
                .get("delay_id")
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
    }

    async fn update_delayed_event(
        &self,
        delay_id: &str,
        action: UpdateDelayedEventAction,
    ) -> Result<(), ClientError> {
        let action = match action {
            UpdateDelayedEventAction::Restart => "restart",
            UpdateDelayedEventAction::Cancel => "cancel",
            UpdateDelayedEventAction::Send => "send",
        };
        self.request(
            reqwest::Method::POST,
            self.url(&format!(
                "/_matrix/client/unstable/org.matrix.msc4140/delayed_events/{delay_id}"
            )),
            &[],
            Some(&json!({ "action": action })),
        )
        .await?;
        Ok(())
    }

    async fn send_event(
        &self,
        room_id: &RoomId,
        event_type: &str,
        content: JsonValue,
    ) -> Result<SendEventResponse, ClientError> {
        let value = self
            .request(
                reqwest::Method::PUT,
                self.url(&format!(
                    "/_matrix/client/v3/rooms/{room_id}/send/{event_type}/{}",
                    Self::txn_id()
                )),
                &[],
                Some(&content),
            )
            .await?;
        let event_id = value
            .get("event_id")
            .and_then(JsonValue::as_str)
            .unwrap_or("$unknown:example.org");
        Ok(SendEventResponse {
            event_id: OwnedEventId::try_from(event_id)
                .map_err(|e| ClientError::Other(e.to_string()))?,
        })
    }

    async fn encrypt_and_send_to_device(
        &self,
        event_type: &str,
        targets: &[ToDeviceTarget],
        content: JsonValue,
    ) -> Result<(), ClientError> {
        // DEVELOPMENT ONLY: sends the keys UNENCRYPTED. A real client must
        // encrypt with Olm via its crypto stack.
        let mut messages = serde_json::Map::new();
        for target in targets {
            messages
                .entry(target.user_id.clone())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .expect("object")
                .insert(target.device_id.clone(), content.clone());
        }
        self.request(
            reqwest::Method::PUT,
            self.url(&format!(
                "/_matrix/client/v3/sendToDevice/{event_type}/{}",
                Self::txn_id()
            )),
            &[],
            Some(&json!({ "messages": messages })),
        )
        .await?;
        Ok(())
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

/// Read the access token from the source given on the command line. It is
/// never taken from argv itself.
fn access_token_from(source: &AccessTokenSource) -> Result<Zeroizing<String>, String> {
    match source {
        AccessTokenSource::Stdin => {
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                eprintln!("reading the access token from stdin (end with Ctrl-D)");
            }
            join_call_args::read_access_token(stdin.lock())
        }
        AccessTokenSource::File(path) => std::fs::File::open(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))
            .and_then(join_call_args::read_access_token),
    }
}

#[tokio::main]
async fn main() {
    let args = join_call_args::parse_args(std::env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("join_call: {error}\n{}", join_call_args::USAGE);
        exit(2);
    });
    let access_token = access_token_from(&args.access_token_source).unwrap_or_else(|error| {
        eprintln!("join_call: {error}");
        exit(2);
    });
    let Args {
        homeserver,
        user_id,
        device_id,
        room_id,
        options,
        ..
    } = &args;

    // The client takes ownership of the token; it is zeroized on drop.
    let client = Arc::new(HttpClientApi {
        http: reqwest::Client::new(),
        homeserver: homeserver.clone(),
        access_token,
        user_id: user_id.clone(),
    });

    let own_identity = CallMembershipIdentity {
        user_id: user_id.clone(),
        device_id: device_id.clone(),
        member_id: format!("{user_id}:{device_id}"),
    };

    let session = Arc::new(
        RtcCallSession::new(
            Arc::clone(&client) as Arc<dyn RtcClientApi>,
            RtcRoom {
                room_id: room_id.as_str().try_into().expect("valid room id"),
                version: "default".to_owned(),
            },
            own_identity,
            RtcCallSessionConfig::default(),
        )
        .expect("valid own identity"),
    );
    let mut events = session.subscribe();

    // Initial call member state.
    let member_events = client
        .get_call_member_events(room_id)
        .await
        .expect("fetch room state");
    session.on_room_state_update(&member_events, |_| true, now_ms());

    println!(
        "call members before join: {:?}",
        session
            .memberships()
            .iter()
            .map(|m| format!("{}:{}", m.user_id(), m.device_id()))
            .collect::<Vec<_>>()
    );

    // Continuous sync: feed to-device events (encryption keys) and the
    // room's call member state into the session.
    {
        let client = Arc::clone(&client);
        let session = Arc::clone(&session);
        let room_id = room_id.clone();
        tokio::spawn(async move {
            let mut since: Option<String> = None;
            loop {
                match client.sync_once(since.as_deref()).await {
                    Ok((next_batch, to_device)) => {
                        since = Some(next_batch);
                        for event in to_device {
                            if let Err(error) = session.on_to_device_event(&event) {
                                eprintln!("malformed to-device event: {error}");
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("sync error: {error}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
                match client.get_call_member_events(&room_id).await {
                    Ok(member_events) => {
                        session.on_room_state_update(&member_events, |_| true, now_ms());
                    }
                    Err(error) => eprintln!("state fetch error: {error}"),
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        });
    }

    // Resolve the focus: the --focus argument is our preferred focus (the
    // pre-warmed one), but if anyone is already in the call we follow the
    // oldest membership's focus instead, so that everyone ends up on the
    // same SFU. We prove SFU access BEFORE writing membership so that a user
    // who cannot reach the SFU never appears in the call (#9).
    let foci_preferred = options
        .focus
        .iter()
        .map(|service_url| {
            Transport::from_livekit(&ruma::events::call::member::LivekitFocus::new(
                room_id.clone(),
                service_url.clone(),
            ))
        })
        .collect::<Vec<_>>();
    let service_url = session
        .livekit_focus_to_join(&foci_preferred)
        .map(|focus| focus.service_url)
        .unwrap_or_else(|| {
            eprintln!("no --focus given and nobody in the call; nothing to connect to");
            exit(1);
        });
    println!("using LiveKit JWT service: {service_url}");

    // Fetch the SFU JWT with our OpenID token (MSC4195) and connect
    // to LiveKit BEFORE joining — proves SFU access before membership
    // is advertised.
    let openid_token = client.get_openid_token().await.expect("openid token");
    let sfu_config = fetch_sfu_config(
        &client.http,
        &service_url,
        room_id,
        device_id,
        &openid_token,
    )
    .await
    .expect("SFU config");
    println!("got SFU config: url={}", sfu_config.url);

    let (mut connection, mut room_events) = LivekitCallConnection::connect(&sfu_config)
        .await
        .expect("LiveKit connect");
    println!("connected as {}", connection.local_identity());

    // Now that SFU access is proven, advertise our membership.
    session.join_rtc_session(foci_preferred);

    // Publish a microphone track and feed it silence, so that remote
    // participants actually receive (E2EE) audio frames from us.
    let audio_source = livekit::webrtc::audio_source::native::NativeAudioSource::new(
        livekit::webrtc::audio_source::AudioSourceOptions::default(),
        48_000,
        1,
        100,
    );
    connection
        .publish_microphone_track(livekit::webrtc::audio_source::RtcAudioSource::Native(
            audio_source.clone(),
        ))
        .await
        .expect("publish microphone");
    tokio::spawn(async move {
        let frame = livekit::webrtc::audio_frame::AudioFrame {
            data: vec![0i16; 480].into(),
            sample_rate: 48_000,
            num_channels: 1,
            samples_per_channel: 480,
        };
        loop {
            // `capture_frame` paces us: it waits until the source queue has
            // room for the 10 ms frame.
            if audio_source.capture_frame(&frame).await.is_err() {
                break;
            }
        }
    });

    // Decrypted audio frames received, by remote participant identity.
    let frames_received: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));

    // Assertion state for --assert-peer.
    let mut peer_membership_seen = false;
    let mut peer_track_subscribed = false;
    let mut peer_key_received = false;
    let mut peer_e2ee_ok = false;
    let mut peer_frames_reported = false;
    let mut assertions_passed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(options.assert_timeout);
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    // Reconnect backoff: starts at 200 ms, capped at 30 s.
    let mut reconnect_backoff = Duration::from_millis(200);

    // Wire the E2EE keys of the session into the frame cryptor, and print
    // membership/track updates. Reconnect to the SFU on disconnect while
    // the Matrix-side membership is still live (#8).
    'outer: loop {
        tokio::select! {
            Some(event) = events.recv() => match event {
                RtcCallSessionEvent::EncryptionKeyChanged {
                    key,
                    key_index,
                    membership,
                    rtc_backend_identity,
                } => {
                    connection.set_participant_key(&rtc_backend_identity, key_index, key);
                    println!("set key {key_index} for {rtc_backend_identity}");
                    if Some(&membership.member_id) == options.assert_peer.as_ref() {
                        peer_key_received = true;
                    }
                }
                RtcCallSessionEvent::MembershipsChanged { ref new, .. } => {
                    let members: Vec<String> = new
                        .iter()
                        .map(|m| format!("{}:{}", m.user_id(), m.device_id()))
                        .collect();
                    println!("memberships changed: {members:?}");
                    if let Some(peer) = &options.assert_peer
                        && members.iter().any(|m| m == peer)
                    {
                        peer_membership_seen = true;
                    }
                }
                RtcCallSessionEvent::JoinStateChanged(false) => {
                    println!("we left the session");
                    break 'outer;
                }
                other => println!("session event: {other:?}"),
            },
            Some(event) = room_events.recv() => match event {
                livekit::RoomEvent::TrackSubscribed { track, participant, .. } => {
                    println!(
                        "subscribed to track {} of {}",
                        track.sid(),
                        participant.identity()
                    );
                    let identity = participant.identity().to_string();
                    if Some(&identity) == options.assert_peer.as_ref() {
                        peer_track_subscribed = true;
                    }
                    if let livekit::track::RemoteTrack::Audio(audio) = track {
                        let frames_received = Arc::clone(&frames_received);
                        tokio::spawn(async move {
                            let mut stream =
                                livekit::webrtc::audio_stream::native::NativeAudioStream::new(
                                    audio.rtc_track(),
                                    48_000,
                                    1,
                                );
                            while stream.next().await.is_some() {
                                *frames_received
                                    .lock()
                                    .unwrap()
                                    .entry(identity.clone())
                                    .or_insert(0) += 1;
                            }
                        });
                    }
                }
                livekit::RoomEvent::E2eeStateChanged { participant, state } => {
                    let identity = participant.identity().to_string();
                    println!("e2ee state of {identity}: {state:?}");
                    if Some(&identity) == options.assert_peer.as_ref()
                        && matches!(
                            state,
                            livekit::webrtc::native::frame_cryptor::EncryptionState::Ok
                        )
                    {
                        peer_e2ee_ok = true;
                    }
                }
                livekit::RoomEvent::Disconnected { reason } => {
                    println!("disconnected: {reason:?}");
                    if !session.is_joined() {
                        break 'outer;
                    }
                    println!(
                        "still joined; reconnecting in {} ms",
                        reconnect_backoff.as_millis()
                    );
                    tokio::time::sleep(reconnect_backoff).await;
                    reconnect_backoff =
                        (reconnect_backoff * 2).min(Duration::from_secs(30));
                    // Re-fetch SFU config (token may have changed) and
                    // reconnect.
                    match client.get_openid_token().await {
                        Ok(openid_token) => {
                            match fetch_sfu_config(
                                &client.http,
                                &service_url,
                                room_id,
                                device_id,
                                &openid_token,
                            )
                            .await
                            {
                                Ok(new_sfu_config) => {
                                    match LivekitCallConnection::connect(
                                        &new_sfu_config,
                                    )
                                    .await
                                    {
                                        Ok((new_connection, new_events)) => {
                                            println!(
                                                "reconnected as {}",
                                                new_connection.local_identity()
                                            );
                                            // Re-apply encryption keys to
                                            // the new connection.
                                            if let Some(key_rings) =
                                                session.get_encryption_keys()
                                            {
                                                for ring in key_rings.values()
                                                {
                                                    new_connection
                                                        .apply_key_ring(
                                                            ring.iter(),
                                                        );
                                                }
                                            }
                                            let _old = std::mem::replace(
                                                &mut connection,
                                                new_connection,
                                            );
                                            drop(_old);
                                            room_events = new_events;
                                            reconnect_backoff =
                                                Duration::from_millis(200);
                                        }
                                        Err(error) => {
                                            eprintln!(
                                                "reconnect failed: {error}"
                                            );
                                        }
                                    }
                                }
                                Err(error) => {
                                    eprintln!(
                                        "SFU config refetch failed: {error}"
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("openid token refresh failed: {error}");
                        }
                    }
                }
                _ => {}
            },
            _ = ticker.tick() => {
                let Some(peer) = &options.assert_peer else { continue };

                let peer_frames = frames_received
                    .lock()
                    .unwrap()
                    .get(peer)
                    .copied()
                    .unwrap_or(0);
                if peer_frames >= 25 && !peer_frames_reported {
                    peer_frames_reported = true;
                    println!("received {peer_frames} decrypted audio frames from {peer}");
                }

                if !assertions_passed
                    && peer_membership_seen
                    && peer_track_subscribed
                    && peer_key_received
                    && peer_e2ee_ok
                    && peer_frames_reported
                {
                    assertions_passed = true;
                    println!("ASSERTIONS PASSED");
                }

                if !assertions_passed && tokio::time::Instant::now() >= deadline {
                    println!(
                        "ASSERTIONS FAILED: membership_seen={peer_membership_seen} \
                         track_subscribed={peer_track_subscribed} key_received={peer_key_received} \
                         e2ee_ok={peer_e2ee_ok} frames_received={peer_frames}"
                    );
                    exit(1);
                }
            },
            _ = tokio::signal::ctrl_c() => {
                break 'outer;
            }
        }
    }

    if session.is_joined() {
        session
            .leave_rtc_session(Some(Duration::from_secs(10)))
            .await;
    }
    let _ = connection.disconnect().await;
    println!("left the session gracefully");
}
