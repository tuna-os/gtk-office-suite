use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gdk, glib, glib::clone};
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineDetails, TimelineItemContent};
use ruma::{OwnedEventId, OwnedTransactionId, events::room::message::MessageType};
use tracing::{error, warn};

use super::{
    audio::MessageAudio,
    caption::MessageCaption,
    file::MessageFile,
    info::{MessageInfo, MessageInfoIcon},
    location::MessageLocation,
    poll::MessagePoll,
    reply::MessageReply,
    text::MessageText,
    visual_media::MessageVisualMedia,
};
use crate::{
    components::AudioPlayerMessage,
    prelude::*,
    session::{Event, Member, Room},
    session_view::room_history::message_toolbar::MessageEventSource,
    spawn,
    utils::matrix::{MediaMessage, MessageCacheKey},
};

#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[repr(i32)]
#[enum_type(name = "ContentFormat")]
pub enum ContentFormat {
    /// The content should appear at its natural size.
    #[default]
    Natural = 0,

    /// The content should appear in a smaller format without interactions, if
    /// possible.
    ///
    /// This has no effect on text replies.
    ///
    /// The related events of replies are not displayed.
    Compact = 1,

    /// Like `Compact`, but the content should be ellipsized if possible to show
    /// only a single line.
    Ellipsized = 2,
}

mod imp {
    use std::{cell::Cell, marker::PhantomData};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::MessageContent)]
    pub struct MessageContent {
        /// The displayed format of the message.
        #[property(get, set = Self::set_format, explicit_notify, builder(ContentFormat::default()))]
        format: Cell<ContentFormat>,
        /// The texture of the image preview displayed by the descendant of this
        /// widget, if any.
        #[property(get = Self::texture)]
        texture: PhantomData<Option<gdk::Texture>>,
        /// The widget with the visual media content of the event, if any.
        visual_media_widget: glib::WeakRef<MessageVisualMedia>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageContent {
        const NAME: &'static str = "ContentMessageContent";
        type Type = super::MessageContent;
        type ParentType = adw::Bin;
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageContent {}

    impl WidgetImpl for MessageContent {}
    impl BinImpl for MessageContent {}

    impl MessageContent {
        /// Set the displayed format of the message.
        fn set_format(&self, format: ContentFormat) {
            if self.format.get() == format {
                return;
            }

            self.format.set(format);
            self.obj().notify_format();
        }

        /// The texture of the image preview displayed by the descendant of this
        /// widget, if any.
        fn texture(&self) -> Option<gdk::Texture> {
            self.visual_media_widget.upgrade()?.texture()
        }

        /// Update the current visual media widget if necessary.
        pub(super) fn update_visual_media_widget(&self) {
            let prev_widget = self.visual_media_widget.upgrade();
            let current_widget = self.visual_media_widget();

            if prev_widget == current_widget {
                return;
            }

            let obj = self.obj();

            if let Some(visual_media) = &current_widget {
                visual_media.connect_texture_notify(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.notify_texture();
                    }
                ));
            }
            self.visual_media_widget.set(current_widget.as_ref());

            let prev_texture = prev_widget.and_then(|visual_media| visual_media.texture());
            let current_texture = current_widget.and_then(|visual_media| visual_media.texture());
            if prev_texture != current_texture {
                obj.notify_texture();
            }
        }

        /// The widget with the visual media content of the event, if any.
        ///
        /// This allows to access the descendant content while discarding the
        /// content of a related message, like a replied-to event, or the
        /// caption of the event.
        fn visual_media_widget(&self) -> Option<MessageVisualMedia> {
            let mut child = self.obj().child()?;

            // If it is a reply, the media is in the main content.
            if let Some(reply) = child.downcast_ref::<MessageReply>() {
                child = reply.content().child()?;
            }

            // If it is a caption, the media is the child of the caption.
            if let Some(caption) = child.downcast_ref::<MessageCaption>() {
                child = caption.child()?;
            }

            child.downcast::<MessageVisualMedia>().ok()
        }
    }
}

