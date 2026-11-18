//! Local screen capture for native calls: Share Screen (INTEGRATION-PLAN A5,
//! ADR 0016).
//!
//! Only compiled with the `calls-media` feature. What is shared is chosen by
//! the person in the desktop's own picker, through the XDG ScreenCast portal:
//! a monitor or a window, such as the "<deck> — Slide Show" window Decks
//! presents in. The portal gives a `PipeWire` stream, read with `GStreamer`'s
//! `pipewiresrc` and converted to the same I420 frames as the camera, which
//! go to the media task through a [`CameraSink`] of their own and are
//! published as a screen share track.
//!
//! Dropping the [`ScreenCapture`] stops the pipeline and closes the portal
//! session, which ends the share in the desktop's indicator too.

use std::{
    os::fd::{AsRawFd, OwnedFd},
    str::FromStr,
    sync::Arc,
};

use ashpd::desktop::{
    PersistMode, Session,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use gst::prelude::*;
use gtk::glib;
use tracing::{debug, error, warn};

use super::camera::{CameraSink, frame_from_sample, make_element};
use crate::spawn_tokio;

/// The caps shared frames are converted to: wider than the camera's, so text
/// on a slide stays legible, and at most 15 frames a second, which a slide or
/// a document does not need more of.
const SCREEN_CAPS: &str =
    "video/x-raw,format=I420,width=(int)[1,1920],framerate=(fraction)[0/1,15/1]";

/// An ongoing share of a screen or window.
///
/// The share ends when this is dropped.
pub(super) struct ScreenCapture {
    /// The capture pipeline.
    pipeline: gst::Pipeline,
    /// The guard of the watch on the bus of the pipeline.
    _bus_guard: gst::bus::BusWatchGuard,
    /// The `PipeWire` remote the portal opened, read by `pipewiresrc`.
    _remote: OwnedFd,
    /// The portal session, closed when the share ends.
    session: Option<Session<Screencast>>,
}

impl std::fmt::Debug for ScreenCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenCapture").finish_non_exhaustive()
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        if let Err(error) = self.pipeline.set_state(gst::State::Null) {
            warn!("Could not stop the screen share pipeline: {error}");
        }
        if let Some(session) = self.session.take() {
            spawn_tokio!(async move {
                if let Err(error) = session.close().await {
                    debug!("Could not close the screen cast session: {error}");
                }
            });
        }
    }
}

impl ScreenCapture {
    /// Ask the person what to share, through the portal's picker, and start
    /// capturing it, sending the frames to the given sink. `on_ended` runs on
    /// the main thread when the stream ends on its own, for example when
    /// the share is stopped from the desktop's indicator.
    ///
    /// Must be called from the main thread.
    pub(super) async fn new(
        sink: Arc<CameraSink>,
        on_ended: impl Fn() + 'static,
    ) -> Result<Self, String> {
        let handle = spawn_tokio!(open_portal());
        let (remote, node_id, session) = handle.await.expect("task was not aborted")?;
        debug!("Sharing the screen cast stream {node_id}");

        let source = gst::ElementFactory::make("pipewiresrc")
            .property("fd", remote.as_raw_fd())
            .property("path", node_id.to_string())
            .build()
            .map_err(|error| format!("failed to create the screen source: {error}"))?;

        let (pipeline, bus_guard) = build_pipeline(&source, sink, on_ended)?;

        Ok(Self {
            pipeline,
            _bus_guard: bus_guard,
            _remote: remote,
            session: Some(session),
        })
    }
}

/// Ask the portal for one monitor or window, start the cast and open its
/// `PipeWire` remote. Runs on the tokio runtime.
async fn open_portal() -> Result<(OwnedFd, u32, Session<Screencast>), String> {
    let portal_error = |error: ashpd::Error| format!("could not share the screen: {error}");

    let proxy = Screencast::new().await.map_err(portal_error)?;
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(portal_error)?;
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::EMBEDDED)
                .set_sources(SourceType::MONITOR | SourceType::WINDOW)
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(portal_error)?;
    let streams = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(portal_error)?
        .response()
        .map_err(portal_error)?;
    let Some(node_id) = streams
        .streams()
        .first()
        .map(ashpd::desktop::screencast::Stream::pipe_wire_node_id)
    else {
        return Err("nothing was chosen to share".to_owned());
    };
    let remote = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(portal_error)?;

    Ok((remote, node_id, session))
}

/// Build and start the pipeline from the `PipeWire` source to the sink.
fn build_pipeline(
    source: &gst::Element,
    sink: Arc<CameraSink>,
    on_ended: impl Fn() + 'static,
) -> Result<(gst::Pipeline, gst::bus::BusWatchGuard), String> {
    let caps = gst::Caps::from_str(SCREEN_CAPS).expect("screen caps should be valid");

    let convert = make_element("videoconvert")?;
    let scale = make_element("videoscale")?;
    let rate = make_element("videorate")?;
    let capsfilter = gst::ElementFactory::make("capsfilter")
        .property("caps", &caps)
        .build()
        .map_err(|error| format!("failed to create the screen capsfilter: {error}"))?;
    // Frames are dropped rather than queued when the encoder cannot keep up.
    let queue = gst::ElementFactory::make("queue")
        .property("max-size-buffers", 2u32)
        .property("max-size-bytes", 0u32)
        .property("max-size-time", 0u64)
        .property_from_str("leaky", "downstream")
        .build()
        .map_err(|error| format!("failed to create the screen queue: {error}"))?;
    let appsink = gst_app::AppSink::builder()
        .caps(&caps)
        .sync(false)
        .max_buffers(2)
        .drop(true)
        .build();

    let pipeline = gst::Pipeline::new();
    let elements = [
        source,
        &convert,
        &scale,
        &rate,
        &capsfilter,
        &queue,
        appsink.upcast_ref(),
    ];
    pipeline
        .add_many(elements)
        .map_err(|error| format!("failed to build the screen pipeline: {error}"))?;
    gst::Element::link_many(elements)
        .map_err(|error| format!("failed to link the screen pipeline: {error}"))?;

    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                let Ok(sample) = appsink.pull_sample() else {
                    return Err(gst::FlowError::Eos);
                };

                if let Some(frame) = frame_from_sample(&sample) {
                    sink.send_frame(frame);
                }

                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    let bus_guard = pipeline
        .bus()
        .expect("pipeline should have a bus")
        .add_watch_local(move |_, message| {
            match message.view() {
                gst::MessageView::Eos(_) => {
                    debug!("The screen cast stream ended");
                    on_ended();
                }
                gst::MessageView::Error(error) => {
                    error!(
                        "Error in the screen share pipeline: {} ({:?})",
                        error.error(),
                        error.debug()
                    );
                    on_ended();
                }
                _ => {}
            }
            glib::ControlFlow::Continue
        })
        .map_err(|_| "failed to watch the screen share pipeline".to_owned())?;

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|error| format!("failed to start the screen share pipeline: {error}"))?;

    Ok((pipeline, bus_guard))
}
