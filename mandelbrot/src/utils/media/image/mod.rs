//! Collection of methods for images.

use std::{cmp::Ordering, error::Error, fmt, str::FromStr, time::Duration};

use gettextrs::gettext;
use gtk::{gdk, gio, glib, graphene, gsk, prelude::*};
use matrix_sdk::{
    Client,
    attachment::{BaseImageInfo, Thumbnail},
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
};
use ruma::{
    OwnedMxcUri,
    api::client::media::get_content_thumbnail::v3::Method,
    events::{
        room::{
            ImageInfo, MediaSource as CommonMediaSource, ThumbnailInfo,
            avatar::ImageInfo as AvatarImageInfo,
        },
        sticker::StickerMediaSource,
    },
};
use tracing::{error, warn};

mod queue;

pub(crate) use queue::{IMAGE_QUEUE, ImageRequestPriority};

use super::{FrameDimensions, MediaFileError};
use crate::{
    DISABLE_GLYCIN_SANDBOX, RUNTIME,
    components::AnimatedImagePaintable,
    utils::{File, save_data_to_tmp_file},
};

/// The maximum dimensions of a thumbnail in the timeline.
pub(crate) const THUMBNAIL_MAX_DIMENSIONS: FrameDimensions = FrameDimensions {
    width: 600,
    height: 400,
};
/// The content type of SVG.
const SVG_CONTENT_TYPE: &str = "image/svg+xml";
/// The content type of WebP.
const WEBP_CONTENT_TYPE: &str = "image/webp";
/// The default WebP quality used for a generated thumbnail.
const WEBP_DEFAULT_QUALITY: f32 = 60.0;
/// The maximum file size threshold in bytes for requesting or generating a
/// thumbnail.
///
/// If the file size of the original image is larger than this, we assume it is
/// worth it to request or generate a thumbnail, even if its dimensions are
/// smaller than wanted. This is particularly helpful for some image formats
/// that can take up a lot of space.
///
/// This is 1MB.
const THUMBNAIL_MAX_FILESIZE_THRESHOLD: u32 = 1024 * 1024;
/// The size threshold in pixels for requesting or generating a thumbnail.
///
/// If the original image is larger than dimensions + threshold, we assume it is
/// worth it to request or generate a thumbnail.
const THUMBNAIL_DIMENSIONS_THRESHOLD: u32 = 200;
/// The known image MIME types that can be animated.
///
/// From the list of [supported image formats of glycin].
///
/// [supported image formats of glycin]: https://gitlab.gnome.org/GNOME/glycin/-/tree/main?ref_type=heads#supported-image-formats
const SUPPORTED_ANIMATED_IMAGE_MIME_TYPES: &[&str] = &["image/gif", "image/png", "image/webp"];

/// The source for decoding an image.
enum ImageDecoderSource {
    /// The bytes containing the encoded image.
    Data(Vec<u8>),
    /// The file containing the encoded image.
    File(File),
}

impl ImageDecoderSource {
    /// The maximum size of the `Data` variant. This is 1 MB.
    const MAX_DATA_SIZE: usize = 1_048_576;

    /// Construct an `ImageSource` from the given bytes.
    ///
    /// If the size of the bytes are too big to be kept in memory, they are
    /// written to a temporary file.
    async fn with_bytes(bytes: Vec<u8>) -> Result<Self, MediaFileError> {
        if bytes.len() > Self::MAX_DATA_SIZE {
            Ok(Self::File(save_data_to_tmp_file(bytes).await?))
        } else {
            Ok(Self::Data(bytes))
        }
    }

    /// Convert this image source into a loader.
    ///
    /// Returns the created loader, and the image file, if any.
    fn into_loader(self) -> (glycin::Loader, Option<File>) {
        let (loader, file) = match self {
            Self::Data(bytes) => (
                glycin::Loader::for_bytes(&glib::Bytes::from_owned(bytes)),
                None,
            ),
            Self::File(file) => (glycin::Loader::new(&file.as_gfile()), Some(file)),
        };

        if DISABLE_GLYCIN_SANDBOX {
            loader.set_sandbox_selector(glycin::SandboxSelector::NotSandboxed);
        }

        (loader, file)
    }