glib::wrapper! {
    /// The content of a message in the timeline.
    pub struct MessageContent(ObjectSubclass<imp::MessageContent>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageContent {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update this widget to present the given `Event`.
    pub(crate) fn update_for_event(&self, event: &Event) {
        let detect_at_room = event.can_contain_at_room() && event.sender().can_notify_room();

        let format = self.format();
        if format == ContentFormat::Natural
            && let Some(related_content) = event.reply_to_event_content()
        {
            match related_content {
                TimelineDetails::Unavailable => {
                    spawn!(
                        glib::Priority::HIGH,
                        clone!(
                            #[weak]
                            event,
                            async move {
                                if let Err(error) = event.fetch_missing_details().await {
                                    error!("Could not fetch event details: {error}");
                                }
                            }
                        )
                    );
                }
                TimelineDetails::Error(error) => {
                    error!(
                        "Could not fetch replied to event '{}': {error}",
                        event
                            .reply_to_id()
                            .expect("reply event should have replied-to event ID")
                    );
                }
                TimelineDetails::Ready(replied_to_event) => {
                    // We should have a strong reference to the list in the RoomHistory so we
                    // can use `get_or_create_members()`.
                    let replied_to_sender = event
                        .room()
                        .get_or_create_members()
                        .get_or_create(replied_to_event.sender);
                    let replied_to_detect_at_room = replied_to_event.content.can_contain_at_room()
                        && replied_to_sender.can_notify_room();

                    let reply = MessageReply::new();
                    reply.set_show_related_content_header(
                        replied_to_event.content.can_show_header(),
                    );
                    reply.set_related_content_sender(replied_to_sender.upcast_ref());
                    reply.related_content().build_content(
                        replied_to_event.content,
                        ContentFormat::Compact,
                        &replied_to_sender,
                        replied_to_detect_at_room,
                        None,
                        event.reply_to_id(),
                    );
                    reply.content().build_content(
                        event.content(),
                        ContentFormat::Natural,
                        &event.sender(),
                        detect_at_room,
                        event.transaction_id(),
                        event.event_id(),
                    );
                    self.set_child(Some(&reply));

                    self.imp().update_visual_media_widget();

                    return;
                }
                TimelineDetails::Pending => {}
            }
        }

        self.build_content(
            event.content(),
            format,
            &event.sender(),
            detect_at_room,
            event.transaction_id(),
            event.event_id(),
        );

        self.imp().update_visual_media_widget();
    }

    /// Update this widget to present the given related event.
    pub(crate) fn update_for_related_event(
        &self,
        msgtype: &MessageType,
        message_event: &MessageEventSource,
        sender: &Member,
    ) {
        let detect_at_room = message_event.can_contain_at_room() && sender.can_notify_room();

        self.build_message_content(
            msgtype,
            self.format(),
            sender,
            detect_at_room,
            MessageCacheKey {
                transaction_id: None,
                event_id: Some(message_event.event_id()),
                is_edited: message_event.is_edited(),
            },
        );
    }
}

impl IsABin for MessageContent {}

/// Helper trait for types used to build a message's content.
trait MessageContentContainer: ChildPropertyExt {
    /// Build the content widget of `event` as a child of this widget.
    fn build_content(
        &self,
        content: TimelineItemContent,
        format: ContentFormat,
        sender: &Member,
        detect_at_room: bool,
        transaction_id: Option<OwnedTransactionId>,
        event_id: Option<OwnedEventId>,
    ) {
        let room = sender.room();

        #[allow(clippy::match_wildcard_for_single_variants)]
        match content {
            TimelineItemContent::MsgLike(msg_like) => match msg_like.kind {
                MsgLikeKind::Message(message) => {
                    self.build_message_content(
                        message.msgtype(),
                        format,
                        sender,
                        detect_at_room,
                        MessageCacheKey {
                            transaction_id,
                            event_id,
                            is_edited: message.is_edited(),
                        },
                    );
                }
                MsgLikeKind::Sticker(sticker) => {
                    self.build_media_message_content(
                        sticker.content().clone().into(),
                        format,
                        &room,
                        detect_at_room,
                        MessageCacheKey {
                            transaction_id,
                            event_id,
                            is_edited: false,
                        },
                    );
                }
                MsgLikeKind::Poll(poll_state) => {
                    let child = self.child_or_default::<MessagePoll>();
                    child.set_poll(&room, event_id, &poll_state, format);
                }
                MsgLikeKind::UnableToDecrypt(_) => {
                    let child = self.child_or_default::<MessageInfo>();
                    child.set_info(
                        MessageInfoIcon::Warning,
                        &gettext("Could not decrypt this message, decryption will be retried once the keys are available.")
                    );
                }
                MsgLikeKind::Redacted => {
                    let child = self.child_or_default::<MessageInfo>();
                    child.set_info(MessageInfoIcon::Info, &gettext("This message was removed."));
                }
                msg_like_kind => {
                    warn!("Unsupported message-like event content: {msg_like_kind:?}");
                    let child = self.child_or_default::<MessageInfo>();
                    child.set_info(MessageInfoIcon::Warning, &gettext("Unsupported event"));
                }
            },
            content => {
                warn!("Unsupported event content: {content:?}");
                let child = self.child_or_default::<MessageInfo>();
                child.set_info(MessageInfoIcon::Warning, &gettext("Unsupported event"));
            }
        }
    }

    /// Build the content widget of the given message as a child of this widget.
    fn build_message_content(
        &self,
        msgtype: &MessageType,
        format: ContentFormat,
        sender: &Member,
        detect_at_room: bool,
        cache_key: MessageCacheKey,
    ) {
        let room = sender.room();

        if let Some(media_message) = MediaMessage::from_message(msgtype) {
            self.build_media_message_content(
                media_message,
                format,
                &room,
                detect_at_room,
                cache_key,
            );
            return;
        }

        match msgtype {
            MessageType::Emote(message) => {
                let child = self.child_or_default::<MessageText>();
                child.with_emote(
                    message.formatted.clone(),
                    message.body.clone(),
                    sender,
                    &room,
                    format,
                    detect_at_room,
                );
            }
            MessageType::Location(message) => {
                let child = self.child_or_default::<MessageLocation>();
                child.set_geo_uri(&message.geo_uri, format);
            }
            MessageType::Notice(message) => {
                let child = self.child_or_default::<MessageText>();
                child.with_markup(
                    message.formatted.clone(),
                    message.body.clone(),
                    &room,
                    format,
                    detect_at_room,
                );
            }
            MessageType::ServerNotice(message) => {
                let child = self.child_or_default::<MessageInfo>();
                child.set_info(MessageInfoIcon::Warning, &message.body.clone());
            }
            MessageType::Text(message) => {
                let child = self.child_or_default::<MessageText>();
                child.with_markup(
                    message.formatted.clone(),
                    message.body.clone(),
                    &room,
                    format,
                    detect_at_room,
                );
            }
            msgtype => {
                warn!("Event not supported: {msgtype:?}");
                let child = self.child_or_default::<MessageInfo>();
                child.set_info(MessageInfoIcon::Warning, &gettext("Unsupported event"));
            }
        }
    }

    /// Build the content widget of the given media message as a child of this
    /// widget.
    fn build_media_message_content(
        &self,
        media_message: MediaMessage,
        format: ContentFormat,
        room: &Room,
        detect_at_room: bool,
        cache_key: MessageCacheKey,
    ) {
        if let Some((caption, formatted_caption)) = media_message.caption() {
            let caption_widget = self.child_or_default::<MessageCaption>();

            caption_widget.set_caption(caption, formatted_caption, room, format, detect_at_room);

            caption_widget.build_media_content(media_message, format, room, cache_key);
        } else {
            self.build_media_content(media_message, format, room, cache_key);
        }
    }

    /// Build the content widget of the given media content as the child of this
    /// widget.
    ///
    /// If the child of the parent is already of the proper type, it is reused.
    fn build_media_content(
        &self,
        media_message: MediaMessage,
        format: ContentFormat,
        room: &Room,
        cache_key: MessageCacheKey,
    ) {
        match media_message {
            MediaMessage::Audio(audio) => {
                let Some(session) = room.session() else {
                    return;
                };
                let widget = self.child_or_default::<MessageAudio>();
                widget.set_audio_message(
                    AudioPlayerMessage::new(audio.into(), &session, cache_key),
                    format,
                );
            }
            MediaMessage::File(file) => {
                let widget = self.child_or_default::<MessageFile>();

                let media_message = MediaMessage::from(file);
                widget.set_filename(Some(media_message.display_name()));
                widget.set_format(format);
            }
            MediaMessage::Image(image) => {
                let widget = self.child_or_default::<MessageVisualMedia>();
                widget.set_media_message(image.into(), room, format, cache_key);
            }
            MediaMessage::Video(video) => {
                let widget = self.child_or_default::<MessageVisualMedia>();
                widget.set_media_message(video.into(), room, format, cache_key);
            }
            MediaMessage::Sticker(sticker) => {
                let widget = self.child_or_default::<MessageVisualMedia>();
                widget.set_media_message(sticker.into(), room, format, cache_key);
            }
        }
    }
}

impl<W> MessageContentContainer for W where W: IsABin {}

impl MessageContentContainer for MessageCaption {}
