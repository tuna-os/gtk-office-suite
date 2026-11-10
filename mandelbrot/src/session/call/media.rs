//! Media layer for native calls, bridging the `MatrixRTC` engine to the
//! `LiveKit` SFU.
//!
//! Only compiled with the `calls-media` feature. The media task runs on the
//! tokio runtime: it fetches the SFU JWT with our `OpenID` token, connects to
//! the `LiveKit` room with end-to-end encryption, publishes the microphone
//! (captured with `GStreamer`) and forwards remote video frames to the UI.

use std::{
    ops::ControlFlow,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gst::prelude::*;
use mandelbrot_matrixrtc::{
    RtcCallSession, RtcCallSessionEvent, Transport, livekit,
    livekit_connection::{LivekitCallConnection, OpenIdToken, SfuConfig, fetch_sfu_config},
    reqwest,
};
use matrix_sdk::Client;
use ruma::{OwnedRoomId, api::client::account::request_openid_token};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, error, warn};

use super::camera::{CameraFrame, CameraMessage, CameraSink};
use crate::spawn_tokio;

/// The number of camera frames that can be queued for the media task.
const CAMERA_QUEUE_LEN: usize = 3;

/// The sample rate of the published microphone track.
const MIC_SAMPLE_RATE: u32 = 48_000;
/// The number of channels of the published microphone track.
const MIC_CHANNELS: u32 = 1;

/// An event from the media task, consumed on the main context.
pub(super) enum MediaEvent {
    /// The connection to the SFU is established.
    Connected,
    /// A video frame of a remote participant arrived.
    VideoFrame {
        /// The `LiveKit` identity of the participant.
        identity: String,
        /// The frame, as RGBA bytes.
        rgba: Vec<u8>,
        /// The width of the frame.
        width: u32,
        /// The height of the frame.
        height: u32,
    },
    /// The video track of a remote participant went away.
    VideoEnded {
        /// The `LiveKit` identity of the participant.
        identity: String,
    },
    /// The media connection failed or ended.
    Ended {
        /// A human-readable reason, if the connection failed.
        error: Option<String>,
    },
}

/// The local media sources of a call, controlled by the UI.
struct LocalSources {
    /// Whether the microphone is muted.
    muted: Arc<AtomicBool>,
    /// The camera frames captured by the UI, if the camera is on.
    camera: mpsc::Receiver<CameraMessage>,
}

/// A handle to the media connection of one call.
pub(super) struct MediaHandle {
    task: JoinHandle<()>,
    /// Whether the microphone is muted.
    muted: Arc<AtomicBool>,
}

impl MediaHandle {
    /// Set whether the microphone is muted.
    pub(super) fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::SeqCst);
    }

    /// The flag controlling whether the microphone is muted.
    pub(super) fn muted_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.muted)
    }
}

impl Drop for MediaHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Start the media connection for the given call.
///
/// Must be called from within the tokio runtime context. Events for the UI
/// are delivered on the returned channel; the connection is torn down when
/// the handle is dropped.
pub(super) fn start(
    client: Client,
    room_id: OwnedRoomId,
    device_id: String,
    engine: Arc<RtcCallSession>,
    preferred_foci: Vec<Transport>,
    camera_sink: &CameraSink,
) -> (MediaHandle, mpsc::Receiver<MediaEvent>) {
    let (tx, rx) = mpsc::channel(8);
    let muted = Arc::new(AtomicBool::new(false));

    // The camera capture outlives the media connection, so the sink keeps a
    // sender that we replace here; the task keeps its own clone alive so that
    // the channel never closes while it runs.
    let (camera_tx, camera_rx) = mpsc::channel(CAMERA_QUEUE_LEN);
    camera_sink.set_sender(Some(camera_tx.clone()));

    let task_muted = Arc::clone(&muted);
    // Must go through the shared runtime: this is called from a GTK signal
    // handler on the main thread, where `tokio::spawn` has no reactor and
    // aborts the process.
    let task = spawn_tokio!(async move {
        // Keep the camera channel open for as long as the task runs.
        let _camera_tx = camera_tx;

        let error = match run(
            client,
            room_id,
            device_id,
            engine,
            preferred_foci,
            &tx,
            LocalSources {
                muted: task_muted,
                camera: camera_rx,
            },
        )
        .await
        {
            Ok(()) => None,
            Err(error) => {
                error!("Call media connection failed: {error}");
                Some(error)
            }
        };
        let _ = tx.send(MediaEvent::Ended { error }).await;
    });

    (MediaHandle { task, muted }, rx)
}