    /// Decode this image source into an [`Image`].
    ///
    /// Set `request_dimensions` if the image will be shown at specific
    /// dimensions. To show the image at its natural size, set it to `None`.
    async fn decode_image(
        self,
        request_dimensions: Option<FrameDimensions>,
    ) -> Result<Image, ImageError> {
        let (loader, file) = self.into_loader();
        let decoder = loader.load_future().await?;

        let frame_request = request_dimensions.map(|request| {
            let original_dimensions = FrameDimensions {
                width: decoder.width(),
                height: decoder.height(),
            };

            original_dimensions.to_image_loader_request(request)
        });

        let first_frame = if let Some(frame_request) = &frame_request {
            decoder.specific_frame_future(frame_request).await?
        } else {
            decoder.next_frame_future().await?
        };

        Ok(Image {
            file,
            decoder,
            first_frame,
        })
    }
}

impl From<File> for ImageDecoderSource {
    fn from(value: File) -> Self {
        Self::File(value)
    }
}

impl From<gio::File> for ImageDecoderSource {
    fn from(value: gio::File) -> Self {
        Self::File(value.into())
    }
}

/// An image that was just loaded.
#[derive(Clone)]
pub(crate) struct Image {
    /// The file containing the image, if any.
    ///
    /// We need to keep a strong reference to the temporary file or it will be
    /// destroyed.
    file: Option<File>,
    /// The image decoder.
    decoder: glycin::Image,
    /// The first frame of the image.
    first_frame: glycin::Frame,
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image").finish_non_exhaustive()
    }
}

impl From<Image> for gdk::Paintable {
    fn from(value: Image) -> Self {
        if value.first_frame.has_delay() {
            AnimatedImagePaintable::new(value.decoder, value.first_frame, value.file).upcast()
        } else {
            value.first_frame.texture().upcast()
        }
    }
}

/// An API to load image information.
pub(crate) enum ImageInfoLoader {
    /// An image file.
    File(gio::File),
    /// A texture in memory.
    Texture(gdk::Texture),
}

impl ImageInfoLoader {
    /// Load the first frame for this source.
    ///
    /// We need to load the first frame of an image so that EXIF rotation is
    /// applied and we get the proper dimensions.
    async fn into_first_frame(self) -> Option<Frame> {
        match self {
            Self::File(file) => {
                let (loader, _) = ImageDecoderSource::from(file).into_loader();
                let frame = loader
                    .load_future()
                    .await
                    .ok()?
                    .next_frame_future()
                    .await
                    .ok()?;

                Some(Frame::Glycin(frame))
            }
            Self::Texture(texture) => Some(Frame::Texture(texture)),
        }
    }

    /// Load the information for this image.
    pub(crate) async fn load_info(self) -> BaseImageInfo {
        self.into_first_frame()
            .await
            .map(|f| f.info())
            .unwrap_or_default()
    }

    /// Load the information for this image and try to generate a thumbnail
    /// given the filesize of the original image.
    pub(crate) async fn load_info_and_thumbnail(
        self,
        filesize: Option<u32>,
        widget: &impl IsA<gtk::Widget>,
    ) -> (BaseImageInfo, Option<Thumbnail>) {
        let Some(frame) = self.into_first_frame().await else {
            return (BaseImageInfo::default(), None);
        };

        let mut info = frame.info();

        // Generate the same thumbnail dimensions as we will need in the timeline.
        let scale_factor = widget.scale_factor();
        let max_thumbnail_dimensions =
            FrameDimensions::thumbnail_max_dimensions(widget.scale_factor());

        if !filesize_is_too_big(filesize)
            && !frame
                .dimensions()
                .is_some_and(|d| d.needs_thumbnail(max_thumbnail_dimensions))
        {
            // It is not worth it to generate a thumbnail.
            info.blurhash = frame.generate_blurhash().map(|blurhash| blurhash.0);

            return (info, None);
        }

        let Some(renderer) = widget
            .root()
            .and_downcast::<gtk::Window>()
            .and_then(|w| w.renderer())
        else {
            // We cannot generate a thumbnail.
            error!("Could not get GdkRenderer");
            return (info, None);
        };

        let (thumbnail, blurhash) = frame
            .generate_thumbnail_and_blurhash(scale_factor, &renderer)
            .unzip();
        info.blurhash = blurhash.map(|blurhash| blurhash.0);

        (info, thumbnail)
    }
}

