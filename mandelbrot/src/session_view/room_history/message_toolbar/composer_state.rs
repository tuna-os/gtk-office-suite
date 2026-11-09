use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk::{ComposerDraft, ComposerDraftType, deserialized_responses::TimelineEvent};
use matrix_sdk_ui::timeline::Message;
use ruma::{
    OwnedEventId, OwnedUserId, RoomOrAliasId, UserId,
    events::{
        AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
        room::message::{MessageFormat, MessageType, OriginalSyncRoomMessageEvent},
    },
};
use sourceview::prelude::*;
use tracing::{error, warn};

use super::ComposerParser;
use crate::{
    components::{AvatarImageSafetySetting, Pill, PillSource},
    session::{Event, Member, Room, Timeline},
    spawn, spawn_tokio,
    utils::matrix::{AT_ROOM, find_at_room, find_html_mentions},
};

// The duration in seconds we wait for before saving a change.
const SAVING_TIMEOUT: u32 = 3;
/// The start tag to represent a mention in a serialized draft.
pub(super) const MENTION_START_TAG: &str = "<org.gnome.fractal.mention>";
/// The end tag to represent a mention in a serialized draft.
pub(super) const MENTION_END_TAG: &str = "</org.gnome.fractal.mention>";

mod imp {
    use std::{cell::RefCell, marker::PhantomData, sync::LazyLock};

