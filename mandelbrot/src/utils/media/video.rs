//! Collection of methods for videos.

use gst::prelude::*;
use gst_video::prelude::*;
use gtk::{gdk, gio, glib, glib::clone, prelude::*};
use matrix_sdk::attachment::{BaseVideoInfo, Thumbnail};
use tracing::{error, warn};

use super::{
    image::{Blurhash, TextureThumbnailer},
    load_gstreamer_media_info,
};
use crate::utils::OneshotNotifier;

/// Load information and try to generate a thumbnail for the video in the given
/// file.
pub(crate) async fn load_video_info(
    file: &gio::File,
    widget: &impl IsA<gtk::Widget>,
) -> (BaseVideoInfo, Option<Thumbnail>) {
    let mut info = BaseVideoInfo::default();

    let Some(media_info) = load_gstreamer_media_info(file).await else {
        return (info, None);
    };

    info.duration = media_info.duration().map(Into::into);

    if let Some(stream_info) = media_info
        .video_streams()
        .first()
        .and_then(|s| s.downcast_ref::<gst_pbutils::DiscovererVideoInfo>())
    {
        info.width = Some(stream_info.width().into());
        info.height = Some(stream_info.height().into());
    }

    let (thumbnail, blurhash) = generate_video_thumbnail_and_blurhash(file, widget.upcast_ref())
        .await
        .unzip();
    info.blurhash = blurhash.map(|blurhash| blurhash.0);

    (info, thumbnail)
}

/// Generate a thumbnail and a Blurhash for the video in the given file.
async fn generate_video_thumbnail_and_blurhash(
    file: &gio::File,
    widget: &gtk::Widget,
) -> Option<(Thumbnail, Blurhash)> {
    let Some(renderer) = widget
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|w| w.renderer())
    else {
        // We cannot generate a thumbnail.
        error!("Could not get GdkRenderer");
        return None;
    };

    let notifier = OneshotNotifier::new("generate_video_thumbnail_and_blurhash");
    let receiver = notifier.listen();

    let pipeline = match create_thumbnailer_pipeline(&file.uri(), notifier.clone()) {
        Ok(pipeline) => pipeline,
        Err(error) => {
            warn!("Could not create pipeline for video thumbnail: {error}");
            return None;
        }
    };

    if pipeline.set_state(gst::State::Paused).is_err() {
        warn!("Could not initialize pipeline for video thumbnail");
        return None;
    }

    let bus = pipeline.bus().expect("Pipeline has a bus");

    let mut started = false;
    let _bus_guard = bus
        .add_watch(clone!(
            #[weak]
            pipeline,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |_, message| {
                match message.view() {
                    gst::MessageView::AsyncDone(_) => {
                        if !started {
                            // AsyncDone means that the pipeline has started now.
                            if pipeline.set_state(gst::State::Playing).is_err() {
                                warn!("Could not start pipeline for video thumbnail");
                                notifier.notify();

                                return glib::ControlFlow::Break;
                            }

                            started = true;
                        }

                        glib::ControlFlow::Continue
                    }
                    gst::MessageView::Eos(_) => {
                        // We have the thumbnail or we cannot have one.
                        glib::ControlFlow::Break
                    }
                    gst::MessageView::Error(error) => {
                        warn!("Could not generate video thumbnail: {error}");
                        notifier.notify();

                        glib::ControlFlow::Break
                    }
                    _ => glib::ControlFlow::Continue,
                }
            }
        ))
        .expect("Setting bus watch succeeds");

    let texture = receiver.await;

    // Clean up.
    let _ = pipeline.set_state(gst::State::Null);
    bus.set_flushing(true);

    let texture = texture?;
    let thumbnail_blurhash = TextureThumbnailer(texture)
        .generate_thumbnail_and_blurhash(widget.scale_factor(), &renderer);

    if thumbnail_blurhash.is_none() {
        warn!("Could not generate thumbnail and Blurhash from GdkTexture");
    }

    thumbnail_blurhash
}

