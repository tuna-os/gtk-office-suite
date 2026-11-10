//! Collection of methods for media.

use std::{str::FromStr, time::Duration};

use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};
use mime::Mime;
use ruma::UInt;

use crate::utils::OneshotNotifier;

pub(crate) mod audio;
pub(crate) mod audio_recorder;
pub(crate) mod image;
pub(crate) mod video;

/// Get a default filename for a mime type.
///
/// Tries to guess the file extension, but it might not find it.
///
/// If the mime type is unknown, it uses the name for `fallback`. The fallback
/// mime types that are recognized are `mime::IMAGE`, `mime::VIDEO` and
/// `mime::AUDIO`, other values will behave the same as `None`.
pub(crate) fn filename_for_mime(mime_type: Option<&str>, fallback: Option<mime::Name>) -> String {
    let (type_, extension) =
        if let Some(mime) = mime_type.and_then(|m| m.parse::<mime::Mime>().ok()) {
            let extension =
                mime_guess::get_mime_extensions(&mime).map(|extensions| extensions[0].to_owned());

            (Some(mime.type_().as_str().to_owned()), extension)
        } else {
            (fallback.map(|type_| type_.as_str().to_owned()), None)
        };

    let name = match type_.as_deref() {
        // Translators: Default name for image files.
        Some("image") => gettext("image"),
        // Translators: Default name for video files.
        Some("video") => gettext("video"),
        // Translators: Default name for audio files.
        Some("audio") => gettext("audio"),
        // Translators: Default name for files.
        _ => gettext("file"),
    };

    extension
        .map(|extension| format!("{name}.{extension}"))
        .unwrap_or(name)
}

/// Information about a file.
pub(crate) struct FileInfo {
    /// The mime type of the file.
    pub(crate) mime: Mime,
    /// The name of the file.
    pub(crate) filename: String,
    /// The size of the file in bytes.
    pub(crate) size: Option<u32>,
}

impl FileInfo {
    /// Try to load information about the given file.
    pub(crate) async fn try_from_file(file: &gio::File) -> Result<FileInfo, glib::Error> {
        let attributes: &[&str] = &[
            gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE,
            gio::FILE_ATTRIBUTE_STANDARD_DISPLAY_NAME,
            gio::FILE_ATTRIBUTE_STANDARD_SIZE,
        ];

        // Read mime type.
        let info = file
            .query_info_future(
                &attributes.join(","),
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::DEFAULT,
            )
            .await?;

        let mime = info
            .content_type()
            .and_then(|content_type| Mime::from_str(&content_type).ok())
            .unwrap_or(mime::APPLICATION_OCTET_STREAM);

        let filename = info.display_name().to_string();

        let raw_size = info.size();
        let size = if raw_size >= 0 {
            Some(raw_size.try_into().unwrap_or(u32::MAX))
        } else {
            None
        };

        Ok(FileInfo {
            mime,
            filename,
            size,
        })
    }
}

/// Load information for the given media file.
async fn load_gstreamer_media_info(file: &gio::File) -> Option<gst_pbutils::DiscovererInfo> {
    let timeout = gst::ClockTime::from_seconds(15);
    let discoverer = gst_pbutils::Discoverer::new(timeout).ok()?;

    let notifier = OneshotNotifier::new("load_gstreamer_media_info");
    let receiver = notifier.listen();

    discoverer.connect_discovered(move |_, info, _| {
        notifier.notify_value(Some(info.clone()));
    });

    discoverer.start();
    discoverer.discover_uri_async(&file.uri()).ok()?;

    let media_info = receiver.await;
    discoverer.stop();

    media_info
}

/// All errors that can occur when downloading a media to a file.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub(crate) enum MediaFileError {
    /// An error occurred when downloading the media.
    Sdk(#[from] matrix_sdk::Error),
    /// An error occurred when writing the media to a file.
    File(#[from] std::io::Error),
    /// We could not access the Matrix client via the [`Session`].
    ///
    /// [`Session`]: crate::session::Session
    #[error("Could not access session")]
    NoSession,
}

/// The dimensions of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameDimensions {
    /// The width of the frame.
    pub(crate) width: u32,
    /// The height of the frame.
    pub(crate) height: u32,
}

