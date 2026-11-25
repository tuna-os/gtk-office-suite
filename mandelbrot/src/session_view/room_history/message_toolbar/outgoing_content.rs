//! Sending of composed messages and attachments.
//!
//! This module holds the logic that turns composer output into events sent
//! through the SDK, independently of the [`MessageToolbar`] widget. The widget
//! only decides when to send and how to report a failure to the user.
//!
//! [`MessageToolbar`]: super::MessageToolbar

use matrix_sdk::{
    attachment::{AttachmentInfo, Thumbnail},
    room::edit::EditedContent,
};
use matrix_sdk_ui::timeline::{AttachmentConfig, AttachmentSource};
use ruma::events::room::message::RoomMessageEventContent;
use tracing::error;

use super::composer_state::RelationInfo;
use crate::{session::Timeline, spawn_tokio};

/// The kind of message that failed to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SendFailure {
    /// A reply to another event.
    Reply,
    /// An edit of an existing event.
    Edit,
    /// A new message without relation.
    Message,
}

/// Send the given message content to the given timeline.
///
/// The relation determines whether this is a reply, an edit, or a plain
/// message. The error is logged here; the returned [`SendFailure`] lets the
/// caller pick the message to show to the user.
pub(super) async fn send_message(
    timeline: &Timeline,
    content: RoomMessageEventContent,
    relation: Option<RelationInfo>,
) -> Result<(), SendFailure> {
    let matrix_timeline = timeline.matrix_timeline();

    match relation {
        Some(RelationInfo::Reply(message_event)) => {
            let event_id = message_event.event_id();

            let handle =
                spawn_tokio!(async move { matrix_timeline.send_reply(content, event_id).await });

            handle
                .await
                .expect("task was not aborted")
                .map_err(|error| {
                    error!("Could not send reply: {error}");
                    SendFailure::Reply
                })
        }
        Some(RelationInfo::Edit(event_id)) => {
            let matrix_room = timeline.room().matrix_room().clone();
            let handle = spawn_tokio!(async move {
                let full_content = matrix_room
                    .make_edit_event(&event_id, EditedContent::RoomMessage(content))
                    .await
                    .map_err(matrix_sdk_ui::timeline::EditError::from)?;
                let send_queue = matrix_room.send_queue();
                send_queue.send(full_content).await?;
                Ok::<(), matrix_sdk_ui::timeline::Error>(())
            });

            handle.await.unwrap().map_err(|error| {
                error!("Could not send edit: {error}");
                SendFailure::Edit
            })
        }
        _ => {
            let handle = spawn_tokio!(async move {
                matrix_timeline
                    .send(content.with_relation(None).into())
                    .await
            });

            handle.await.unwrap().map_err(|error| {
                error!("Could not send message: {error}");
                SendFailure::Message
            })
        }
    }
}

/// Send the attachment with the given data to the given timeline.
pub(super) async fn send_attachment(
    timeline: &Timeline,
    source: AttachmentSource,
    mime: mime::Mime,
    info: AttachmentInfo,
    thumbnail: Option<Thumbnail>,
) -> Result<(), ()> {
    let config = AttachmentConfig {
        info: Some(info),
        thumbnail,
        ..Default::default()
    };

    let matrix_timeline = timeline.matrix_timeline();

    let handle = spawn_tokio!(async move {
        matrix_timeline
            .send_attachment(source, mime, config)
            .use_send_queue()
            .await
    });

    handle.await.unwrap().map_err(|error| {
        error!("Could not send file: {error}");
    })
}
