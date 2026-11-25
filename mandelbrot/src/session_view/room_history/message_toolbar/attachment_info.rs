//! Loading of the metadata and thumbnail of a file to send as an attachment.

use matrix_sdk::attachment::{AttachmentInfo, BaseFileInfo, Thumbnail};

use crate::utils::media::{
    FileInfo, audio::load_audio_info, image::ImageInfoLoader, video::load_video_info,
};

/// Load the [`AttachmentInfo`] and the optional thumbnail of the given file.
///
/// The kind of info depends on the MIME type of the file. `widget` is used to
/// get the scale factor when generating thumbnails.
pub(super) async fn load_attachment_info(
    file: &gtk::gio::File,
    file_info: &FileInfo,
    widget: &impl gtk::prelude::IsA<gtk::Widget>,
) -> (AttachmentInfo, Option<Thumbnail>) {
    let size = file_info.size.map(Into::into);

    match file_info.mime.type_() {
        mime::IMAGE => {
            let (mut info, thumbnail) = ImageInfoLoader::from(file.clone())
                .load_info_and_thumbnail(file_info.size, widget)
                .await;
            info.size = size;

            (AttachmentInfo::Image(info), thumbnail)
        }
        mime::VIDEO => {
            let (mut info, thumbnail) = load_video_info(file, widget).await;
            info.size = size;
            (AttachmentInfo::Video(info), thumbnail)
        }
        mime::AUDIO => {
            let mut info = load_audio_info(file).await;
            info.size = size;
            (AttachmentInfo::Audio(info), None)
        }
        _ => (AttachmentInfo::File(BaseFileInfo { size }), None),
    }
}
