use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};
use matrix_sdk::Client;
use ruma::events::{
    room::message::{
        AudioMessageEventContent, FileMessageEventContent, FormattedBody, ImageMessageEventContent,
        MessageType, VideoMessageEventContent,
    },
    sticker::StickerEventContent,
};
use tracing::{debug, error};

use crate::{
    components::ContentType,
    gettext_f,
    prelude::*,
    toast,
    utils::{
        File,
        media::{
            FrameDimensions, MediaFileError,
            audio::normalize_waveform,
            image::{
                Blurhash, Image, ImageError, ImageRequestPriority, ImageSource,
                ThumbnailDownloader, ThumbnailSettings,
            },
        },
        save_data_to_tmp_file,
    },
};

/// Get the filename of a media message.
macro_rules! filename {
    ($message:ident, $mime_fallback:expr) => {{
        let mut filename = $message.filename().to_owned();
        filename.clean_string();

        if filename.is_empty() {
            let mimetype = $message
                .info
                .as_ref()
                .and_then(|info| info.mimetype.as_deref());

            $crate::utils::media::filename_for_mime(mimetype, $mime_fallback)
        } else {
            filename
        }
    }};
}

/// A media message.
#[derive(Debug, Clone)]
pub(crate) enum MediaMessage {
    /// An audio.
    Audio(AudioMessageEventContent),
    /// A file.
    File(FileMessageEventContent),
    /// An image.
    Image(ImageMessageEventContent),
    /// A video.
    Video(VideoMessageEventContent),
    /// A sticker.
    Sticker(Box<StickerEventContent>),
}

impl MediaMessage {
    /// Construct a `MediaMessage` from the given message.
    pub(crate) fn from_message(msgtype: &MessageType) -> Option<Self> {
        match msgtype {
            MessageType::Audio(c) => Some(Self::Audio(c.clone())),
            MessageType::File(c) => Some(Self::File(c.clone())),
            MessageType::Image(c) => Some(Self::Image(c.clone())),
            MessageType::Video(c) => Some(Self::Video(c.clone())),
            _ => None,
        }
    }

    /// The name of the media, as displayed in the interface.
    ///
    /// This is usually the filename in the message, except:
    ///
    /// - For a voice message, it's a placeholder string because file names are
    ///   usually generated randomly.
    /// - For a sticker, this returns the description of the sticker, because
    ///   they do not have a filename.
    pub(crate) fn display_name(&self) -> String {
        match self {
            Self::Audio(c) => {
                if c.voice.is_some() {
                    gettext("Voice Message")
                } else {
                    filename!(c, Some(mime::AUDIO))
                }
            }
            Self::File(c) => filename!(c, None),
            Self::Image(c) => filename!(c, Some(mime::IMAGE)),
            Self::Video(c) => filename!(c, Some(mime::VIDEO)),
            Self::Sticker(c) => c.body.clone(),
        }
    }

    /// The filename of the media, used when saving the file.
    ///
    /// This is usually the filename in the message, except:
    ///
    /// - For a voice message, it's a generated name that uses the timestamp of
    ///   the message.
    /// - For a sticker, this returns the description of the sticker, because
    ///   they do not have a filename.
    pub(crate) fn filename(&self, timestamp: &glib::DateTime) -> String {
        match self {
            Self::Audio(c) => {
                let mut filename = filename!(c, Some(mime::AUDIO));

                if c.voice.is_some() {
                    let datetime = timestamp
                        .to_local()
                        .and_then(|local_timestamp| local_timestamp.format("%Y-%m-%d %H-%M-%S"))
                        // Fallback to the timestamp in seconds.
                        .map_or_else(|_| timestamp.second().to_string(), String::from);
                    // Translators: this is the name of the file that the voice message is saved as.
                    // Do NOT translate the content between '{' and '}', this is a variable name
                    // corresponding to a date and time, e.g. "2017-05-21 12-24-03".
                    let name =
                        gettext_f("Voice Message From {datetime}", &[("datetime", &datetime)]);

                    filename = filename
                        .rsplit_once('.')
                        .map(|(_, extension)| format!("{name}.{extension}"))
                        .unwrap_or(name);
                }

                filename
            }
            Self::File(c) => filename!(c, None),
            Self::Image(c) => filename!(c, Some(mime::IMAGE)),
            Self::Video(c) => filename!(c, Some(mime::VIDEO)),
            Self::Sticker(c) => c.body.clone(),
        }
    }