impl From<gio::File> for ImageInfoLoader {
    fn from(value: gio::File) -> Self {
        Self::File(value)
    }
}

impl From<gdk::Texture> for ImageInfoLoader {
    fn from(value: gdk::Texture) -> Self {
        Self::Texture(value)
    }
}

/// A frame of an image.
#[derive(Debug, Clone)]
enum Frame {
    /// A frame loaded via glycin.
    Glycin(glycin::Frame),
    /// A texture in memory,
    Texture(gdk::Texture),
}

impl Frame {
    /// The dimensions of the frame.
    fn dimensions(&self) -> Option<FrameDimensions> {
        match self {
            Self::Glycin(frame) => Some(FrameDimensions {
                width: frame.width(),
                height: frame.height(),
            }),
            Self::Texture(texture) => FrameDimensions::with_texture(texture),
        }
    }

    /// Whether the image that this frame belongs to is animated.
    fn is_animated(&self) -> bool {
        match self {
            Self::Glycin(frame) => frame.has_delay(),
            Self::Texture(_) => false,
        }
    }

    /// Get the `BaseImageInfo` for this frame.
    fn info(&self) -> BaseImageInfo {
        let dimensions = self.dimensions();
        BaseImageInfo {
            width: dimensions.map(|d| d.width.into()),
            height: dimensions.map(|d| d.height.into()),
            is_animated: Some(self.is_animated()),
            ..Default::default()
        }
    }

    /// Generate a Blurhash of this frame.
    fn generate_blurhash(self) -> Option<Blurhash> {
        let texture = match self {
            Self::Glycin(frame) => frame.texture(),
            Self::Texture(texture) => texture,
        };

        let blurhash = Blurhash::with_texture(&texture);

        if blurhash.is_none() {
            warn!("Could not generate Blurhash from GdkTexture");
        }

        blurhash
    }

    /// Generate a thumbnail and a Blurhash of this frame.
    ///
    /// We use the thumbnail to compute the blurhash, which should be less
    /// expensive than using the original frame.
    fn generate_thumbnail_and_blurhash(
        self,
        scale_factor: i32,
        renderer: &gsk::Renderer,
    ) -> Option<(Thumbnail, Blurhash)> {
        let texture = match self {
            Self::Glycin(frame) => frame.texture(),
            Self::Texture(texture) => texture,
        };

        let thumbnail_blurhash =
            TextureThumbnailer(texture).generate_thumbnail_and_blurhash(scale_factor, renderer);

        if thumbnail_blurhash.is_none() {
            warn!("Could not generate thumbnail and Blurhash from GdkTexture");
        }

        thumbnail_blurhash
    }
}

/// Extensions to `FrameDimensions` for computing thumbnail dimensions.
impl FrameDimensions {
    /// Get the maximum dimensions for a thumbnail with the given scale factor.
    pub(crate) fn thumbnail_max_dimensions(scale_factor: i32) -> Self {
        let scale_factor = scale_factor.try_into().unwrap_or(1);
        THUMBNAIL_MAX_DIMENSIONS.scale(scale_factor)
    }

    /// Construct a `FrameDimensions` for the given texture.
    fn with_texture(texture: &gdk::Texture) -> Option<Self> {
        Some(Self {
            width: texture.width().try_into().ok()?,
            height: texture.height().try_into().ok()?,
        })
    }