/// Fetch a Matrix `OpenID` token for our own user.
async fn get_openid_token(client: &Client) -> Result<OpenIdToken, String> {
    let user_id = client
        .user_id()
        .ok_or_else(|| "not logged in".to_owned())?
        .to_owned();

    let response = client
        .send(request_openid_token::v3::Request::new(user_id))
        .await
        .map_err(|error| format!("failed to fetch OpenID token: {error}"))?;

    Ok(OpenIdToken {
        access_token: response.access_token,
        token_type: response.token_type.to_string(),
        matrix_server_name: response.matrix_server_name.to_string(),
        expires_in: response.expires_in.as_secs(),
    })
}

/// The `LiveKit` service URL to use for the call.
fn service_url(engine: &RtcCallSession, preferred_foci: &[Transport]) -> Option<String> {
    engine
        .get_active_focus()
        .as_ref()
        .and_then(Transport::as_livekit)
        .map(|focus| focus.service_url)
        .or_else(|| {
            preferred_foci
                .iter()
                .find_map(|focus| focus.as_livekit().map(|focus| focus.service_url))
        })
}

/// Run the media connection until the task is aborted or the user leaves.
///
/// On SFU disconnect the connection is retried with exponential backoff
/// while the Matrix-side membership is still live (#8). Membership is only
/// advertised after the first successful SFU connection (#9).
async fn run(
    client: Client,
    room_id: OwnedRoomId,
    device_id: String,
    engine: Arc<RtcCallSession>,
    preferred_foci: Vec<Transport>,
    tx: &mpsc::Sender<MediaEvent>,
    local: LocalSources,
) -> Result<(), String> {
    let LocalSources {
        muted,
        camera: mut camera_rx,
    } = local;

    // Subscribe before connecting so that no key event is lost.
    let mut engine_events = engine.subscribe();

    // Resolve the focus: preferred foci first (pre-warming from
    // .well-known), falling back to the active focus of the session
    // once we are in it.
    let service_url = preferred_foci
        .iter()
        .find_map(|focus| focus.as_livekit().map(|lk| lk.service_url))
        .or_else(|| service_url(&engine, &preferred_foci))
        .ok_or_else(|| "no LiveKit focus available for this call".to_owned())?;

    let http = mandelbrot_matrixrtc::reqwest::Client::new();

    // Connect to the SFU BEFORE advertising membership, so that a user who
    // cannot reach the SFU never appears in the call (#9).
    let (mut connection, mut room_events) =
        connect_to_sfu(&client, &http, &service_url, &room_id, &device_id).await?;

    // SFU access proven — now advertise our membership.
    if !engine.is_joined() {
        engine.join_rtc_session(preferred_foci.clone());
    }

    // Keys that arrived before the connection was established.
    apply_known_keys(&engine, &connection);

    let _ = tx.send(MediaEvent::Connected).await;

    // Publish the microphone. A call without a microphone is still useful to
    // listen in.
    let _mic_guard = publish_microphone(&connection, muted)
        .await
        .inspect_err(|error| warn!("Failed to publish the microphone: {error}"))
        .ok();

    let mut video_tasks: Vec<JoinHandle<()>> = Vec::new();
    // The published camera track, if the camera is on. It is only published
    // once frames actually arrive, so that the resolution is known and the
    // other participants never see an empty track.
    let mut camera: Option<CameraTrack> = None;

    // Reconnect backoff: starts at 200 ms, capped at 30 s.
    let mut reconnect_backoff = Duration::from_millis(200);

    // Outer loop: reconnects on SFU disconnect while the user is still
    // joined to the call (#8).
    let result = 'outer: loop {
        // Inner event loop: runs while the SFU connection is alive.
        loop {
            tokio::select! {
                message = camera_rx.recv() => {
                    let Some(message) = message else {
                        break 'outer Ok(());
                    };
                    handle_camera_message(&connection, &mut camera, message).await;
                }
                event = engine_events.recv() => {
                    let Some(event) = event else {
                        break 'outer Ok(());
                    };
                    match event {
                        RtcCallSessionEvent::EncryptionKeyChanged {
                            key,
                            key_index,
                            rtc_backend_identity,
                            ..
                        } => {
                            connection.set_participant_key(
                                &rtc_backend_identity, key_index, key,
                            );
                        }
                        RtcCallSessionEvent::JoinStateChanged(false) => {
                            debug!("User left the call; stopping media");
                            break 'outer Ok(());
                        }
                        _ => {}
                    }
                }
                event = room_events.recv() => {
                    let Some(event) = event else {
                        // The LiveKit room event stream ended.
                        break;
                    };
                    if handle_room_event(event, tx, &mut video_tasks).await.is_break() {
                        // Disconnected from the SFU.
                        break;
                    }
                }
            }
        }

        // Inner loop ended — SFU disconnected. Check if we should
        // reconnect.
        if !engine.is_joined() {
            debug!("No longer joined; not reconnecting");
            break 'outer Ok(());
        }

        warn!(
            "SFU disconnected; reconnecting in {} ms",
            reconnect_backoff.as_millis()
        );
        tokio::time::sleep(reconnect_backoff).await;
        reconnect_backoff = (reconnect_backoff * 2).min(Duration::from_secs(30));

        // Re-fetch the SFU config (token may have changed) and reconnect.
        match connect_to_sfu(&client, &http, &service_url, &room_id, &device_id).await {
            Ok((new_connection, new_events)) => {
                debug!(
                    "Reconnected to LiveKit as {}",
                    new_connection.local_identity()
                );
                // Keys may have arrived while disconnected.
                apply_known_keys(&engine, &new_connection);
                // Replacing the old connection drops it.
                connection = new_connection;
                room_events = new_events;
                reconnect_backoff = Duration::from_millis(200);
            }
            Err(error) => {
                warn!("SFU reconnect failed: {error}");
                // Loop around and retry with growing backoff.
            }
        }
    };

    for task in video_tasks {
        task.abort();
    }
    let _ = connection.disconnect().await;
    result
}