    /// The caption of the media, if any.
    ///
    /// Returns `Some((body, formatted_body))` if the media includes a caption.
    pub(crate) fn caption(&self) -> Option<(String, Option<FormattedBody>)> {
        let mut caption = match self {
            Self::Audio(c) => c
                .caption()
                .map(|caption| (caption.to_owned(), c.formatted.clone())),
            Self::File(c) => c
                .caption()
                .map(|caption| (caption.to_owned(), c.formatted.clone())),
            Self::Image(c) => c
                .caption()
                .map(|caption| (caption.to_owned(), c.formatted.clone())),
            Self::Video(c) => c
                .caption()
                .map(|caption| (caption.to_owned(), c.formatted.clone())),
            Self::Sticker(_) => None,
        };

        caption.take_if(|(caption, formatted)| {
            caption.clean_string();
            formatted.clean_string();

            caption.is_empty()
        });

        caption
    }

    /// Fetch the content of the media with the given client.
    ///
    /// Returns an error if something occurred while fetching the content.
    pub(crate) async fn into_content(self, client: &Client) -> Result<Vec<u8>, matrix_sdk::Error> {
        let media = client.media();

        macro_rules! content {
            ($event_content:expr) => {{
                Ok(
                    $crate::spawn_tokio!(
                        async move { media.get_file(&$event_content, true).await }
                    )
                    .await
                    .unwrap()?
                    .expect("All media message types have a file"),
                )
            }};
        }

        match self {
            Self::Audio(c) => content!(c),
            Self::File(c) => content!(c),
            Self::Image(c) => content!(c),
            Self::Video(c) => content!(c),
            Self::Sticker(c) => content!(*c),
        }
    }

    /// Fetch the content of the media with the given client and write it to a
    /// temporary file.
    ///
    /// Returns an error if something occurred while fetching the content.
    pub(crate) async fn into_tmp_file(self, client: &Client) -> Result<File, MediaFileError> {
        let data = self.into_content(client).await?;
        Ok(save_data_to_tmp_file(data).await?)
    }

    /// Save the content of the media to a file selected by the user.
    ///
    /// Shows a dialog to the user to select a file on the system.
    pub(crate) async fn save_to_file(
        self,
        timestamp: &glib::DateTime,
        client: &Client,
        parent: &impl IsA<gtk::Widget>,
    ) {
        let filename = self.filename(timestamp);

        let data = match self.into_content(client).await {
            Ok(data) => data,
            Err(error) => {
                error!("Could not retrieve media file: {error}");
                toast!(parent, error.to_user_facing());

                return;
            }
        };

        let dialog = gtk::FileDialog::builder()
            .title(gettext("Save File"))
            .modal(true)
            .accept_label(gettext("Save"))
            .initial_name(filename)
            .build();

        match dialog
            .save_future(parent.root().and_downcast_ref::<gtk::Window>())
            .await
        {
            Ok(file) => {
                if let Err(error) = file.replace_contents(
                    &data,
                    None,
                    false,
                    gio::FileCreateFlags::REPLACE_DESTINATION,
                    gio::Cancellable::NONE,
                ) {
                    error!("Could not save file: {error}");
                    toast!(parent, gettext("Could not save file"));
                }
            }
            Err(error) => {
                if error.matches(gtk::DialogError::Dismissed) {
                    debug!("File dialog dismissed by user");
                } else {
                    error!("Could not access file: {error}");
                    toast!(parent, gettext("Could not access file"));
                }
            }
        }
    }
}