    /// Whether we should generate or request a thumbnail for these dimensions,
    /// given the wanted thumbnail dimensions.
    pub(super) fn needs_thumbnail(self, thumbnail_dimensions: FrameDimensions) -> bool {
        self.ge(thumbnail_dimensions.increase_by(THUMBNAIL_DIMENSIONS_THRESHOLD))
    }

    /// Downscale these dimensions to fit into the given maximum dimensions
    /// while preserving the aspect ratio.
    ///
    /// Returns `None` if these dimensions are smaller than the maximum
    /// dimensions.
    pub(super) fn downscale_for(self, max_dimensions: FrameDimensions) -> Option<Self> {
        if !self.ge(max_dimensions) {
            // We do not need to downscale.
            return None;
        }

        Some(self.scale_to_fit(max_dimensions, gtk::ContentFit::ScaleDown))
    }

    /// Convert these dimensions to a request for the image loader with the
    /// requested dimensions.
    fn to_image_loader_request(self, requested: Self) -> glycin::FrameRequest {
        let scaled = self.scale_to_fit(requested, gtk::ContentFit::Cover);

        let request = glycin::FrameRequest::new();
        request.set_scale(scaled.width, scaled.height);
        request
    }
}

/// A thumbnailer for a `GdkTexture`.
#[derive(Debug, Clone)]
pub(super) struct TextureThumbnailer(pub(super) gdk::Texture);

impl TextureThumbnailer {
    /// Downscale the texture if needed to fit into the given maximum thumbnail
    /// dimensions.
    ///
    /// Returns `None` if the dimensions of the texture are unknown.
    fn downscale_texture_if_needed(
        self,
        max_dimensions: FrameDimensions,
        renderer: &gsk::Renderer,
    ) -> Option<gdk::Texture> {
        let dimensions = FrameDimensions::with_texture(&self.0)?;

        let texture = if let Some(target_dimensions) = dimensions.downscale_for(max_dimensions) {
            let snapshot = gtk::Snapshot::new();
            let bounds = graphene::Rect::new(
                0.0,
                0.0,
                target_dimensions.width as f32,
                target_dimensions.height as f32,
            );
            snapshot.append_texture(&self.0, &bounds);
            let node = snapshot.to_node()?;
            renderer.render_texture(node, None)
        } else {
            self.0
        };

        Some(texture)
    }

    /// Convert the given texture memory format to the format needed to make a
    /// thumbnail.
    ///
    /// The WebP encoder only supports RGB and RGBA.
    ///
    /// Returns `None` if the format is unknown.
    fn texture_format_to_thumbnail_format(
        format: gdk::MemoryFormat,
    ) -> Option<(gdk::MemoryFormat, webp::PixelLayout)> {
        match format {
            gdk::MemoryFormat::B8g8r8a8Premultiplied
            | gdk::MemoryFormat::A8r8g8b8Premultiplied
            | gdk::MemoryFormat::R8g8b8a8Premultiplied
            | gdk::MemoryFormat::B8g8r8a8
            | gdk::MemoryFormat::A8r8g8b8
            | gdk::MemoryFormat::R8g8b8a8
            | gdk::MemoryFormat::R16g16b16a16Premultiplied
            | gdk::MemoryFormat::R16g16b16a16
            | gdk::MemoryFormat::R16g16b16a16FloatPremultiplied
            | gdk::MemoryFormat::R16g16b16a16Float
            | gdk::MemoryFormat::R32g32b32a32FloatPremultiplied
            | gdk::MemoryFormat::R32g32b32a32Float
            | gdk::MemoryFormat::G8a8Premultiplied
            | gdk::MemoryFormat::G8a8
            | gdk::MemoryFormat::G16a16Premultiplied
            | gdk::MemoryFormat::G16a16
            | gdk::MemoryFormat::A8
            | gdk::MemoryFormat::A16
            | gdk::MemoryFormat::A16Float
            | gdk::MemoryFormat::A32Float
            | gdk::MemoryFormat::A8b8g8r8Premultiplied
            | gdk::MemoryFormat::A8b8g8r8 => {
                Some((gdk::MemoryFormat::R8g8b8a8, webp::PixelLayout::Rgba))
            }
            gdk::MemoryFormat::R8g8b8
            | gdk::MemoryFormat::B8g8r8
            | gdk::MemoryFormat::R16g16b16
            | gdk::MemoryFormat::R16g16b16Float
            | gdk::MemoryFormat::R32g32b32Float
            | gdk::MemoryFormat::G8
            | gdk::MemoryFormat::G16
            | gdk::MemoryFormat::B8g8r8x8
            | gdk::MemoryFormat::X8r8g8b8
            | gdk::MemoryFormat::R8g8b8x8
            | gdk::MemoryFormat::X8b8g8r8 => {
                Some((gdk::MemoryFormat::R8g8b8, webp::PixelLayout::Rgb))
            }
            _ => None,
        }
    }