/// Apply every encryption key the session has received so far.
fn apply_known_keys(engine: &RtcCallSession, connection: &LivekitCallConnection) {
    if let Some(key_rings) = engine.get_encryption_keys() {
        for ring in key_rings.values() {
            connection.apply_key_ring(ring.iter());
        }
    }
}

/// Fetch an `OpenID` token and SFU config, then connect to the `LiveKit` SFU.
async fn connect_to_sfu(
    client: &Client,
    http: &reqwest::Client,
    service_url: &str,
    room_id: &OwnedRoomId,
    device_id: &str,
) -> Result<
    (
        LivekitCallConnection,
        mpsc::UnboundedReceiver<livekit::RoomEvent>,
    ),
    String,
> {
    let openid_token = get_openid_token(client).await?;
    let sfu_config: SfuConfig = fetch_sfu_config(
        http,
        service_url,
        room_id.as_str(),
        device_id,
        &openid_token,
    )
    .await
    .map_err(|error| format!("failed to fetch the SFU configuration: {error}"))?;

    Box::pin(LivekitCallConnection::connect(&sfu_config))
        .await
        .map_err(|error| format!("failed to connect to the SFU: {error}"))
}

/// The published camera track and the source feeding it.
struct CameraTrack {
    /// The source frames are captured into.
    source: livekit::webrtc::video_source::native::NativeVideoSource,
    /// The SID of the published track, to unpublish it.
    sid: livekit::id::TrackSid,
}

impl CameraTrack {
    /// Capture the given frame into the track.
    fn capture(&self, frame: &CameraFrame) {
        use livekit::webrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};

        let mut buffer = I420Buffer::new(frame.width, frame.height);
        let (stride_y, stride_u, stride_v) = buffer.strides();
        let (data_y, data_u, data_v) = buffer.data_mut();

        let chroma_width = frame.width.div_ceil(2) as usize;
        let chroma_height = frame.height.div_ceil(2) as usize;
        let luma_len = frame.width as usize * frame.height as usize;
        let chroma_len = chroma_width * chroma_height;