/// Create a pipeline to get a thumbnail of the first frame.
fn create_thumbnailer_pipeline(
    uri: &str,
    notifier: OneshotNotifier<Option<gdk::Texture>>,
) -> Result<gst::Pipeline, glib::Error> {
    // Create our pipeline from a pipeline description string.
    let pipeline = gst::parse::launch(&format!(
        "uridecodebin uri={uri} ! videoconvert ! appsink name=sink"
    ))?
    .downcast::<gst::Pipeline>()
    .expect("Element is a pipeline");

    let appsink = pipeline
        .by_name("sink")
        .expect("Sink element is in the pipeline")
        .downcast::<gst_app::AppSink>()
        .expect("Sink element is an appsink");

    // Do not synchronize on the clock, we only want a snapshot asap.
    appsink.set_property("sync", false);

    // Tell the appsink what format we want, for simplicity we only accept 8-bit
    // RGB.
    appsink.set_caps(Some(
        &gst_video::VideoCapsBuilder::new()
            .format(gst_video::VideoFormat::Rgbx)
            .build(),
    ));

    let mut got_snapshot = false;

    // Listen to callbacks to get the data.
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                // Pull the sample out of the buffer.
                let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let Some(buffer) = sample.buffer() else {
                    warn!("Could not get buffer from appsink");
                    notifier.notify();

                    return Err(gst::FlowError::Error);
                };

                // Make sure that we only get a single buffer.
                if got_snapshot {
                    return Err(gst::FlowError::Eos);
                }
                got_snapshot = true;

                let Some(caps) = sample.caps() else {
                    warn!("Got video sample without caps");
                    notifier.notify();

                    return Err(gst::FlowError::Error);
                };
                let Ok(info) = gst_video::VideoInfo::from_caps(caps) else {
                    warn!("Could not parse video caps");
                    notifier.notify();

                    return Err(gst::FlowError::Error);
                };

                let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
                    .map_err(|_| {
                        warn!("Could not map video buffer readable");
                        notifier.notify();

                        gst::FlowError::Error
                    })?;

                if let Some(texture) = video_frame_to_texture(&frame) {
                    notifier.notify_value(Some(texture));
                    Err(gst::FlowError::Eos)
                } else {
                    warn!("Could not convert video frame to GdkTexture");
                    notifier.notify();
                    Err(gst::FlowError::Error)
                }
            })
            .build(),
    );

    Ok(pipeline)
}

/// Convert the given video frame to a `GdkTexture`.
fn video_frame_to_texture(
    frame: &gst_video::VideoFrameRef<&gst::BufferRef>,
) -> Option<gdk::Texture> {
    let format = video_format_to_memory_format(frame.format())?;
    let width = frame.width();
    let height = frame.height();
    let rowstride = frame.plane_stride()[0].try_into().ok()?;

    let texture = gdk::MemoryTexture::new(
        width.try_into().ok()?,
        height.try_into().ok()?,
        format,
        &glib::Bytes::from(frame.plane_data(0).ok()?),
        rowstride,
    )
    .upcast::<gdk::Texture>();

    Some(texture)
}

/// Convert the given `GstVideoFormat` to a `GdkMemoryFormat`.
fn video_format_to_memory_format(format: gst_video::VideoFormat) -> Option<gdk::MemoryFormat> {
    let format = match format {
        gst_video::VideoFormat::Bgrx => gdk::MemoryFormat::B8g8r8x8,
        gst_video::VideoFormat::Xrgb => gdk::MemoryFormat::X8r8g8b8,
        gst_video::VideoFormat::Rgbx => gdk::MemoryFormat::R8g8b8x8,
        gst_video::VideoFormat::Xbgr => gdk::MemoryFormat::X8b8g8r8,
        gst_video::VideoFormat::Bgra => gdk::MemoryFormat::B8g8r8a8,
        gst_video::VideoFormat::Argb => gdk::MemoryFormat::A8r8g8b8,
        gst_video::VideoFormat::Rgba => gdk::MemoryFormat::R8g8b8a8,
        gst_video::VideoFormat::Abgr => gdk::MemoryFormat::A8b8g8r8,
        gst_video::VideoFormat::Rgb => gdk::MemoryFormat::R8g8b8,
        gst_video::VideoFormat::Bgr => gdk::MemoryFormat::B8g8r8,
        _ => return None,
    };

    Some(format)
}