    use futures_util::lock::Mutex;
    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ComposerState)]
    pub struct ComposerState {
        /// The room associated with this state.
        #[property(get, construct_only, nullable)]
        room: glib::WeakRef<Room>,
        /// The ID of the thread root, if this state is associated with a
        /// timeline focused on a thread.
        pub(super) thread_root_id: RefCell<Option<OwnedEventId>>,
        /// The buffer of this state.
        #[property(get)]
        buffer: sourceview::Buffer,
        /// The relation of this state.
        related_to: RefCell<Option<RelationInfo>>,
        /// Whether this state has a relation.
        #[property(get = Self::has_relation)]
        has_relation: PhantomData<bool>,
        /// The widgets of this state.
        ///
        /// These are the widgets inserted in the composer.
        widgets: RefCell<Vec<(gtk::Widget, gtk::TextChildAnchor)>>,
        /// The current view attached to this state.
        view: glib::WeakRef<sourceview::View>,
        /// The draft that was saved in the store.
        saved_draft: RefCell<Option<ComposerDraft>>,
        /// The signal handler for the current draft saving timeout.
        draft_timeout: RefCell<Option<glib::SourceId>>,
        /// The lock to prevent multiple draft saving operations at the same
        /// time.
        draft_lock: Mutex<()>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ComposerState {
        const NAME: &'static str = "ContentComposerState";
        type Type = super::ComposerState;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ComposerState {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("related-to-changed").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            crate::utils::sourceview::setup_style_scheme(&self.buffer);

            // Markdown highlighting.
            let md_lang = sourceview::LanguageManager::default().language("markdown");
            self.buffer.set_language(md_lang.as_ref());

            self.buffer.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_widgets();
                    imp.trigger_draft_saving();
                }
            ));
        }
    }

    impl ComposerState {
        /// Attach this state to the given view.
        pub(super) fn attach_to_view(&self, view: Option<&sourceview::View>) {
            self.view.set(view);

            if let Some(view) = view {
                view.set_buffer(Some(&self.buffer));

                self.update_widgets();

                for (widget, anchor) in &*self.widgets.borrow() {
                    view.add_child_at_anchor(widget, anchor);
                }
            }
        }

        /// The relation to send with the current message.
        pub(super) fn related_to(&self) -> Option<RelationInfo> {
            self.related_to.borrow().clone()
        }

        /// Set the relation to send with the current message.
        pub(super) fn set_related_to(&self, related_to: Option<RelationInfo>) {
            let had_relation = self.has_relation();

            if self
                .related_to
                .borrow()
                .as_ref()
                .is_some_and(|r| matches!(r, RelationInfo::Edit(_)))
            {
                // The user aborted the edit or the edit is done, clean up the entry.
                self.buffer.set_text("");
            }

            self.related_to.replace(related_to);

            let obj = self.obj();
            if self.has_relation() != had_relation {
                obj.notify_has_relation();
            }

            obj.emit_by_name::<()>("related-to-changed", &[]);
            self.trigger_draft_saving();
        }

        /// Whether this state has a relation.
        fn has_relation(&self) -> bool {
            self.related_to.borrow().is_some()
        }

        /// Update the list of widgets present in the composer.
        pub(super) fn update_widgets(&self) {
            self.widgets
                .borrow_mut()
                .retain(|(_w, anchor)| !anchor.is_deleted());
        }

        /// Get the draft for the current state.
        ///
        /// Returns `None` if the draft would be empty.
        fn draft(&self) -> Option<ComposerDraft> {
            ComposerParser::new(&self.obj(), None).into_composer_draft()
        }

        /// Trigger the timeout for saving the current draft.
        pub(super) fn trigger_draft_saving(&self) {
            if self.draft_timeout.borrow().is_some() {
                return;
            }

            let draft = self.draft();
            if *self.saved_draft.borrow() == draft {
                return;
            }

            let timeout = glib::timeout_add_seconds_local_once(
                SAVING_TIMEOUT,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move || {
                        imp.draft_timeout.take();
                        let obj = imp.obj().clone();

                        spawn!(glib::Priority::DEFAULT_IDLE, async move {
                            obj.imp().save_draft().await;
                        });
                    }
                ),
            );
            self.draft_timeout.replace(Some(timeout));
        }

        /// Save the current draft.
        async fn save_draft(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Some(_lock) = self.draft_lock.try_lock() else {
                // The previous saving operation is still ongoing, try saving again later.
                self.trigger_draft_saving();
                return;
            };

            let draft = self.draft();
            if *self.saved_draft.borrow() == draft {
                // Nothing to do.
                return;
            }

            let matrix_room = room.matrix_room().clone();
            let thread_root_id = self.thread_root_id.borrow().clone();
            let draft_clone = draft.clone();
            let handle = spawn_tokio!(async move {
                if let Some(draft) = draft_clone {
                    matrix_room
                        .save_composer_draft(draft, thread_root_id.as_deref())
                        .await
                } else {
                    matrix_room
                        .clear_composer_draft(thread_root_id.as_deref())
                        .await
                }
            });

            match handle.await.unwrap() {
                Ok(()) => {
                    self.saved_draft.replace(draft);
                }
                Err(error) => {
                    error!("Could not save composer draft: {error}");
                }
            }
        }

        /// Add the given widget at the position of the given iter to this
        /// state.
        pub(super) fn add_widget(&self, widget: gtk::Widget, iter: &mut gtk::TextIter) {
            let Some(view) = self.view.upgrade() else {
                return;
            };

            // Reuse the child anchor at the iter if it does not have a child widget,
            // otherwise create a new one.
            let anchor = iter
                .child_anchor()
                .filter(|anchor| self.widget_at_anchor(anchor).is_none())
                .unwrap_or_else(|| self.buffer.create_child_anchor(iter));

            view.add_child_at_anchor(&widget, &anchor);
            self.widgets.borrow_mut().push((widget, anchor));
        }

        /// Get the widget at the given anchor, if any.
        pub(super) fn widget_at_anchor(
            &self,
            anchor: &gtk::TextChildAnchor,
        ) -> Option<gtk::Widget> {
            self.widgets
                .borrow()
                .iter()
                .find(|(_, a)| a == anchor)
                .map(|(w, _)| w.clone())
        }

        /// Restore the state from the persisted draft.
        pub(super) async fn restore_draft(&self, timeline: &Timeline) {
            let matrix_room = timeline.room().matrix_room().clone();
            let thread_root_id = self.thread_root_id.borrow().clone();
            let handle = spawn_tokio!(async move {
                matrix_room
                    .load_composer_draft(thread_root_id.as_deref())
                    .await
            });

            match handle.await.expect("task was not aborted") {
                Ok(Some(draft)) => self.restore_from_draft(timeline, draft).await,
                Ok(None) => {}
                Err(error) => {
                    error!("Could not restore draft: {error}");
                }
            }
        }

        /// Restore the state from the given draft.
        async fn restore_from_draft(&self, timeline: &Timeline, draft: ComposerDraft) {
            let room = timeline.room();

            // Restore the relation.
            self.restore_related_to_from_draft(&room, draft.draft_type.clone())
                .await;

            // Make sure we start from an empty state.
            self.buffer.set_text("");
            self.widgets.borrow_mut().clear();

            // Fill the buffer while inserting mentions.
            let text = &draft.plain_text;
            let mut end_iter = self.buffer.end_iter();
            let mut pos = 0;

            while let Some(rel_start) = text[pos..].find(MENTION_START_TAG) {
                let start = pos + rel_start;
                let content_start = start + MENTION_START_TAG.len();

                let Some(rel_content_end) = text[content_start..].find(MENTION_END_TAG) else {
                    // Abort parsing.
                    error!("Could not find end tag for mention in serialized draft");
                    break;
                };
                let content_end = content_start + rel_content_end;

                if start != pos {
                    self.buffer.insert(&mut end_iter, &text[pos..start]);
                }

                match DraftMention::new(&room, &text[content_start..content_end]) {
                    DraftMention::Source(source) => {
                        // We do not need to watch safety settings for mentions, rooms will be
                        // watched automatically.
                        let pill = Pill::new(&source, AvatarImageSafetySetting::None, None);
                        self.add_widget(pill.upcast(), &mut end_iter);
                    }
                    DraftMention::Text(s) => {
                        self.buffer.insert(&mut end_iter, s);
                    }
                }

                pos = content_end + MENTION_END_TAG.len();
            }

            if pos != text.len() {
                self.buffer.insert(&mut end_iter, &text[pos..]);
            }

            self.saved_draft.replace(Some(draft));
        }

        /// Restore the relation from the given draft content.
        async fn restore_related_to_from_draft(&self, room: &Room, draft_type: ComposerDraftType) {
            let related_to = RelationInfo::from_draft(room, draft_type).await;
            self.related_to.replace(related_to);

            let obj = self.obj();
            obj.emit_by_name::<()>("related-to-changed", &[]);
            obj.notify_has_relation();
        }

        /// Update the buffer for the given edit source.
        pub(super) fn set_edit_source(&self, event_id: OwnedEventId, message: &Message) {
            let Some(room) = self.room.upgrade() else {
                return;
            };

            // We don't support editing non-text messages.
            let (text, formatted) = match message.msgtype() {
                MessageType::Emote(emote) => {
                    (format!("/me {}", emote.body), emote.formatted.clone())
                }
                MessageType::Text(text) => (text.body.clone(), text.formatted.clone()),
                _ => return,
            };

            self.set_related_to(Some(RelationInfo::Edit(event_id)));

            // Try to detect rich mentions.
            let mut mentions = if let Some(html) =
                formatted.and_then(|f| (f.format == MessageFormat::Html).then_some(f.body))
            {
                let mentions = find_html_mentions(&html, &room);
                let mut pos = 0;
                // This is looking for the mention link's inner text in the Markdown
                // so it is not super reliable: if there is other text that matches
                // a user's display name in the string it might be replaced instead
                // of the actual mention.
                // Short of an HTML to Markdown converter, it won't be a simple task
                // to locate mentions in Markdown.
                mentions
                    .into_iter()
                    .filter_map(|(pill, s)| {
                        text[pos..].find(s.as_ref()).map(|index| {
                            let start = pos + index;
                            let end = start + s.len();
                            pos = end;
                            DetectedMention { pill, start, end }
                        })
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            // Try to detect `@room` mentions.
            let can_contain_at_room = message.mentions().is_none_or(|m| m.room);
            if room.permissions().can_notify_room()
                && can_contain_at_room
                && let Some(start) = find_at_room(&text)
            {
                // We do not need to watch safety settings for at-room mentions, our own member
                // is in the room.
                let pill = Pill::new(&room.at_room(), AvatarImageSafetySetting::None, None);
                let end = start + AT_ROOM.len();
                mentions.push(DetectedMention { pill, start, end });

                // Make sure the list is sorted.
                mentions.sort_by_key(|mention| mention.start);
            }

            if mentions.is_empty() {
                self.buffer.set_text(&text);
            } else {
                // Place the pills instead of the text at the appropriate places in
                // the GtkSourceView.
                self.buffer.set_text("");

                let mut pos = 0;
                let mut iter = self.buffer.iter_at_offset(0);

                for DetectedMention { pill, start, end } in mentions {
                    if pos != start {
                        self.buffer.insert(&mut iter, &text[pos..start]);
                    }

                    self.add_widget(pill.upcast(), &mut iter);

                    pos = end;
                }

                if pos != text.len() {
                    self.buffer.insert(&mut iter, &text[pos..]);
                }
            }

            self.trigger_draft_saving();
        }

        /// Clear this state.
        pub(super) fn clear(&self) {
            self.set_related_to(None);

            self.buffer.set_text("");
            self.widgets.borrow_mut().clear();
        }
    }
}

glib::wrapper! {
    /// The composer state for a room.
    ///
    /// This allows to save and restore the composer state between room changes.
    /// It keeps track of the related event and restores the state of the composer's `GtkSourceView`.
    pub struct ComposerState(ObjectSubclass<imp::ComposerState>);
}

impl ComposerState {
    /// Create a new empty `ComposerState` for the room of the given timeline.
    pub fn new(timeline: Option<Timeline>) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("room", timeline.as_ref().map(Timeline::room))
            .build();

        obj.imp()
            .thread_root_id
            .replace(timeline.as_ref().and_then(Timeline::thread_root_id));

        if let Some(timeline) = timeline {
            let imp = obj.imp();
            spawn!(clone!(
                #[weak]
                imp,
                async move {
                    imp.restore_draft(&timeline).await;
                }
            ));
        }

        obj
    }

    /// Attach this state to the given view.
    pub(crate) fn attach_to_view(&self, view: Option<&sourceview::View>) {
        self.imp().attach_to_view(view);
    }

    /// Clear this state.
    pub(crate) fn clear(&self) {
        self.imp().clear();
    }

    /// The relation to send with the current message.
    pub(crate) fn related_to(&self) -> Option<RelationInfo> {
        self.imp().related_to()
    }

    /// Set the relation to send with the current message.
    pub(crate) fn set_related_to(&self, related_to: Option<RelationInfo>) {
        self.imp().set_related_to(related_to);
    }

    /// Update the buffer for the given edit source.
    pub(crate) fn set_edit_source(&self, event_id: OwnedEventId, message: &Message) {
        self.imp().set_edit_source(event_id, message);
    }

    /// Add the given widget at the position of the given iter to this state.
    pub(crate) fn add_widget(&self, widget: impl IsA<gtk::Widget>, iter: &mut gtk::TextIter) {
        self.imp().add_widget(widget.upcast(), iter);
    }

    /// Get the widget at the given anchor, if any.
    pub(crate) fn widget_at_anchor(&self, anchor: &gtk::TextChildAnchor) -> Option<gtk::Widget> {
        self.imp().widget_at_anchor(anchor)
    }

    /// Connect to the signal emitted when the relation changed.
    pub fn connect_related_to_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "related-to-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

/// The possible relations to send with a message.
#[derive(Debug, Clone)]
pub(crate) enum RelationInfo {
    /// Send a reply to the given event.
    Reply(Box<MessageEventSource>),

    /// Send an edit to the event with the given ID.
    Edit(OwnedEventId),
}

impl RelationInfo {
    /// Construct a relation info from a composer draft, if possible.
    pub(crate) async fn from_draft(room: &Room, draft_type: ComposerDraftType) -> Option<Self> {
        match draft_type {
            ComposerDraftType::NewMessage => None,
            ComposerDraftType::Reply { event_id } => {
                // We need to fetch the event and extract its content, so we can display it.
                let matrix_room = room.matrix_room().clone();
                let event_id_clone = event_id.clone();

                let handle = spawn_tokio!(async move {
                    matrix_room.load_or_fetch_event(&event_id_clone, None).await
                });

                let event = match handle.await.expect("task was not aborted") {
                    Ok(event) => event,
                    Err(error) => {
                        warn!("Could not fetch replied-to event content of draft: {error}");
                        return None;
                    }
                };

                // We only reply to messages.
                let Some(message_event) = MessageEventSource::from_original_event(&event) else {
                    warn!("Could not fetch replied-to event content of draft: unsupported event");
                    return None;
                };

                Some(RelationInfo::Reply(message_event.into()))
            }
            ComposerDraftType::Edit { event_id } => Some(RelationInfo::Edit(event_id)),
        }
    }

    /// The unique global identifier of the related event.
    pub(crate) fn event_id(&self) -> OwnedEventId {
        match self {
            RelationInfo::Reply(message_event) => message_event.event_id(),
            RelationInfo::Edit(event_id) => event_id.clone(),
        }
    }

    /// Get this `RelationInfo` as a draft type.
    pub(crate) fn as_draft_type(&self) -> ComposerDraftType {
        match self {
            Self::Reply(message_event) => ComposerDraftType::Reply {
                event_id: message_event.event_id(),
            },
            Self::Edit(event_id) => ComposerDraftType::Edit {
                event_id: event_id.clone(),
            },
        }
    }
}

/// A mention that was serialized in a draft.
///
/// If we managed to restore the mention, this is a `PillSource`, otherwise it's
/// the text of the mention.
enum DraftMention<'a> {
    /// The source of the mention.
    Source(PillSource),
    /// The text of the mention.
    Text(&'a str),
}

impl<'a> DraftMention<'a> {
    /// Construct a `MentionContent` from the given string in the given room.
    fn new(room: &Room, s: &'a str) -> Self {
        if s == AT_ROOM {
            Self::Source(room.at_room().upcast())
        } else if s.starts_with('@') {
            // This is a user mention.
            match UserId::parse(s) {
                Ok(user_id) => {
                    let member = Member::new(room, user_id);
                    member.update();
                    Self::Source(member.upcast())
                }
                Err(error) => {
                    error!("Could not parse user ID `{s}` from serialized mention: {error}");
                    Self::Text(s)
                }
            }
        } else {
            // It should be a room mention.
            let Some(session) = room.session() else {
                return Self::Text(s);
            };
            let room_list = session.room_list();

            match RoomOrAliasId::parse(s) {
                Ok(identifier) => {
                    if let Some(room) = room_list.get_by_identifier(&identifier) {
                        Self::Source(room.upcast())
                    } else {
                        warn!("Could not find room `{s}` from serialized mention");
                        Self::Text(s)
                    }
                }
                Err(error) => {
                    error!(
                        "Could not parse room identifier `{s}` from serialized mention: {error}"
                    );
                    Self::Text(s)
                }
            }
        }
    }
}

/// A mention that was detected in a message.
struct DetectedMention {
    /// The pill to represent the mention.
    pill: Pill,
    /// The start of the mention in the text.
    start: usize,
    /// The end of the mention in the text.
    end: usize,
}

/// The possible sources of a message event.
#[derive(Debug, Clone)]
pub(crate) enum MessageEventSource {
    /// An original event.
    OriginalEvent(Box<OriginalSyncRoomMessageEvent>),
    /// An [`Event`].
    Event(Event),
}

impl MessageEventSource {
    /// Try to construct a `MessageEventSource` from the given
    /// [`TimelineEvent`].
    ///
    /// Returns `None` if the event is not a message.
    fn from_original_event(event: &TimelineEvent) -> Option<Self> {
        let event = event.raw().deserialize().ok()?;
        match event {
            AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
                SyncMessageLikeEvent::Original(message_event),
            )) => Some(Self::OriginalEvent(message_event.into())),
            _ => None,
        }
    }

    /// Try to construct a `MessageEventSource` from the given [`Event`].
    ///
    /// Returns `None` if the event is not a message.
    pub(crate) fn from_event(event: Event) -> Option<Self> {
        (event.can_be_replied_to()).then_some(Self::Event(event))
    }

    /// The ID of the underlying event.
    pub(crate) fn event_id(&self) -> OwnedEventId {
        match self {
            Self::OriginalEvent(event) => event.event_id.clone(),
            Self::Event(event) => event
                .event_id()
                .expect("replied-to event should always have an event ID"),
        }
    }

    /// The ID of the sender of the event.
    pub(crate) fn sender(&self) -> OwnedUserId {
        match self {
            Self::OriginalEvent(event) => event.sender.clone(),
            Self::Event(event) => event.sender_id(),
        }
    }

    /// The message content of the event.
    ///
    /// Returns `None` if the event was redacted after being selected to be
    /// replied to.
    pub(crate) fn msgtype(&self) -> Option<MessageType> {
        match self {
            Self::OriginalEvent(event) => Some(event.content.msgtype.clone()),
            Self::Event(event) => Some(event.message()?.msgtype().clone()),
        }
    }

    /// Whether the message was edited.
    pub(crate) fn is_edited(&self) -> bool {
        match self {
            Self::OriginalEvent(event) => event.unsigned.relations.has_replacement(),
            Self::Event(event) => event.is_edited(),
        }
    }

    /// Whether this message might contain an `@room` mention.
    pub(crate) fn can_contain_at_room(&self) -> bool {
        match self {
            Self::OriginalEvent(event) => event
                .content
                .mentions
                .as_ref()
                .is_none_or(|mentions| mentions.room),
            Self::Event(event) => event.can_contain_at_room(),
        }
    }
}