impl FrameDimensions {
    /// Construct a `FrameDimensions` from the given optional dimensions.
    pub(crate) fn from_options(width: Option<UInt>, height: Option<UInt>) -> Option<Self> {
        Some(Self {
            width: width?.try_into().ok()?,
            height: height?.try_into().ok()?,
        })
    }

    /// Get the dimension for the given orientation.
    pub(crate) fn dimension_for_orientation(self, orientation: gtk::Orientation) -> u32 {
        match orientation {
            gtk::Orientation::Vertical => self.height,
            _ => self.width,
        }
    }

    /// Get the dimension for the other orientation than the given one.
    pub(crate) fn dimension_for_other_orientation(self, orientation: gtk::Orientation) -> u32 {
        match orientation {
            gtk::Orientation::Vertical => self.width,
            _ => self.height,
        }
    }

    /// Whether these dimensions are greater than or equal to the given
    /// dimensions.
    ///
    /// Returns `true` if either `width` or `height` is bigger than or equal to
    /// the one in the other dimensions.
    pub(crate) fn ge(self, other: Self) -> bool {
        self.width >= other.width || self.height >= other.height
    }

    /// Increase both of these dimensions by the given value.
    pub(crate) const fn increase_by(mut self, value: u32) -> Self {
        self.width = self.width.saturating_add(value);
        self.height = self.height.saturating_add(value);
        self
    }

    /// Scale these dimensions with the given factor.
    pub(crate) const fn scale(mut self, factor: u32) -> Self {
        self.width = self.width.saturating_mul(factor);
        self.height = self.height.saturating_mul(factor);
        self
    }

    /// Scale these dimensions to fit into the requested dimensions while
    /// preserving the aspect ratio and respecting the given content fit.
    pub(crate) fn scale_to_fit(self, requested: Self, content_fit: gtk::ContentFit) -> Self {
        let w_ratio = f64::from(self.width) / f64::from(requested.width);
        let h_ratio = f64::from(self.height) / f64::from(requested.height);

        let resize_from_width = match content_fit {
            // The largest ratio wins so the frame fits into the requested dimensions.
            gtk::ContentFit::Contain | gtk::ContentFit::ScaleDown => w_ratio > h_ratio,
            // The smallest ratio wins so the frame fills the requested dimensions.
            gtk::ContentFit::Cover => w_ratio < h_ratio,
            // We just return the requested dimensions since we do not care about the ratio.
            _ => return requested,
        };
        let downscale_only = content_fit == gtk::ContentFit::ScaleDown;

        #[allow(clippy::cast_sign_loss)] // We need to convert the f64 to a u32.
        let (width, height) = if resize_from_width {
            if downscale_only && w_ratio <= 1.0 {
                // We do not want to upscale.
                return self;
            }

            let new_height = f64::from(self.height) / w_ratio;
            (requested.width, new_height as u32)
        } else {
            if downscale_only && h_ratio <= 1.0 {
                // We do not want to upscale.
                return self;
            }

            let new_width = f64::from(self.width) / h_ratio;
            (new_width as u32, requested.height)
        };

        Self { width, height }
    }
}

/// Get the string representation of the given elapsed time to present it in a
/// media player.
pub(crate) fn time_to_label(time: &Duration) -> String {
    let mut time = time.as_secs();

    let sec = time % 60;
    time -= sec;
    let min = (time % (60 * 60)) / 60;
    time -= min * 60;
    let hour = time / (60 * 60);

    if hour > 0 {
        // FIXME: Find how to localize this.
        // hour:minutes:seconds
        format!("{hour}:{min:02}:{sec:02}")
    } else {
        // FIXME: Find how to localize this.
        // minutes:seconds
        format!("{min:02}:{sec:02}")
    }
}
