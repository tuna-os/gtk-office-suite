use std::{collections::HashMap, time::Duration};

use adw::{prelude::*, subclass::prelude::*};
use futures_util::{StreamExt, future, lock::Mutex, pin_mut};
use gettextrs::{gettext, pgettext};
use gtk::{gdk, gio, glib, glib::clone};
use matrix_sdk::{
    attachment::{AttachmentInfo, BaseAudioInfo, BaseFileInfo, Thumbnail},
    room::edit::EditedContent,
};
use matrix_sdk_ui::timeline::{
    AttachmentConfig, AttachmentSource, TimelineEventItemId, TimelineItemContent,
};
use ruma::events::{
    AnyMessageLikeEventContent, Mentions,
    room::{
        message::{LocationMessageEventContent, MessageType, RoomMessageEventContent},
        tombstone::RoomTombstoneEventContent,
    },
};
use tracing::{debug, error, warn};

mod attachment_dialog;
mod completion;
mod composer_parser;
mod composer_state;
mod create_poll_dialog;

pub(crate) use self::composer_state::{ComposerState, MessageEventSource, RelationInfo};
use self::{
    attachment_dialog::AttachmentDialog, completion::CompletionPopover,
    composer_parser::ComposerParser, create_poll_dialog::CreatePollDialog,
};
use super::message_row::MessageContent;
use crate::{
    Application, Window,
    components::{AvatarImageSafetySetting, CustomEntry, LabelWithWidgets, LoadingButton},
    gettext_f,
    prelude::*,
    session::{Event, Member, Room, RoomListRoomInfo, Timeline},
    spawn, spawn_tokio, toast,
    utils::{
        Location, LocationError, TemplateCallbacks, TokioDrop,
        media::{
            FileInfo,
            audio::load_audio_info,
            audio_recorder::{AudioRecorder, AudioRecorderError, MAX_RECORDING_DURATION},
            filename_for_mime,
            image::ImageInfoLoader,
            video::load_video_info,
        },
    },
};

/// A map of composer state per-session and per-timeline.
///
/// The timeline key is built from the room ID and the optional thread root
/// event ID.
type ComposerStatesMap = HashMap<Option<String>, HashMap<Option<String>, ComposerState>>;

/// The available stack pages of the [`MessageToolbar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageToolbarPage {
    /// The composer and other buttons to send messages.
    Composer,
    /// A voice message is being recorded.
    Recording,
    /// The user is not allowed to send messages in the room.
    NoPermission,
    /// The room was tombstoned.
    Tombstoned,
}