    /// Generate the thumbnail for the given scale factor, with the given
    /// `GskRenderer`, and a Blurhash.
    ///
    /// We use the thumbnail to compute the blurhash, which should be less
    /// expensive than using the original texture.
    pub(super) fn generate_thumbnail_and_blurhash(
        self,
        scale_factor: i32,
        renderer: &gsk::Renderer,
    ) -> Option<(Thumbnail, Blurhash)> {
        let max_thumbnail_dimensions = FrameDimensions::thumbnail_max_dimensions(scale_factor);
        let thumbnail = self.downscale_texture_if_needed(max_thumbnail_dimensions, renderer)?;
        let dimensions = FrameDimensions::with_texture(&thumbnail)?;

        let blurhash = Blurhash::with_texture(&thumbnail)?;

        let (downloader_format, webp_layout) =
            Self::texture_format_to_thumbnail_format(thumbnail.format())?;

        let mut downloader = gdk::TextureDownloader::new(&thumbnail);
        downloader.set_format(downloader_format);
        let (data, _) = downloader.download_bytes();

        let encoder = webp::Encoder::new(&data, webp_layout, dimensions.width, dimensions.height);
        let data = encoder.encode(WEBP_DEFAULT_QUALITY).to_vec();

        let size = data.len().try_into().ok()?;
        let content_type =
            mime::Mime::from_str(WEBP_CONTENT_TYPE).expect("content type should be valid");

        let thumbnail = Thumbnail {
            data,
            content_type,
            width: dimensions.width.into(),
            height: dimensions.height.into(),
            size,
        };

        Some((thumbnail, blurhash))
    }
}

/// A [Blurhash].
///
/// [Blurhash]: https://blurha.sh/
#[derive(Debug, Clone)]
pub(crate) struct Blurhash(pub(crate) String);

impl Blurhash {
    /// Try to compute the Blurhash for the given `GdkTexture`.
    pub(super) fn with_texture(texture: &gdk::Texture) -> Option<Self> {
        let dimensions = FrameDimensions::with_texture(texture)?;

        let mut downloader = gdk::TextureDownloader::new(texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (data, _) = downloader.download_bytes();

        let (components_x, components_y) = match dimensions.width.cmp(&dimensions.height) {
            Ordering::Less => (3, 4),
            Ordering::Equal => (3, 3),
            Ordering::Greater => (4, 3),
        };

        let hash = blurhash::encode(
            components_x,
            components_y,
            dimensions.width,
            dimensions.height,
            &data,
        )
        .inspect_err(|error| {
            warn!("Could not encode Blurhash: {error}");
        })
        .ok()?;

        Some(Self(hash))
    }

    /// Try to convert this Blurhash to a `GdkTexture` with the given
    /// dimensions.
    pub(crate) async fn into_texture(self, dimensions: FrameDimensions) -> Option<gdk::Texture> {
        // Because it can take some time, spawn on a separate thread.
        RUNTIME
            .spawn_blocking(move || {
                let data = blurhash::decode(&self.0, dimensions.width, dimensions.height, 1.0)
                    .inspect_err(|error| {
                        warn!("Could not decode Blurhash: {error}");
                    })
                    .ok()?;

                Some(
                    gdk::MemoryTexture::new(
                        dimensions.width.try_into().ok()?,
                        dimensions.height.try_into().ok()?,
                        gdk::MemoryFormat::R8g8b8a8,
                        &glib::Bytes::from_owned(data),
                        4 * dimensions.width as usize,
                    )
                    .upcast(),
                )
            })
            .await
            .expect("task was not aborted")
    }
}

/// An API to download a thumbnail for a media.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThumbnailDownloader<'a> {
    /// The main source of the image.
    ///
    /// This should be the source with the best quality.
    pub(crate) main: ImageSource<'a>,
    /// An alternative source for the image.
    ///
    /// This should be a source with a lower quality.
    pub(crate) alt: Option<ImageSource<'a>>,
}

