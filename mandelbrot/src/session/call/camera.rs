//! Local camera capture for native calls.
//!
//! Only compiled with the `calls-media` feature. The camera is opened through
//! the XDG camera portal (the Flatpak has no raw device access), enumerated
//! with `aperture`'s `PipeWire` device provider — the same provider the QR code
//! scanner uses — and captured with `GStreamer`.
//!
//! The pipeline is built and owned on the main thread, because the `PipeWire`
//! device provider and the `GdkPaintable` of the self-view are main-thread
//! objects. It tees into two branches: a `gtk4paintablesink` for the local
//! self-view, and an `appsink` whose I420 frames are forwarded to the media
//! task through a [`CameraSink`], which publishes them to the SFU.
//!
//! Dropping the [`CameraCapture`] stops the pipeline, which closes the camera
//! device: the camera is only open while the user has it turned on.

use std::{
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use ashpd::desktop::camera;
use gst::prelude::*;
use gst_video::prelude::VideoFrameExt;
use gtk::{gdk, glib};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use crate::spawn_tokio;

/// The caps captured frames are converted to.
///
/// The width and framerate are capped to keep the encoder and the uplink
/// reasonable; `videoscale` passes smaller resolutions through untouched. The
/// height is left free so that the aspect ratio of the camera is preserved
/// instead of being letterboxed into a fixed resolution.
const CAPTURE_CAPS: &str =
    "video/x-raw,format=I420,width=(int)[1,1280],framerate=(fraction)[0/1,30/1]";

/// How long to wait for the device provider to list a camera.
const CAMERA_WAIT: Duration = Duration::from_secs(3);

/// The interval at which the device provider is polled for a camera.
const CAMERA_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A captured video frame in I420, with tightly packed planes.
///
/// The data is the Y plane, followed by the U and V planes, each row of each
/// plane exactly as wide as the plane (no padding).
pub(super) struct CameraFrame {
    /// The width of the frame, in pixels.
    pub(super) width: u32,
    /// The height of the frame, in pixels.
    pub(super) height: u32,
    /// The packed I420 planes.
    pub(super) data: Vec<u8>,
}

/// A message from the camera capture to the media task.
pub(super) enum CameraMessage {
    /// A frame was captured.
    Frame(CameraFrame),
    /// The camera was turned off, the track should be unpublished.
    Stopped,
}

/// The destination of the captured frames.
///
/// The capture and the media connection have independent lifetimes: the
/// camera can be turned on in the prescreen, before the call is joined, and a
/// call can be joined while the camera is off. The sink bridges the two: it
/// holds the sender of the media task, if there is one, and drops frames when
/// there is not.
#[derive(Debug, Default)]
pub(super) struct CameraSink {
    sender: Mutex<Option<mpsc::Sender<CameraMessage>>>,
}

impl CameraSink {
    /// Set the sender of the media task consuming the frames.
    pub(super) fn set_sender(&self, sender: Option<mpsc::Sender<CameraMessage>>) {
        *self.sender.lock().unwrap() = sender;
    }

    /// The sender of the media task consuming the frames, if any.
    fn sender(&self) -> Option<mpsc::Sender<CameraMessage>> {
        self.sender.lock().unwrap().clone()
    }

    /// Send a captured frame, dropping it if the media task cannot keep up.
    fn send_frame(&self, frame: CameraFrame) {
        if let Some(sender) = self.sender() {
            let _ = sender.try_send(CameraMessage::Frame(frame));
        }
    }

    /// Tell the media task that the camera was turned off.
    ///
    /// Unlike frames, this must not be dropped, so it is sent from the tokio
    /// runtime where the send can wait for a free slot.
    pub(super) fn notify_stopped(&self) {
        let Some(sender) = self.sender() else {
            return;
        };

        spawn_tokio!(async move {
            let _ = sender.send(CameraMessage::Stopped).await;
        });
    }
}

/// An ongoing capture of the local camera.
///
/// The camera is closed when this is dropped.
pub(super) struct CameraCapture {
    /// The capture pipeline.
    pipeline: gst::Pipeline,
    /// The paintable of the self-view.
    paintable: gdk::Paintable,
    /// The guard of the watch on the bus of the pipeline.
    _bus_guard: gst::bus::BusWatchGuard,
}

impl std::fmt::Debug for CameraCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CameraCapture").finish_non_exhaustive()
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        // This closes the camera device.
        if let Err(error) = self.pipeline.set_state(gst::State::Null) {
            warn!("Could not stop the camera pipeline: {error}");
        }
    }
}

impl CameraCapture {
    /// Start capturing the default camera, sending the frames to the given
    /// sink.
    ///
    /// This asks the user for access to the camera via the portal, if it was
    /// not granted before. Must be called from the main thread.
    pub(super) async fn new(sink: Arc<CameraSink>) -> Result<Self, String> {
        let camera = default_camera().await?;
        debug!("Capturing camera {}", camera.display_name());

        let source = camera
            .device()
            .create_element(None)
            .map_err(|error| format!("failed to create the camera source: {error}"))?;

        Self::with_source(&source, sink)
    }

    /// The paintable rendering the captured frames.
    pub(super) fn paintable(&self) -> &gdk::Paintable {
        &self.paintable
    }