impl MessageToolbarPage {
    /// The name of this page.
    const fn name(self) -> &'static str {
        match self {
            Self::Composer => "composer",
            Self::Recording => "recording",
            Self::NoPermission => "no-permission",
            Self::Tombstoned => "tombstoned",
        }
    }

    /// Get the page matching the given name.
    ///
    /// Panics if the name does not match any variant.
    fn from_name(name: &str) -> Self {
        match name {
            "composer" => Self::Composer,
            "recording" => Self::Recording,
            "no-permission" => Self::NoPermission,
            "tombstoned" => Self::Tombstoned,
            _ => panic!("Unknown MessageToolbarPage: {name}"),
        }
    }
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_toolbar/mod.ui"
    )]
    #[properties(wrapper_type = super::MessageToolbar)]
    pub struct MessageToolbar {
        #[template_child]
        main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub(super) message_entry: TemplateChild<sourceview::View>,
        #[template_child]
        attach_button: TemplateChild<gtk::Button>,
        #[template_child]
        send_button: TemplateChild<gtk::Button>,
        #[template_child]
        record_voice_button: TemplateChild<gtk::Button>,
        #[template_child]
        send_voice_button: TemplateChild<gtk::Button>,
        #[template_child]
        recording_time_label: TemplateChild<gtk::Label>,
        #[template_child]
        recording_level_bar: TemplateChild<gtk::LevelBar>,
        #[template_child]
        related_event_header: TemplateChild<LabelWithWidgets>,
        #[template_child]
        related_event_content: TemplateChild<MessageContent>,
        #[template_child]
        tombstoned_label: TemplateChild<gtk::Label>,
        #[template_child]
        tombstoned_button: TemplateChild<LoadingButton>,
        /// The timeline used to send messages.
        #[property(get, set = Self::set_timeline, explicit_notify, nullable)]
        timeline: glib::WeakRef<Timeline>,
        successor_room_list_info: RoomListRoomInfo,
        room_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        send_message_permission_handler: RefCell<Option<glib::SignalHandlerId>>,
        /// Whether outgoing messages should be interpreted as markdown.
        #[property(get, set)]
        markdown_enabled: Cell<bool>,
        completion: CompletionPopover,
        /// The current composer state.
        #[property(get = Self::current_composer_state)]
        current_composer_state: PhantomData<ComposerState>,
        composer_state_handler: RefCell<Option<glib::SignalHandlerId>>,
        buffer_handlers: RefCell<Option<(glib::SignalHandlerId, glib::Binding)>>,
        /// The composer states, per-session and per-room.
        ///
        /// The fallback composer state has the `None` key.
        composer_states: RefCell<ComposerStatesMap>,
        /// A guard to avoid sending several messages at once.
        send_guard: Mutex<()>,
        /// The recorder of the ongoing voice message recording, if any.
        recorder: RefCell<Option<AudioRecorder>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageToolbar {
        const NAME: &'static str = "MessageToolbar";
        type Type = super::MessageToolbar;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            CustomEntry::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);

            // Menu actions.
            klass.install_action_async(
                "message-toolbar.send-location",
                None,
                |obj, _, _| async move {
                    obj.imp().send_location().await;
                },
            );

            klass.install_action_async(
                "message-toolbar.create-poll",
                None,
                |obj, _, _| async move {
                    obj.imp().create_poll().await;
                },
            );

            klass.install_property_action("message-toolbar.markdown", "markdown-enabled");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageToolbar {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("message-sent").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            // Markdown highlighting.
            let settings = Application::default().settings();
            settings
                .bind("markdown-enabled", &*obj, "markdown-enabled")
                .build();

            // Tab auto-completion.
            self.completion.set_parent(&*self.message_entry);

            // Location.
            let location = Location::new();
            obj.action_set_enabled("message-toolbar.send-location", location.is_available());

            // Listen to changes in the room list for the successor.
            self.successor_room_list_info
                .connect_is_joining_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_tombstoned_page();
                    }
                ));
            self.successor_room_list_info
                .connect_local_room_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_tombstoned_page();
                    }
                ));
        }

        fn dispose(&self) {
            self.completion.unparent();
            self.disconnect_signals();

            if let Some(recorder) = self.recorder.take() {
                recorder.cancel();
            }
        }
    }

    impl WidgetImpl for MessageToolbar {
        fn grab_focus(&self) -> bool {
            let Some(visible_page) = self
                .main_stack
                .visible_child_name()
                .map(|name| MessageToolbarPage::from_name(&name))
            else {
                return false;
            };

            match visible_page {
                MessageToolbarPage::Composer => self.message_entry.grab_focus(),
                MessageToolbarPage::Recording => self.send_voice_button.grab_focus(),
                MessageToolbarPage::NoPermission => false,
                MessageToolbarPage::Tombstoned => {
                    if self.tombstoned_button.is_visible() {
                        self.tombstoned_button.grab_focus()
                    } else {
                        false
                    }
                }
            }
        }
    }

    impl BinImpl for MessageToolbar {}

    #[gtk::template_callbacks]
    impl MessageToolbar {
        /// Set the timeline used to send messages.
        fn set_timeline(&self, timeline: Option<&Timeline>) {
            let old_timeline = self.timeline.upgrade();
            if old_timeline.as_ref() == timeline {
                return;
            }
            let obj = self.obj();

            self.disconnect_signals();

            // Discard any ongoing voice message recording, it belongs to the
            // previous timeline.
            self.discard_voice_recording();

            if let Some(timeline) = timeline {
                let room = timeline.room();

                let is_tombstoned_handler = room.connect_is_tombstoned_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_visible_page();
                    }
                ));
                let successor_id_handler = room.connect_successor_id_string_notify(clone!(
                    #[weak(rename_to= imp)]
                    self,
                    move |_| {
                        imp.update_successor_identifier();
                        imp.update_tombstoned_page();
                    }
                ));
                let successor_handler = room.connect_successor_notify(clone!(
                    #[weak(rename_to= imp)]
                    self,
                    move |_| {
                        imp.update_tombstoned_page();
                    }
                ));
                self.room_handlers.replace(vec![
                    is_tombstoned_handler,
                    successor_id_handler,
                    successor_handler,
                ]);

                let send_message_permission_handler = timeline
                    .room()
                    .permissions()
                    .connect_can_send_message_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_visible_page();
                        }
                    ));
                self.send_message_permission_handler
                    .replace(Some(send_message_permission_handler));

                if let Some(session) = room.session() {
                    self.successor_room_list_info
                        .set_room_list(session.room_list());
                }
            }

            self.completion.set_room(timeline.map(Timeline::room));
            self.timeline.set(timeline);

            self.update_successor_identifier();
            self.update_tombstoned_page();
            self.update_visible_page();

            obj.notify_timeline();
            self.update_current_composer_state(old_timeline);
        }

        /// The stack page that should be presented given the current state.
        fn visible_page(&self) -> MessageToolbarPage {
            let Some(room) = self.timeline.upgrade().map(|timeline| timeline.room()) else {
                return MessageToolbarPage::NoPermission;
            };

            if room.is_tombstoned() {
                MessageToolbarPage::Tombstoned
            } else if room.permissions().can_send_message() {
                if self.recorder.borrow().is_some() {
                    MessageToolbarPage::Recording
                } else {
                    MessageToolbarPage::Composer
                }
            } else {
                MessageToolbarPage::NoPermission
            }
        }

        /// Whether the user can compose a message.
        ///
        /// It depends on whether our own user has the permission to send a
        /// message in the current room.
        pub(super) fn can_compose_message(&self) -> bool {
            self.visible_page() == MessageToolbarPage::Composer
        }

        /// Update the visible stack page.
        fn update_visible_page(&self) {
            let page = self.visible_page();

            // If we cannot present the recording page anymore, e.g. because
            // the permission to send messages was revoked, discard the ongoing
            // recording.
            if page != MessageToolbarPage::Recording
                && let Some(recorder) = self.recorder.take()
            {
                recorder.cancel();
            }

            self.main_stack.set_visible_child_name(page.name());
        }

        /// Update the identifier to watch for the successor of the current
        /// room.
        fn update_successor_identifier(&self) {
            let successor_id = self
                .timeline
                .upgrade()
                .and_then(|timeline| timeline.room().successor_id().cloned());
            self.successor_room_list_info
                .set_identifiers(successor_id.into_iter().map(Into::into).collect());
        }

        /// Update the tombstoned stack page.
        fn update_tombstoned_page(&self) {
            let Some(room) = self.timeline.upgrade().map(|timeline| timeline.room()) else {
                return;
            };

            // A "real" successor must have the current room as a predecessor. We still want
            // to show the "View" button if it is only the room that matches the successor
            // ID.
            let has_successor_room =
                room.successor().is_some() || self.successor_room_list_info.local_room().is_some();
            let has_successor_id = room.successor_id().is_some();
            let has_successor = has_successor_room || has_successor_id;

            // Update description.
            let description = if has_successor {
                gettext("The conversation continues in a new room")
            } else {
                gettext("The conversation has ended")
            };
            self.tombstoned_label.set_label(&description);

            // Update button.
            if has_successor {
                let label = if has_successor_room {
                    // Translators: This is a verb, as in 'View Room'.
                    gettext("View")
                } else {
                    gettext("Join")
                };
                self.tombstoned_button.set_content_label(label);

                let is_joining_successor = self.successor_room_list_info.is_joining();
                self.tombstoned_button.set_is_loading(is_joining_successor);
            }
            self.tombstoned_button.set_visible(has_successor);
        }

        /// Whether the buffer of the composer is empty.
        ///
        /// Returns `true` if the buffer is empty or contains only whitespace.
        fn is_buffer_empty(&self) -> bool {
            let mut iter = self.message_entry.buffer().start_iter();

            if iter.is_end() {
                return true;
            }

            loop {
                if !iter.char().is_whitespace() {
                    // The buffer is not empty.
                    return false;
                }

                if !iter.forward_cursor_position() {
                    // We are at the end and we did not encounter a non-whitespace character, the
                    // buffer is empty.
                    return true;
                }
            }
        }

        /// The current composer state.
        fn current_composer_state(&self) -> ComposerState {
            let timeline = self.timeline.upgrade();
            self.composer_state(timeline)
        }

        /// The composer state for the given room.
        ///
        /// If the composer state does not exist, it is created.
        fn composer_state(&self, timeline: Option<Timeline>) -> ComposerState {
            let room = timeline.as_ref().map(Timeline::room);

            let timeline_key = timeline.as_ref().map(|timeline| {
                let mut key = timeline.room().room_id().to_string();

                if let Some(thread_root_id) = timeline.thread_root_id() {
                    key.push('#');
                    key.push_str(thread_root_id.as_str());
                }

                key
            });

            self.composer_states
                .borrow_mut()
                .entry(
                    room.as_ref()
                        .and_then(Room::session)
                        .map(|s| s.session_id().to_owned()),
                )
                .or_default()
                .entry(timeline_key)
                .or_insert_with(|| ComposerState::new(timeline))
                .clone()
        }

        /// Update the current composer state.
        fn update_current_composer_state(&self, old_timeline: Option<Timeline>) {
            let old_composer_state = self.composer_state(old_timeline);
            old_composer_state.attach_to_view(None);

            if let Some(handler) = self.composer_state_handler.take() {
                old_composer_state.disconnect(handler);
            }
            if let Some((handler, binding)) = self.buffer_handlers.take() {
                let prev_buffer = self.message_entry.buffer();
                prev_buffer.disconnect(handler);

                binding.unbind();
            }

            let composer_state = self.current_composer_state();
            let buffer = composer_state.buffer();
            let obj = self.obj();

            composer_state.attach_to_view(Some(&self.message_entry));

            // Actions on changes in message entry.
            let text_notify_handler = buffer.connect_text_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    let is_empty = imp.is_buffer_empty();
                    imp.send_button.set_sensitive(!is_empty);
                    imp.record_voice_button.set_visible(is_empty);
                    imp.send_typing_notification(!is_empty);
                }
            ));

            let is_empty = self.is_buffer_empty();
            self.send_button.set_sensitive(!is_empty);
            self.record_voice_button.set_visible(is_empty);

            // Markdown highlighting.
            let markdown_binding = obj
                .bind_property("markdown-enabled", &buffer, "highlight-syntax")
                .sync_create()
                .build();

            self.buffer_handlers
                .replace(Some((text_notify_handler, markdown_binding)));

            // Related event.
            let composer_state_handler = composer_state.connect_related_to_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_related_event();
                }
            ));
            self.composer_state_handler
                .replace(Some(composer_state_handler));
            self.update_related_event();

            obj.notify_current_composer_state();
        }

        /// Update the displayed related event for the current state.
        fn update_related_event(&self) {
            let composer_state = self.current_composer_state();

            match composer_state.related_to() {
                Some(RelationInfo::Reply(event)) => {
                    self.update_for_reply(&event);
                    self.enable_sending_non_text_messages(false);
                }
                Some(RelationInfo::Edit(_)) => {
                    self.update_for_edit();
                    self.enable_sending_non_text_messages(false);
                }
                None => {
                    self.enable_sending_non_text_messages(true);
                }
            }
        }

        /// Update the displayed related event for the given replied-to event.
        fn update_for_reply(&self, message_event: &MessageEventSource) {
            let Some(msgtype) = message_event.msgtype() else {
                // The event was probably redacted, we cannot reply to it anymore.
                self.clear_related_event();
                return;
            };
            let Some(timeline) = self.timeline.upgrade() else {
                return;
            };

            let room = timeline.room();
            let sender = room
                .get_or_create_members()
                .get_or_create(message_event.sender());

            let label = gettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name. In this string, 'Reply' is a noun.
                "Reply to {user}",
                &[("user", LabelWithWidgets::PLACEHOLDER)],
            );
            // We do not need to watch safety settings for mentions, rooms will be watched
            // automatically.
            let pill = sender.to_pill(AvatarImageSafetySetting::None, None);

            self.related_event_header
                .set_label_and_widgets(label, vec![pill]);

            self.related_event_content
                .update_for_related_event(&msgtype, message_event, &sender);
            self.related_event_content.set_visible(true);
        }

        /// Update the displayed related event for the given edit.
        fn update_for_edit(&self) {
            // Translators: In this string, 'Edit' is a noun.
            let label = pgettext("room-history", "Edit");
            self.related_event_header
                .set_label_and_widgets::<gtk::Widget>(label, vec![]);
            self.related_event_content.set_visible(false);
        }

        /// Toggle UI for sending non-text messages.
        fn enable_sending_non_text_messages(&self, enable: bool) {
            self.attach_button.set_sensitive(enable);
            self.record_voice_button.set_sensitive(enable);
            self.obj().action_set_enabled(
                "message-toolbar.send-location",
                enable && Location::new().is_available(),
            );
        }

        /// Clear the related event.
        #[template_callback]
        fn clear_related_event(&self) {
            self.current_composer_state().set_related_to(None);
        }

        /// Add a mention of the given member to the message composer.
        pub(super) fn mention_member(&self, member: &Member) {
            if !self.can_compose_message() {
                return;
            }

            let buffer = self.message_entry.buffer();
            let mut insert = buffer.iter_at_mark(&buffer.get_insert());

            // We do not need to watch safety settings for users.
            let pill = member.to_pill(AvatarImageSafetySetting::None, None);
            self.current_composer_state().add_widget(pill, &mut insert);

            self.message_entry.grab_focus();
        }

        /// Set the event to reply to.
        pub(super) fn set_reply_to(&self, event: Event) {
            if !self.can_compose_message() {
                return;
            }

            if event.event_id().is_none() {
                warn!("Cannot send reply for event that is not sent yet");
                return;
            }
            let Some(message_event) = MessageEventSource::from_event(event) else {
                warn!("Unsupported event type for reply");
                return;
            };

            self.current_composer_state()
                .set_related_to(Some(RelationInfo::Reply(message_event.into())));

            self.message_entry.grab_focus();
        }

        /// Set the event to edit.
        pub(super) fn set_edit(&self, event: &Event) {
            if !self.can_compose_message() {
                return;
            }

            let item = event.item();

            let Some(event_id) = item.event_id() else {
                warn!("Cannot send edit for event that is not sent yet");
                return;
            };
            let TimelineItemContent::MsgLike(msg_like) = item.content() else {
                warn!("Unsupported event type for edit");
                return;
            };
            let Some(message) = msg_like.as_message() else {
                warn!("Unsupported event type for edit");
                return;
            };

            self.current_composer_state()
                .set_edit_source(event_id.to_owned(), &message);

            self.message_entry.grab_focus();
        }

        /// Handle when a key was pressed in the message entry.
        #[template_callback]
        fn key_pressed(
            &self,
            key: gdk::Key,
            _keycode: u32,
            modifier: gdk::ModifierType,
        ) -> glib::Propagation {
            // Do not capture key press if there is a mask other than CapsLock.
            if modifier != gdk::ModifierType::NO_MODIFIER_MASK
                && modifier != gdk::ModifierType::LOCK_MASK
            {
                return glib::Propagation::Proceed;
            }

            // Send message on enter.
            if matches!(
                key,
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter,
            ) {
                spawn!(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.send_text_message().await;
                    }
                ));
                return glib::Propagation::Stop;
            }

            // Clear related event on escape.
            if key == gdk::Key::Escape && self.current_composer_state().has_relation() {
                self.clear_related_event();
                return glib::Propagation::Stop;
            }

            // Edit the last message on key up, if the composer is empty and the completion
            // popover is not open.
            if matches!(key, gdk::Key::Up | gdk::Key::KP_Up)
                && !self.completion.is_visible()
                && self.is_buffer_empty()
            {
                if self
                    .obj()
                    .activate_action("room-history.edit-latest-message", None)
                    .is_err()
                {
                    error!("Could not activate `room-history.edit-latest-message` action");
                }
                return glib::Propagation::Stop;
            }

            glib::Propagation::Proceed
        }

        /// Send the text message that is currently in the message entry.
        #[template_callback]
        async fn send_text_message(&self) {
            let Some(_send_guard) = self.send_guard.try_lock() else {
                return;
            };
            if !self.can_compose_message() {
                return;
            }
            let Some(timeline) = self.timeline.upgrade() else {
                return;
            };

            let composer_state = self.current_composer_state();
            let markdown_enabled = self.markdown_enabled.get();

            let Some(content) = ComposerParser::new(&composer_state, None)
                .into_message_event_content(markdown_enabled)
                .await
            else {
                return;
            };

            let matrix_timeline = timeline.matrix_timeline();

            // Send event depending on relation.
            match composer_state.related_to() {
                Some(RelationInfo::Reply(message_event)) => {
                    let event_id = message_event.event_id();

                    let handle =
                        spawn_tokio!(
                            async move { matrix_timeline.send_reply(content, event_id).await }
                        );

                    if let Err(error) = handle.await.expect("task was not aborted") {
                        error!("Could not send reply: {error}");
                        toast!(self.obj(), gettext("Could not send reply"));
                    }
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
                    if let Err(error) = handle.await.unwrap() {
                        error!("Could not send edit: {error}");
                        toast!(self.obj(), gettext("Could not send edit"));
                    }
                }
                _ => {
                    let handle = spawn_tokio!(async move {
                        matrix_timeline
                            .send(content.with_relation(None).into())
                            .await
                    });
                    if let Err(error) = handle.await.unwrap() {
                        error!("Could not send message: {error}");
                        toast!(self.obj(), gettext("Could not send message"));
                    }
                }
            }

            // Clear the composer state.
            composer_state.clear();

            self.obj().emit_by_name::<()>("message-sent", &[]);
        }

        /// Open the emoji chooser in the message entry.
        #[template_callback]
        fn open_emoji(&self) {
            if !self.can_compose_message() {
                return;
            }
            self.message_entry.emit_insert_emoji();
        }

        /// Send the current location of the user.
        ///
        /// Shows a preview of the location first and asks the user to confirm
        /// the action.
        async fn send_location(&self) {
            let Some(_send_guard) = self.send_guard.try_lock() else {
                return;
            };
            if !self.can_compose_message() {
                return;
            }
            let Some(timeline) = self.timeline.upgrade() else {
                return;
            };

            let location = Location::new();
            if !location.is_available() {
                return;
            }

            // Listen whether the user cancels before the location API is initialized.
            if let Err(error) = location.init().await {
                self.location_error_toast(error);
                return;
            }

            // Show the dialog as loading.
            let obj = self.obj();
            let dialog = AttachmentDialog::new(&gettext("Your Location"));
            let response_fut = dialog.response_future(&*obj);
            pin_mut!(response_fut);

            // Listen whether the user cancels before the location stream is ready.
            let location_stream_fut = location.updates_stream();
            pin_mut!(location_stream_fut);
            let (mut location_stream, response_fut) =
                match future::select(location_stream_fut, response_fut).await {
                    future::Either::Left((stream_res, response_fut)) => match stream_res {
                        Ok(stream) => (stream, response_fut),
                        Err(error) => {
                            dialog.close();
                            self.location_error_toast(error);
                            return;
                        }
                    },
                    future::Either::Right(_) => {
                        // The only possible response at this stage should be cancel.
                        return;
                    }
                };

            // Listen to location changes while waiting for the user's response.
            let mut response_fut_wrapper = Some(response_fut);
            let mut geo_uri_wrapper = None;
            loop {
                let response_fut = response_fut_wrapper.take().unwrap();

                match future::select(location_stream.next(), response_fut).await {
                    future::Either::Left((update, response_fut)) => {
                        if let Some(uri) = update {
                            dialog.set_location(&uri);
                            geo_uri_wrapper.replace(uri);
                        }
                        response_fut_wrapper.replace(response_fut);
                    }
                    future::Either::Right((response, _)) => {
                        // The linux location stream requires a tokio executor when dropped.
                        let _ = TokioDrop::new(location_stream);

                        if response == gtk::ResponseType::Ok {
                            break;
                        }

                        return;
                    }
                }
            }

            let Some(geo_uri) = geo_uri_wrapper else {
                return;
            };

            let geo_uri_string = geo_uri.to_string();
            let timestamp =
                glib::DateTime::now_local().expect("Should be able to get the local timestamp");
            let location_body = gettext_f(
                // Translators: Do NOT translate the content between '{' and '}', this is a
                // variable name.
                "User Location {geo_uri} at {iso8601_datetime}",
                &[
                    ("geo_uri", &geo_uri_string),
                    (
                        "iso8601_datetime",
                        timestamp.format_iso8601().unwrap().as_str(),
                    ),
                ],
            );

            let content = RoomMessageEventContent::new(MessageType::Location(
                LocationMessageEventContent::new(location_body, geo_uri_string),
            ))
            // To avoid triggering legacy pushrules, we must always include the mentions,
            // even if they are empty.
            .add_mentions(Mentions::default());

            let matrix_timeline = timeline.matrix_timeline();
            let handle = spawn_tokio!(async move { matrix_timeline.send(content.into()).await });

            if let Err(error) = handle.await.unwrap() {
                error!("Could not send location: {error}");
                toast!(self.obj(), gettext("Could not send location"));
            }

            self.obj().emit_by_name::<()>("message-sent", &[]);
        }

        /// Create a poll and send it.
        ///
        /// Opens a dialog to create the poll and asks the user to confirm the
        /// action.
        async fn create_poll(&self) {
            let Some(_send_guard) = self.send_guard.try_lock() else {
                return;
            };
            if !self.can_compose_message() {
                return;
            }
            let Some(timeline) = self.timeline.upgrade() else {
                return;
            };

            let obj = self.obj();
            let dialog = CreatePollDialog::new();
            let Some(content) = dialog.response_future(&*obj).await else {
                return;
            };

            let matrix_timeline = timeline.matrix_timeline();
            let handle = spawn_tokio!(async move {
                matrix_timeline
                    .send(AnyMessageLikeEventContent::UnstablePollStart(
                        content.into(),
                    ))
                    .await
            });

            if let Err(error) = handle.await.expect("task was not aborted") {
                error!("Could not send poll: {error}");
                toast!(self.obj(), gettext("Could not send poll"));
            }

            self.obj().emit_by_name::<()>("message-sent", &[]);
        }

        /// Show a toast for the given location error;
        fn location_error_toast(&self, error: LocationError) {
            let msg = match error {
                LocationError::Cancelled => gettext("The location request has been cancelled"),
                LocationError::Disabled => gettext("The location services are disabled"),
                LocationError::Other => gettext("Could not retrieve current location"),
            };

            toast!(self.obj(), msg);
        }

        /// Send the attachment with the given data.
        async fn send_attachment(
            &self,
            source: AttachmentSource,
            mime: mime::Mime,
            info: AttachmentInfo,
            thumbnail: Option<Thumbnail>,
        ) {
            let Some(timeline) = self.timeline.upgrade() else {
                return;
            };

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

            if let Err(error) = handle.await.unwrap() {
                error!("Could not send file: {error}");
                toast!(self.obj(), gettext("Could not send file"));
            }

            self.obj().emit_by_name::<()>("message-sent", &[]);
        }

        /// Start recording a voice message.
        #[template_callback]
        fn start_voice_recording(&self) {
            if !self.can_compose_message() || self.recorder.borrow().is_some() {
                return;
            }

            match AudioRecorder::start() {
                Ok(recorder) => {
                    self.recorder.replace(Some(recorder));
                    self.recording_time_label.set_label("0:00");
                    self.recording_level_bar.set_value(0.0);
                    self.update_visible_page();

                    glib::timeout_add_local(
                        Duration::from_millis(100),
                        clone!(
                            #[weak(rename_to = imp)]
                            self,
                            #[upgrade_or]
                            glib::ControlFlow::Break,
                            move || imp.voice_recording_tick()
                        ),
                    );
                }
                Err(_) => {
                    toast!(
                        self.obj(),
                        gettext(
                            "Could not start recording. Check that the microphone is accessible."
                        )
                    );
                }
            }
        }

        /// Update the recording UI periodically while a voice message is being
        /// recorded.
        fn voice_recording_tick(&self) -> glib::ControlFlow {
            let borrow = self.recorder.borrow();
            let Some(recorder) = &*borrow else {
                // The recording was sent or cancelled.
                return glib::ControlFlow::Break;
            };

            if recorder.has_error() {
                drop(borrow);
                self.discard_voice_recording();
                toast!(self.obj(), gettext("Could not record voice message"));

                return glib::ControlFlow::Break;
            }

            let elapsed = recorder.elapsed();
            let elapsed_secs = elapsed.as_secs();
            self.recording_time_label.set_label(&format!(
                "{}:{:02}",
                elapsed_secs / 60,
                elapsed_secs % 60
            ));
            self.recording_level_bar.set_value(recorder.last_peak());

            if elapsed >= MAX_RECORDING_DURATION {
                drop(borrow);

                // Stop the recording and send it when the maximum duration is
                // reached.
                spawn!(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.send_voice_recording().await;
                    }
                ));

                return glib::ControlFlow::Break;
            }

            glib::ControlFlow::Continue
        }

        /// Cancel the ongoing voice message recording, if any, and discard the
        /// recorded audio.
        #[template_callback]
        fn cancel_voice_recording(&self) {
            self.discard_voice_recording();
        }

        /// Discard the ongoing voice message recording, if any.
        fn discard_voice_recording(&self) {
            if let Some(recorder) = self.recorder.take() {
                recorder.cancel();
                self.update_visible_page();
            }
        }

        /// Stop the ongoing voice message recording and send it.
        #[template_callback]
        async fn send_voice_recording(&self) {
            let Some(_send_guard) = self.send_guard.try_lock() else {
                return;
            };
            let Some(recorder) = self.recorder.take() else {
                return;
            };
            self.update_visible_page();

            let audio = match recorder.stop().await {
                Ok(audio) => audio,
                Err(AudioRecorderError::NoData) => {
                    toast!(self.obj(), gettext("The voice message is empty"));
                    return;
                }
                Err(_) => {
                    toast!(self.obj(), gettext("Could not record voice message"));
                    return;
                }
            };

            let info = AttachmentInfo::Voice(BaseAudioInfo {
                duration: Some(audio.duration),
                size: (audio.data.len() as u64).try_into().ok(),
                waveform: Some(audio.waveform),
            });
            let source = AttachmentSource::Data {
                bytes: audio.data,
                filename: "voice-message.ogg".to_owned(),
            };
            let mime = "audio/ogg"
                .parse()
                .expect("audio/ogg should be a valid MIME type");

            self.send_attachment(source, mime, info, None).await;
        }

        /// Send the given texture as an image.
        ///
        /// Shows a preview of the image first and asks the user to confirm the
        /// action.
        async fn send_image(&self, image: gdk::Texture) {
            let Some(_send_guard) = self.send_guard.try_lock() else {
                return;
            };
            if !self.can_compose_message() {
                return;
            }

            let obj = self.obj();
            let filename = filename_for_mime(Some(mime::IMAGE_PNG.as_ref()), None);
            let dialog = AttachmentDialog::new(&filename);
            dialog.set_image(&image);

            if dialog.response_future(&*obj).await != gtk::ResponseType::Ok {
                return;
            }

            let bytes = image.save_to_png_bytes();
            let filesize = bytes.len().try_into().ok();

            let (mut base_info, thumbnail) = ImageInfoLoader::from(image)
                .load_info_and_thumbnail(filesize, &*obj)
                .await;
            base_info.size = filesize.map(Into::into);

            let info = AttachmentInfo::Image(base_info);
            let source = AttachmentSource::Data {
                bytes: bytes.to_vec(),
                filename,
            };
            self.send_attachment(source, mime::IMAGE_PNG, info, thumbnail)
                .await;
        }

        /// Select a file to send.
        #[template_callback]
        async fn select_file(&self) {
            let Some(_send_guard) = self.send_guard.try_lock() else {
                return;
            };
            if !self.can_compose_message() {
                return;
            }

            let obj = self.obj();
            let dialog = gtk::FileDialog::builder()
                .title(gettext("Select File"))
                .modal(true)
                .accept_label(gettext("Select"))
                .build();

            match dialog
                .open_future(obj.root().and_downcast_ref::<gtk::Window>())
                .await
            {
                Ok(file) => {
                    self.send_file_inner(file).await;
                }
                Err(error) => {
                    if error.matches(gtk::DialogError::Dismissed) {
                        debug!("File dialog dismissed by user");
                    } else {
                        error!("Could not open file: {error:?}");
                        toast!(obj, gettext("Could not open file"));
                    }
                }
            }
        }

        /// Send the given file.
        ///
        /// Shows a preview of the file first, if possible, and asks the user to
        /// confirm the action.
        pub(super) async fn send_file(&self, file: gio::File) {
            let Some(_send_guard) = self.send_guard.try_lock() else {
                return;
            };
            if !self.can_compose_message() {
                return;
            }

            self.send_file_inner(file).await;
        }

        async fn send_file_inner(&self, file: gio::File) {
            let obj = self.obj();

            let Some(path) = file.path() else {
                warn!("Could not read file: file does not have a path");
                toast!(obj, gettext("Error reading file"));
                return;
            };

            let file_info = match FileInfo::try_from_file(&file).await {
                Ok(file_info) => file_info,
                Err(error) => {
                    warn!("Could not read file info: {error}");
                    toast!(obj, gettext("Error reading file"));
                    return;
                }
            };

            let dialog = AttachmentDialog::new(&file_info.filename);
            dialog.set_file(file.clone());

            if dialog.response_future(&*obj).await != gtk::ResponseType::Ok {
                return;
            }

            let size = file_info.size.map(Into::into);
            let (info, thumbnail) = match file_info.mime.type_() {
                mime::IMAGE => {
                    let (mut info, thumbnail) = ImageInfoLoader::from(file)
                        .load_info_and_thumbnail(file_info.size, &*obj)
                        .await;
                    info.size = size;

                    (AttachmentInfo::Image(info), thumbnail)
                }
                mime::VIDEO => {
                    let (mut info, thumbnail) = load_video_info(&file, &*obj).await;
                    info.size = size;
                    (AttachmentInfo::Video(info), thumbnail)
                }
                mime::AUDIO => {
                    let mut info = load_audio_info(&file).await;
                    info.size = size;
                    (AttachmentInfo::Audio(info), None)
                }
                _ => (AttachmentInfo::File(BaseFileInfo { size }), None),
            };

            self.send_attachment(path.into(), file_info.mime, info, thumbnail)
                .await;
        }

        /// Read the file data from the clipboard and send it.
        pub(super) async fn read_clipboard_file(&self) {
            let obj = self.obj();
            let clipboard = obj.clipboard();
            let formats = clipboard.formats();

            if formats.contains_type(gdk::Texture::static_type()) {
                // There is an image in the clipboard.
                match clipboard
                    .read_value_future(gdk::Texture::static_type(), glib::Priority::DEFAULT)
                    .await
                {
                    Ok(value) => match value.get::<gdk::Texture>() {
                        Ok(texture) => {
                            self.send_image(texture).await;
                            return;
                        }
                        Err(error) => warn!("Could not get GdkTexture from value: {error}"),
                    },
                    Err(error) => warn!("Could not get GdkTexture from the clipboard: {error}"),
                }

                toast!(obj, gettext("Error getting image from clipboard"));
            } else if formats.contains_type(gio::File::static_type()) {
                // There is a file in the clipboard.
                match clipboard
                    .read_value_future(gio::File::static_type(), glib::Priority::DEFAULT)
                    .await
                {
                    Ok(value) => match value.get::<gio::File>() {
                        Ok(file) => {
                            self.send_file(file).await;
                            return;
                        }
                        Err(error) => warn!("Could not get file from value: {error}"),
                    },
                    Err(error) => warn!("Could not get file from the clipboard: {error}"),
                }

                toast!(obj, gettext("Error getting file from clipboard"));
            }
        }

        /// Handle a click on the related event.
        ///
        /// Scrolls to the corresponding event.
        #[template_callback]
        fn handle_related_event_click(&self) {
            if let Some(related_to) = self.current_composer_state().related_to()
                && self
                    .obj()
                    .activate_action(
                        "room-history.scroll-to-event",
                        Some(&TimelineEventItemId::EventId(related_to.event_id()).to_variant()),
                    )
                    .is_err()
            {
                error!("Could not activate `room-history.scroll-to-event` action");
            }
        }

        /// Paste the content of the clipboard into the message entry.
        #[template_callback]
        fn paste_from_clipboard(&self) {
            if !self.can_compose_message() {
                return;
            }

            let formats = self.obj().clipboard().formats();

            // We only handle files and supported images.
            if formats.contains_type(gio::File::static_type())
                || formats.contains_type(gdk::Texture::static_type())
            {
                self.message_entry
                    .stop_signal_emission_by_name("paste-clipboard");
                spawn!(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.read_clipboard_file().await;
                    }
                ));
            }
        }

        /// Copy the content of the message entry to the clipboard.
        #[template_callback]
        fn copy_to_clipboard(&self) {
            self.message_entry
                .stop_signal_emission_by_name("copy-clipboard");
            self.copy_buffer_selection_to_clipboard();
        }

        /// Cut the content of the message entry to the clipboard.
        #[template_callback]
        fn cut_to_clipboard(&self) {
            self.message_entry
                .stop_signal_emission_by_name("cut-clipboard");
            self.copy_buffer_selection_to_clipboard();
            self.message_entry.buffer().delete_selection(true, true);
        }

        // Copy the selection in the message entry to the clipboard while replacing
        // mentions.
        fn copy_buffer_selection_to_clipboard(&self) {
            let buffer = self.message_entry.buffer();
            let Some((start, end)) = buffer.selection_bounds() else {
                return;
            };

            let composer_state = self.current_composer_state();
            let body = ComposerParser::new(&composer_state, Some((start, end))).into_plain_text();

            self.obj().clipboard().set_text(&body);
        }

        /// Send a typing notification for the given typing state.
        fn send_typing_notification(&self, typing: bool) {
            let Some(timeline) = self.timeline.upgrade() else {
                return;
            };
            let room = timeline.room();

            let Some(session) = room.session() else {
                return;
            };

            if !session.settings().typing_enabled() {
                return;
            }

            room.send_typing_notification(typing);
        }

        /// Join or view the successor of the room, if possible.
        #[template_callback]
        async fn join_or_view_successor(&self) {
            let Some(room) = self.timeline.upgrade().map(|timeline| timeline.room()) else {
                return;
            };
            let Some(session) = room.session() else {
                return;
            };

            if !room.is_tombstoned() {
                return;
            }
            let obj = self.obj();

            if let Some(successor) = room
                .successor()
                .or_else(|| self.successor_room_list_info.local_room())
            {
                let Some(window) = obj.root().and_downcast::<Window>() else {
                    return;
                };

                window.session_view().select_room(successor);
            } else if let Some(successor_id) = room.successor_id().cloned() {
                // Route the successor room ID via the server of the sender of the tombstone
                // event, which is likely to know the room.
                let matrix_room = room.matrix_room().clone();
                let tombstone_event = spawn_tokio!(async move {
                    matrix_room
                        .get_state_event_static::<RoomTombstoneEventContent>()
                        .await
                })
                .await
                .expect("task was not aborted")
                .ok()
                .flatten();

                let via = tombstone_event
                    .and_then(|raw_event| raw_event.deserialize().ok())
                    .map(|event| event.sender().server_name().to_owned())
                    .into_iter()
                    .collect();

                if let Err(error) = session
                    .room_list()
                    .join_by_id_or_alias(successor_id.into(), via)
                    .await
                {
                    toast!(obj, error);
                }
            }
        }

        /// Disconnect the signal handlers of this toolbar.
        fn disconnect_signals(&self) {
            if let Some(timeline) = self.timeline.upgrade() {
                let room = timeline.room();

                for handler in self.room_handlers.take() {
                    room.disconnect(handler);
                }

                if let Some(handler) = self.send_message_permission_handler.take() {
                    room.permissions().disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A toolbar with different actions to send messages.
    pub struct MessageToolbar(ObjectSubclass<imp::MessageToolbar>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageToolbar {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Add a mention of the given member to the message composer.
    pub(crate) fn mention_member(&self, member: &Member) {
        self.imp().mention_member(member);
    }

    /// Set the event to reply to.
    pub(crate) fn set_reply_to(&self, event: Event) {
        self.imp().set_reply_to(event);
    }

    /// Set the event to edit.
    pub(crate) fn set_edit(&self, event: &Event) {
        self.imp().set_edit(event);
    }

    /// Send the given file.
    ///
    /// Shows a preview of the file first, if possible, and asks the user to
    /// confirm the action.
    pub(crate) async fn send_file(&self, file: gio::File) {
        self.imp().send_file(file).await;
    }

    /// Handle a paste action.
    pub(crate) fn handle_paste_action(&self) {
        let imp = self.imp();

        if !imp.can_compose_message() {
            return;
        }

        spawn!(clone!(
            #[weak]
            imp,
            async move {
                imp.read_clipboard_file().await;
            }
        ));
    }

    /// Connect to the signal emitted when a message was sent.
    pub(crate) fn connect_message_sent<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "message-sent",
            false,
            glib::closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