impl ThumbnailDownloader<'_> {
    /// Download the thumbnail of the media.
    ///
    /// This might not return a thumbnail at the requested dimensions, depending
    /// on the sources and the homeserver.
    pub(crate) async fn download(
        self,
        client: Client,
        settings: ThumbnailSettings,
        priority: ImageRequestPriority,
    ) -> Result<Image, ImageError> {
        let dimensions = settings.dimensions;

        // First, select which source we are going to download from.
        let source = if let Some(alt) = self.alt {
            let is_animated = settings.animated && self.main.is_animated();

            if !is_animated
                && !self.main.can_be_thumbnailed()
                && (filesize_is_too_big(self.main.filesize())
                    || alt.dimensions().is_some_and(|s| s.ge(settings.dimensions)))
            {
                // Use the alternative source to save bandwidth.
                alt
            } else {
                self.main
            }
        } else {
            self.main
        };

        if source.should_thumbnail(
            settings.prefer_thumbnail,
            settings.animated,
            settings.dimensions,
        ) {
            // Try to get a thumbnail.
            let request = MediaRequestParameters {
                source: source.source.to_common_media_source(),
                format: MediaFormat::Thumbnail(settings.into()),
            };
            let handle = IMAGE_QUEUE.add_download_request(
                client.clone(),
                request,
                Some(dimensions),
                priority,
            );

            if let Ok(image) = handle.await {
                return Ok(image);
            }
        }

        // Fallback to downloading the full source.
        let request = MediaRequestParameters {
            source: source.source.to_common_media_source(),
            format: MediaFormat::File,
        };
        let handle = IMAGE_QUEUE.add_download_request(client, request, Some(dimensions), priority);

        handle.await
    }
}

/// The source of an image.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageSource<'a> {
    /// The source of the image.
    pub(crate) source: MediaSource<'a>,
    /// Information about the image.
    pub(crate) info: Option<ImageSourceInfo<'a>>,
}

impl ImageSource<'_> {
    /// Whether we should try to thumbnail this source for the given requested
    /// dimensions.
    fn should_thumbnail(
        &self,
        prefer_thumbnail: bool,
        prefer_animated: bool,
        thumbnail_dimensions: FrameDimensions,
    ) -> bool {
        if !self.can_be_thumbnailed() {
            return false;
        }

        // Even if we request animated thumbnails, not a lot of media repositories
        // support scaling animated images. So we just download the original to be able
        // to play it.
        if prefer_animated && self.is_animated() {
            return false;
        }

        let dimensions = self.dimensions();

        if prefer_thumbnail && dimensions.is_none() {
            return true;
        }

        dimensions.is_some_and(|d| d.needs_thumbnail(thumbnail_dimensions))
            || filesize_is_too_big(self.filesize())
    }

    /// Whether this source can be thumbnailed by the media repo.
    ///
    /// Returns `false` in these cases:
    ///
    /// - The image is encrypted, because it is not possible for the media repo
    ///   to make a thumbnail.
    /// - The image uses the SVG format, because media repos usually do not
    ///   accept to create a thumbnail of those.
    fn can_be_thumbnailed(&self) -> bool {
        !self.source.is_encrypted()
            && self
                .info
                .and_then(|i| i.mimetype)
                .is_none_or(|m| m != SVG_CONTENT_TYPE)
    }

    /// The filesize of this source.
    fn filesize(&self) -> Option<u32> {
        self.info.and_then(|i| i.filesize)
    }

    /// The dimensions of this source.
    fn dimensions(&self) -> Option<FrameDimensions> {
        self.info.and_then(|i| i.dimensions)
    }

    /// Whether this source is animated.
    ///
    /// Returns `false` if the info does not say that it is animated, or if the
    /// MIME type is not one of the supported animated image formats.
    fn is_animated(&self) -> bool {
        if self
            .info
            .and_then(|i| i.is_animated)
            .is_none_or(|is_animated| !is_animated)
        {
            return false;
        }

        self.info
            .and_then(|i| i.mimetype)
            .is_some_and(|mimetype| SUPPORTED_ANIMATED_IMAGE_MIME_TYPES.contains(&mimetype))
    }
}