        let planes = [
            (data_y, stride_y as usize, frame.width as usize, 0, luma_len),
            (
                data_u,
                stride_u as usize,
                chroma_width,
                luma_len,
                luma_len + chroma_len,
            ),
            (
                data_v,
                stride_v as usize,
                chroma_width,
                luma_len + chroma_len,
                luma_len + chroma_len * 2,
            ),
        ];

        for (destination, stride, row_len, start, end) in planes {
            let Some(source) = frame.data.get(start..end) else {
                warn!("Discarding a truncated camera frame");
                return;
            };

            for (row, chunk) in source.chunks_exact(row_len).enumerate() {
                let offset = row * stride;
                let Some(destination) = destination.get_mut(offset..offset + row_len) else {
                    break;
                };
                destination.copy_from_slice(chunk);
            }
        }

        self.source.capture_frame(&VideoFrame {
            rotation: VideoRotation::VideoRotation0,
            timestamp_us: 0,
            frame_metadata: None,
            buffer,
        });
    }
}

/// Apply an event of the `LiveKit` room, and tell whether the connection is
/// over.
async fn handle_room_event(
    event: livekit::RoomEvent,
    tx: &mpsc::Sender<MediaEvent>,
    video_tasks: &mut Vec<JoinHandle<()>>,
) -> ControlFlow<()> {
    match event {
        livekit::RoomEvent::TrackSubscribed {
            track: livekit::track::RemoteTrack::Video(video),
            participant,
            ..
        } => {
            video_tasks.push(spawn_video_task(
                &video,
                participant.identity().to_string(),
                tx.clone(),
            ));
        }
        livekit::RoomEvent::TrackUnsubscribed {
            track: livekit::track::RemoteTrack::Video(_),
            participant,
            ..
        } => {
            let _ = tx
                .send(MediaEvent::VideoEnded {
                    identity: participant.identity().to_string(),
                })
                .await;
        }
        livekit::RoomEvent::Disconnected { reason } => {
            debug!("Disconnected from LiveKit: {reason:?}");
            return ControlFlow::Break(());
        }
        _ => {}
    }

    ControlFlow::Continue(())
}

/// Apply a message from the local camera capture to the connection.
///
/// The track is only published once frames actually arrive, so that its
/// resolution is known and the other participants never see an empty track.
async fn handle_camera_message(
    connection: &LivekitCallConnection,
    camera: &mut Option<CameraTrack>,
    message: CameraMessage,
) {
    match message {
        CameraMessage::Frame(frame) => {
            if camera.is_none() {
                match publish_camera(connection, &frame).await {
                    Ok(track) => *camera = Some(track),
                    Err(error) => warn!("Failed to publish the camera: {error}"),
                }
            }

            if let Some(camera) = camera.as_ref() {
                camera.capture(&frame);
            }
        }
        CameraMessage::Stopped => {
            if let Some(camera) = camera.take()
                && let Err(error) = connection.unpublish_track(&camera.sid).await
            {
                warn!("Failed to unpublish the camera: {error}");
            }
        }
    }
}

/// Publish a camera track with the resolution of the given first frame.
async fn publish_camera(
    connection: &LivekitCallConnection,
    frame: &CameraFrame,
) -> Result<CameraTrack, String> {
    use livekit::webrtc::video_source::{
        RtcVideoSource, VideoResolution, native::NativeVideoSource,
    };

    let source = NativeVideoSource::new(
        VideoResolution {
            width: frame.width,
            height: frame.height,
        },
        false,
    );

    let sid = connection
        .publish_camera_track(RtcVideoSource::Native(source.clone()))
        .await
        .map_err(|error| format!("failed to publish the camera track: {error}"))?;
    debug!("Published the camera track {sid}");

    Ok(CameraTrack { source, sid })
}

/// A guard stopping the microphone pipeline on drop.
struct MicrophoneGuard {
    pipeline: gst::Pipeline,
    pump: JoinHandle<()>,
}