impl From<AudioMessageEventContent> for MediaMessage {
    fn from(value: AudioMessageEventContent) -> Self {
        Self::Audio(value)
    }
}

impl From<FileMessageEventContent> for MediaMessage {
    fn from(value: FileMessageEventContent) -> Self {
        Self::File(value)
    }
}

impl From<StickerEventContent> for MediaMessage {
    fn from(value: StickerEventContent) -> Self {
        Self::Sticker(value.into())
    }
}

/// A visual media message.
#[derive(Debug, Clone)]
pub(crate) enum VisualMediaMessage {
    /// An image.
    Image(ImageMessageEventContent),
    /// A video.
    Video(VideoMessageEventContent),
    /// A sticker.
    Sticker(Box<StickerEventContent>),
}

impl VisualMediaMessage {
    /// Construct a `VisualMediaMessage` from the given message.
    pub(crate) fn from_message(msgtype: &MessageType) -> Option<Self> {
        match msgtype {
            MessageType::Image(c) => Some(Self::Image(c.clone())),
            MessageType::Video(c) => Some(Self::Video(c.clone())),
            _ => None,
        }
    }

    /// The filename of the media.
    ///
    /// For a sticker, this returns the description of the sticker.
    pub(crate) fn filename(&self) -> String {
        match self {
            Self::Image(c) => filename!(c, Some(mime::IMAGE)),
            Self::Video(c) => filename!(c, Some(mime::VIDEO)),
            Self::Sticker(c) => c.body.clone(),
        }
    }

    /// The dimensions of the media, if any.
    pub(crate) fn dimensions(&self) -> Option<FrameDimensions> {
        let (width, height) = match self {
            Self::Image(c) => c.info.as_ref().map(|i| (i.width, i.height))?,
            Self::Video(c) => c.info.as_ref().map(|i| (i.width, i.height))?,
            Self::Sticker(c) => (c.info.width, c.info.height),
        };
        FrameDimensions::from_options(width, height)
    }

    /// The type of the media.
    pub(crate) fn visual_media_type(&self) -> VisualMediaType {
        match self {
            Self::Image(_) => VisualMediaType::Image,
            Self::Sticker(_) => VisualMediaType::Sticker,
            Self::Video(_) => VisualMediaType::Video,
        }
    }

    /// The content type of the media.
    pub(crate) fn content_type(&self) -> ContentType {
        match self {
            Self::Image(_) | Self::Sticker(_) => ContentType::Image,
            Self::Video(_) => ContentType::Video,
        }
    }

    /// Get the Blurhash of the media, if any.
    pub(crate) fn blurhash(&self) -> Option<Blurhash> {
        match self {
            Self::Image(image_content) => image_content.info.as_deref()?.blurhash.clone(),
            Self::Sticker(sticker_content) => sticker_content.info.blurhash.clone(),
            Self::Video(video_content) => video_content.info.as_deref()?.blurhash.clone(),
        }
        .map(Blurhash)
    }