/// Whether the given filesize is considered too big to be the preferred source
/// to download.
fn filesize_is_too_big(filesize: Option<u32>) -> bool {
    filesize.is_some_and(|s| s > THUMBNAIL_MAX_FILESIZE_THRESHOLD)
}

/// The source of a media file.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MediaSource<'a> {
    /// A common media source.
    Common(&'a CommonMediaSource),
    /// The media source of a sticker.
    Sticker(&'a StickerMediaSource),
    /// An MXC URI.
    Uri(&'a OwnedMxcUri),
}

impl MediaSource<'_> {
    /// Whether this source is encrypted.
    fn is_encrypted(&self) -> bool {
        match self {
            Self::Common(source) => matches!(source, CommonMediaSource::Encrypted(_)),
            Self::Sticker(source) => matches!(source, StickerMediaSource::Encrypted(_)),
            Self::Uri(_) => false,
        }
    }

    /// Get this source as a `CommonMediaSource`.
    fn to_common_media_source(self) -> CommonMediaSource {
        match self {
            Self::Common(source) => source.clone(),
            Self::Sticker(source) => source.clone().into(),
            Self::Uri(uri) => CommonMediaSource::Plain(uri.clone()),
        }
    }
}

impl<'a> From<&'a CommonMediaSource> for MediaSource<'a> {
    fn from(value: &'a CommonMediaSource) -> Self {
        Self::Common(value)
    }
}

impl<'a> From<&'a StickerMediaSource> for MediaSource<'a> {
    fn from(value: &'a StickerMediaSource) -> Self {
        Self::Sticker(value)
    }
}

impl<'a> From<&'a OwnedMxcUri> for MediaSource<'a> {
    fn from(value: &'a OwnedMxcUri) -> Self {
        Self::Uri(value)
    }
}

/// Information about the source of an image.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ImageSourceInfo<'a> {
    /// The dimensions of the image.
    dimensions: Option<FrameDimensions>,
    /// The MIME type of the image.
    mimetype: Option<&'a str>,
    /// The file size of the image.
    filesize: Option<u32>,
    /// Whether the image is animated.
    is_animated: Option<bool>,
}

impl<'a> From<&'a ImageInfo> for ImageSourceInfo<'a> {
    fn from(value: &'a ImageInfo) -> Self {
        Self {
            dimensions: FrameDimensions::from_options(value.width, value.height),
            mimetype: value.mimetype.as_deref(),
            filesize: value.size.and_then(|u| u.try_into().ok()),
            is_animated: value.is_animated,
        }
    }
}

impl<'a> From<&'a ThumbnailInfo> for ImageSourceInfo<'a> {
    fn from(value: &'a ThumbnailInfo) -> Self {
        Self {
            dimensions: FrameDimensions::from_options(value.width, value.height),
            mimetype: value.mimetype.as_deref(),
            filesize: value.size.and_then(|u| u.try_into().ok()),
            is_animated: None,
        }
    }
}