    /// Build and start the capture pipeline for the given source element.
    fn with_source(source: &gst::Element, sink: Arc<CameraSink>) -> Result<Self, String> {
        let caps = gst::Caps::from_str(CAPTURE_CAPS).expect("camera caps should be valid");

        let convert = make_element("videoconvert")?;
        let scale = make_element("videoscale")?;
        let rate = make_element("videorate")?;
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", &caps)
            .build()
            .map_err(|error| format!("failed to create the camera capsfilter: {error}"))?;
        let tee = make_element("tee")?;

        // The branch feeding the SFU. Frames are dropped rather than queued
        // when the encoder cannot keep up.
        let app_queue = gst::ElementFactory::make("queue")
            .property("max-size-buffers", 2u32)
            .property("max-size-bytes", 0u32)
            .property("max-size-time", 0u64)
            .property_from_str("leaky", "downstream")
            .build()
            .map_err(|error| format!("failed to create the camera queue: {error}"))?;
        let appsink = gst_app::AppSink::builder()
            .caps(&caps)
            .sync(false)
            .max_buffers(2)
            .drop(true)
            .build();

        // The branch feeding the self-view.
        let view_queue = gst::ElementFactory::make("queue")
            .property("max-size-buffers", 2u32)
            .property("max-size-bytes", 0u32)
            .property("max-size-time", 0u64)
            .property_from_str("leaky", "downstream")
            .build()
            .map_err(|error| format!("failed to create the self-view queue: {error}"))?;
        let paintable_sink = make_element("gtk4paintablesink")?;
        let paintable = paintable_sink.property::<gdk::Paintable>("paintable");

        let pipeline = gst::Pipeline::new();
        let elements = [
            source,
            &convert,
            &scale,
            &rate,
            &capsfilter,
            &tee,
            &app_queue,
            appsink.upcast_ref(),
            &view_queue,
            &paintable_sink,
        ];
        pipeline
            .add_many(elements)
            .map_err(|error| format!("failed to build the camera pipeline: {error}"))?;

        gst::Element::link_many([source, &convert, &scale, &rate, &capsfilter, &tee])
            .map_err(|error| format!("failed to link the camera pipeline: {error}"))?;
        gst::Element::link_many([&tee, &app_queue, appsink.upcast_ref()])
            .map_err(|error| format!("failed to link the camera pipeline: {error}"))?;
        gst::Element::link_many([&tee, &view_queue, &paintable_sink])
            .map_err(|error| format!("failed to link the self-view: {error}"))?;

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
            .add_watch_local(|_, message| {
                if let gst::MessageView::Error(error) = message.view() {
                    error!(
                        "Error in the camera pipeline: {} ({:?})",
                        error.error(),
                        error.debug()
                    );
                }
                glib::ControlFlow::Continue
            })
            .map_err(|_| "failed to watch the camera pipeline".to_owned())?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("failed to start the camera pipeline: {error}"))?;

        Ok(Self {
            pipeline,
            paintable,
            _bus_guard: bus_guard,
        })
    }
}

/// Create the `GStreamer` element with the given factory name.
fn make_element(name: &str) -> Result<gst::Element, String> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|error| format!("failed to create the `{name}` element: {error}"))
}

/// Request access to the cameras of the system and return the default one.
async fn default_camera() -> Result<aperture::Camera, String> {
    let provider = aperture::DeviceProvider::instance();

    // The provider is a singleton shared with the QR code scanner, and it can
    // only be given a file descriptor before it is started.
    if !provider.started() {
        let handle = spawn_tokio!(camera::request());
        let fd = match handle.await.expect("task was not aborted") {
            Ok(Some(fd)) => fd,
            Ok(None) => return Err("no camera is available".to_owned()),
            Err(error) => return Err(format!("could not access the camera: {error}")),
        };

        provider
            .set_fd(fd)
            .map_err(|error| format!("could not access the camera: {error}"))?;
        provider
            .start()
            .map_err(|error| format!("could not list the cameras: {error}"))?;
    }

    // The provider lists the cameras it already knows about when it starts,
    // but a device that is still being announced can show up shortly after.
    let deadline = std::time::Instant::now() + CAMERA_WAIT;
    loop {
        if let Some(camera) = provider.camera(0) {
            return Ok(camera);
        }

        if std::time::Instant::now() >= deadline {
            return Err("no camera is available".to_owned());
        }

        glib::timeout_future(CAMERA_POLL_INTERVAL).await;
    }
}

/// Extract a tightly packed I420 frame from the given sample.
fn frame_from_sample(sample: &gst::Sample) -> Option<CameraFrame> {
    let buffer = sample.buffer()?;
    let info = gst_video::VideoInfo::from_caps(sample.caps()?).ok()?;
    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info).ok()?;

    let width = info.width();
    let height = info.height();
    let chroma_width = width.div_ceil(2) as usize;
    let chroma_height = height.div_ceil(2) as usize;

    let mut data =
        Vec::with_capacity((width as usize * height as usize) + (chroma_width * chroma_height * 2));

    for plane in 0..3 {
        let (row_len, rows) = if plane == 0 {
            (width as usize, height as usize)
        } else {
            (chroma_width, chroma_height)
        };

        let stride = frame.plane_stride().get(plane)?.unsigned_abs() as usize;
        let plane_data = frame.plane_data(plane as u32).ok()?;

        for row in 0..rows {
            let start = row * stride;
            data.extend_from_slice(plane_data.get(start..start + row_len)?);
        }
    }

    Some(CameraFrame {
        width,
        height,
        data,
    })
}