    /// Fetch a thumbnail of the media with the given client and thumbnail
    /// settings.
    ///
    /// This might not return a thumbnail at the requested size, depending on
    /// the message and the homeserver.
    ///
    /// Returns `Ok(None)` if no thumbnail could be retrieved and no fallback
    /// could be downloaded. This only applies to video messages.
    ///
    /// Returns an error if something occurred while fetching the content or
    /// loading it.
    pub(crate) async fn thumbnail(
        &self,
        client: Client,
        settings: ThumbnailSettings,
        priority: ImageRequestPriority,
    ) -> Result<Option<Image>, ImageError> {
        let downloader = match &self {
            Self::Image(c) => {
                let image_info = c.info.as_deref();
                ThumbnailDownloader {
                    main: ImageSource {
                        source: (&c.source).into(),
                        info: image_info.map(Into::into),
                    },
                    alt: image_info.and_then(|i| {
                        i.thumbnail_source.as_ref().map(|s| ImageSource {
                            source: s.into(),
                            info: i.thumbnail_info.as_deref().map(Into::into),
                        })
                    }),
                }
            }
            Self::Video(c) => {
                let Some(video_info) = c.info.as_deref() else {
                    return Ok(None);
                };
                let Some(thumbnail_source) = video_info.thumbnail_source.as_ref() else {
                    return Ok(None);
                };

                ThumbnailDownloader {
                    main: ImageSource {
                        source: thumbnail_source.into(),
                        info: video_info.thumbnail_info.as_deref().map(Into::into),
                    },
                    alt: None,
                }
            }
            Self::Sticker(c) => {
                let image_info = &c.info;
                ThumbnailDownloader {
                    main: ImageSource {
                        source: (&c.source).into(),
                        info: Some(image_info.into()),
                    },
                    alt: image_info.thumbnail_source.as_ref().map(|s| ImageSource {
                        source: s.into(),
                        info: image_info.thumbnail_info.as_deref().map(Into::into),
                    }),
                }
            }
        };

        downloader
            .download(client, settings, priority)
            .await
            .map(Some)
    }

    /// Fetch the content of the media with the given client and write it to a
    /// temporary file.
    ///
    /// Returns an error if something occurred while fetching the content or
    /// saving the content to a file.
    pub(crate) async fn into_tmp_file(self, client: &Client) -> Result<File, MediaFileError> {
        MediaMessage::from(self).into_tmp_file(client).await
    }

    /// Save the content of the media to a file selected by the user.
    ///
    /// Shows a dialog to the user to select a file on the system.
    pub(crate) async fn save_to_file(
        self,
        timestamp: &glib::DateTime,
        client: &Client,
        parent: &impl IsA<gtk::Widget>,
    ) {
        MediaMessage::from(self)
            .save_to_file(timestamp, client, parent)
            .await;
    }
}

impl From<ImageMessageEventContent> for VisualMediaMessage {
    fn from(value: ImageMessageEventContent) -> Self {
        Self::Image(value)
    }
}

impl From<VideoMessageEventContent> for VisualMediaMessage {
    fn from(value: VideoMessageEventContent) -> Self {
        Self::Video(value)
    }
}

impl From<StickerEventContent> for VisualMediaMessage {
    fn from(value: StickerEventContent) -> Self {
        Self::Sticker(value.into())
    }
}

impl From<Box<StickerEventContent>> for VisualMediaMessage {
    fn from(value: Box<StickerEventContent>) -> Self {
        Self::Sticker(value)
    }
}

impl From<VisualMediaMessage> for MediaMessage {
    fn from(value: VisualMediaMessage) -> Self {
        match value {
            VisualMediaMessage::Image(c) => Self::Image(c),
            VisualMediaMessage::Video(c) => Self::Video(c),
            VisualMediaMessage::Sticker(c) => Self::Sticker(c),
        }
    }
}

/// The type of a visual media message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisualMediaType {
    /// An image.
    Image,
    /// A video.
    Video,
    /// A sticker.
    Sticker,
}

/// Extension trait for audio messages.
pub(crate) trait AudioMessageExt {
    /// Get the normalized waveform in this audio message, if any.
    ///
    /// A normalized waveform is a waveform containing only values between 0 and
    /// 1.
    fn normalized_waveform(&self) -> Option<Vec<f32>>;
}

impl AudioMessageExt for AudioMessageEventContent {
    fn normalized_waveform(&self) -> Option<Vec<f32>> {
        let waveform = &self.audio.as_ref()?.waveform;

        if waveform.is_empty() {
            return None;
        }

        Some(normalize_waveform(
            waveform
                .iter()
                .map(|amplitude| u64::from(amplitude.get()) as f64)
                .collect(),
        ))
    }
}