impl Drop for MicrophoneGuard {
    fn drop(&mut self) {
        self.pump.abort();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// Capture the default microphone with `GStreamer` and publish it as an audio
/// track.
async fn publish_microphone(
    connection: &LivekitCallConnection,
    muted: Arc<AtomicBool>,
) -> Result<MicrophoneGuard, String> {
    use livekit::webrtc::{
        audio_frame::AudioFrame,
        audio_source::{RtcAudioSource, native::NativeAudioSource},
    };

    let source = NativeAudioSource::new(
        livekit::webrtc::audio_source::AudioSourceOptions {
            echo_cancellation: true,
            noise_suppression: true,
            auto_gain_control: true,
        },
        MIC_SAMPLE_RATE,
        MIC_CHANNELS,
        1000,
    );

    let pipeline = gst::parse::launch(&format!(
        "autoaudiosrc ! audioconvert ! audioresample ! \
         audio/x-raw,format=S16LE,rate={MIC_SAMPLE_RATE},channels={MIC_CHANNELS} ! \
         appsink name=sink emit-signals=false sync=false"
    ))
    .map_err(|error| format!("failed to create the microphone pipeline: {error}"))?
    .downcast::<gst::Pipeline>()
    .map_err(|_| "microphone pipeline is not a pipeline".to_owned())?;

    let appsink = pipeline
        .by_name("sink")
        .and_then(|sink| sink.downcast::<gst_app::AppSink>().ok())
        .ok_or_else(|| "no appsink in the microphone pipeline".to_owned())?;

    let (samples_tx, mut samples_rx) = mpsc::channel::<Vec<i16>>(16);
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                let Ok(sample) = appsink.pull_sample() else {
                    return Err(gst::FlowError::Eos);
                };
                if muted.load(Ordering::SeqCst) {
                    return Ok(gst::FlowSuccess::Ok);
                }
                let Some(buffer) = sample.buffer() else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let Ok(map) = buffer.map_readable() else {
                    return Ok(gst::FlowSuccess::Ok);
                };
                let bytes = map.as_slice();
                let (chunks, _) = bytes.as_chunks::<2>();
                let samples: Vec<i16> = chunks
                    .iter()
                    .map(|chunk| i16::from_le_bytes(*chunk))
                    .collect();
                // Drop frames if the pump cannot keep up.
                let _ = samples_tx.try_send(samples);
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|error| format!("failed to start the microphone pipeline: {error}"))?;

    connection
        .publish_microphone_track(RtcAudioSource::Native(source.clone()))
        .await
        .map_err(|error| format!("failed to publish the microphone track: {error}"))?;

    let pump = tokio::spawn(async move {
        while let Some(samples) = samples_rx.recv().await {
            let samples_per_channel =
                u32::try_from(samples.len()).unwrap_or(u32::MAX) / MIC_CHANNELS;
            let frame = AudioFrame {
                data: samples.into(),
                sample_rate: MIC_SAMPLE_RATE,
                num_channels: MIC_CHANNELS,
                samples_per_channel,
            };
            if let Err(error) = source.capture_frame(&frame).await {
                warn!("Failed to capture a microphone frame: {error}");
            }
        }
    });

    Ok(MicrophoneGuard { pipeline, pump })
}

/// Spawn a task converting the frames of a remote video track to RGBA and
/// forwarding them to the UI.
fn spawn_video_task(
    video: &livekit::track::RemoteVideoTrack,
    identity: String,
    tx: mpsc::Sender<MediaEvent>,
) -> JoinHandle<()> {
    use futures_util::StreamExt;
    use livekit::webrtc::{
        native::yuv_helper, video_frame::VideoBuffer, video_stream::native::NativeVideoStream,
    };

    let mut stream = NativeVideoStream::new(video.rtc_track());
    tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            let buffer = frame.buffer.to_i420();
            let width = buffer.width();
            let height = buffer.height();
            let (stride_y, stride_u, stride_v) = buffer.strides();
            let (data_y, data_u, data_v) = buffer.data();

            let mut rgba = vec![0u8; (width * height * 4) as usize];
            yuv_helper::i420_to_abgr(
                data_y,
                stride_y,
                data_u,
                stride_u,
                data_v,
                stride_v,
                &mut rgba,
                width * 4,
                i32::try_from(width).unwrap_or(i32::MAX),
                i32::try_from(height).unwrap_or(i32::MAX),
            );

            // Drop frames if the UI cannot keep up.
            let event = MediaEvent::VideoFrame {
                identity: identity.clone(),
                rgba,
                width,
                height,
            };
            if tx.try_send(event).is_err() && tx.is_closed() {
                break;
            }
        }
        let _ = tx.send(MediaEvent::VideoEnded { identity }).await;
    })
}