impl<'a> From<&'a AvatarImageInfo> for ImageSourceInfo<'a> {
    fn from(value: &'a AvatarImageInfo) -> Self {
        Self {
            dimensions: FrameDimensions::from_options(value.width, value.height),
            mimetype: value.mimetype.as_deref(),
            filesize: value.size.and_then(|u| u.try_into().ok()),
            is_animated: None,
        }
    }
}

/// The settings for downloading a thumbnail.
#[derive(Debug, Clone)]
pub(crate) struct ThumbnailSettings {
    /// The requested dimensions of the thumbnail.
    pub(crate) dimensions: FrameDimensions,
    /// The method to use to resize the thumbnail.
    pub(crate) method: Method,
    /// Whether to request an animated thumbnail.
    pub(crate) animated: bool,
    /// Whether we should prefer to get a thumbnail if dimensions are unknown.
    ///
    /// This is particularly useful for avatars where we will prefer to save
    /// bandwidth and memory usage as we download a lot of them and they might
    /// appear several times on the screen. For media messages, we will on the
    /// contrary prefer to download the original content to reduce the space
    /// taken in the media cache.
    pub(crate) prefer_thumbnail: bool,
}

impl From<ThumbnailSettings> for MediaThumbnailSettings {
    fn from(value: ThumbnailSettings) -> Self {
        let ThumbnailSettings {
            dimensions,
            method,
            animated,
            ..
        } = value;

        MediaThumbnailSettings {
            method,
            width: dimensions.width.into(),
            height: dimensions.height.into(),
            animated,
        }
    }
}

/// An error encountered when loading an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageError {
    /// Could not download the image.
    Download,
    /// Could not save the image to a temporary file.
    File,
    /// The image uses an unsupported format.
    UnsupportedFormat,
    /// An unexpected error occurred.
    Unknown,
    /// The request for the image was aborted.
    Aborted,
}

impl ImageError {
    /// Log the given image error.
    fn log_error(error: impl fmt::Display) {
        warn!("Could not decode image: {error}");
    }
}

impl Error for ImageError {}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Download => gettext("Could not retrieve media"),
            Self::UnsupportedFormat => gettext("Image format not supported"),
            Self::File | Self::Unknown | Self::Aborted => gettext("An unexpected error occurred"),
        };

        f.write_str(&s)
    }
}

impl From<MediaFileError> for ImageError {
    fn from(value: MediaFileError) -> Self {
        Self::log_error(&value);

        match value {
            MediaFileError::Sdk(_) => Self::Download,
            MediaFileError::File(_) => Self::File,
            MediaFileError::NoSession => Self::Unknown,
        }
    }
}

impl From<glib::Error> for ImageError {
    fn from(value: glib::Error) -> Self {
        Self::log_error(&value);

        if let Some(glycin::LoaderError::UnknownImageFormat) = value.kind() {
            Self::UnsupportedFormat
        } else {
            Self::Unknown
        }
    }
}

/// Extensions to [`glycin::Frame`].
pub(crate) trait GlycinFrameExt {
    /// Whether the frame has a delay, which means that the image is animated.
    fn has_delay(&self) -> bool;

    /// How long to show this frame for if the image is animated, as a
    /// [`Duration`].
    fn delay_duration(&self) -> Option<Duration>;

    /// Convert this frame to a [`gdk::Texture`].
    fn texture(&self) -> gdk::Texture;
}

impl GlycinFrameExt for glycin::Frame {
    fn has_delay(&self) -> bool {
        // glycin always computes a suitable delay if the image is animated but its
        // delay is set to 0, so 0 should mean that the image is not animated.
        self.delay() > 0
    }

    fn delay_duration(&self) -> Option<Duration> {
        self.has_delay()
            .then(|| u64::try_from(self.delay()).ok())
            .flatten()
            .map(Duration::from_micros)
    }

    fn texture(&self) -> gdk::Texture {
        glycin_gtk4::frame_get_texture(self)
    }
}
