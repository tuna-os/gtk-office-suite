use std::sync::Arc;

use gtk::{gio, glib, glib::closure_local, prelude::*, subclass::prelude::*};
use indexmap::IndexMap;
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange, EmbeddedEvent, Error as TimelineError, EventSendState,
    EventTimelineItem, MembershipChange, Message, MsgLikeKind, PollState, TimelineDetails,
    TimelineEventItemId, TimelineItemContent,
};
use ruma::{
    MatrixToUri, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedTransactionId, OwnedUserId, UserId,
    events::{AnySyncTimelineEvent, TimelineEventType, receipt::Receipt},
    serde::Raw,
};
use serde::{Deserialize, de::IgnoredAny};
use tracing::{debug, error};

mod reaction_group;
mod reaction_list;

pub(crate) use self::{
    reaction_group::{ReactionData, ReactionGroup},
    reaction_list::ReactionList,
};
use super::{Timeline, TimelineItem, TimelineItemImpl};
use crate::{
    prelude::*,
    session::Member,
    spawn_tokio,
    utils::matrix::{MediaMessage, raw_eq, timestamp_to_date},
};

/// The possible states of a message.
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "MessageState")]
pub enum MessageState {
    /// The message has no particular state.
    #[default]
    None,
    /// The message is being sent.
    Sending,
    /// A transient error occurred when sending the message.
    ///
    /// The user can try to send it again.
    RecoverableError,
    /// A permanent error occurred when sending the message.
    ///
    /// The message can only be cancelled.
    PermanentError,
    /// The message was edited.
    Edited,
}

/// The read receipt of a user.
#[derive(Clone, Debug)]
pub(crate) struct UserReadReceipt {
    /// The ID of the user.
    pub(crate) user_id: OwnedUserId,
    /// The data of the receipt.
    pub(crate) receipt: Receipt,
}

/// The state of the header of an event in the room history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "EventHeaderState")]
pub enum EventHeaderState {
    /// The full header is displayed, with an avatar, a sender name and the
    /// timestamp.
    #[default]
    Full,
    /// Only the timestamp is displayed.
    TimestampOnly,
    /// The header is hidden.
    Hidden,
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::Event)]
    pub struct Event {
        /// The underlying SDK timeline item.
        item: RefCell<Option<Arc<EventTimelineItem>>>,
        /// The global permanent ID of this event, if it has been received from
        /// the server, as a string.
        #[property(get = Self::event_id_string)]
        event_id_string: PhantomData<Option<String>>,
        /// The ID of the sender of this event, as a string.
        #[property(get = Self::sender_id_string)]
        sender_id_string: PhantomData<String>,
        /// The timestamp of this event, as a `GDateTime`.
        #[property(get = Self::timestamp)]
        timestamp: PhantomData<glib::DateTime>,
        /// The formatted timestamp of this event.
        #[property(get = Self::formatted_timestamp)]
        formatted_timestamp: PhantomData<String>,
        /// The pretty-formatted JSON source, if it has been echoed back by the
        /// server.
        #[property(get = Self::source)]
        source: PhantomData<Option<String>>,
        /// Whether we have the JSON source of this event.
        #[property(get = Self::has_source)]
        has_source: PhantomData<bool>,
        /// The state of this event.
        #[property(get, builder(MessageState::default()))]
        state: Cell<MessageState>,
        /// Whether this event was edited.
        #[property(get = Self::is_edited)]
        is_edited: PhantomData<bool>,
        /// The pretty-formatted JSON source for the latest edit of this
        /// event.
        ///
        /// This string is empty if the event is not edited.
        #[property(get = Self::latest_edit_source)]
        latest_edit_source: PhantomData<String>,
        /// The ID for the latest edit of this event, as a string.
        ///
        /// This string is empty if the event is not edited.
        #[property(get = Self::latest_edit_event_id_string)]
        latest_edit_event_id_string: PhantomData<String>,
        /// The timestamp for the latest edit of this event, as a `GDateTime`,
        /// if any.
        #[property(get = Self::latest_edit_timestamp)]
        latest_edit_timestamp: PhantomData<Option<glib::DateTime>>,
        /// The formatted timestamp for the latest edit of this event.
        ///
        /// This string is empty if the event is not edited.
        #[property(get = Self::latest_edit_formatted_timestamp)]
        latest_edit_formatted_timestamp: PhantomData<String>,
        /// Whether this event should be highlighted.
        #[property(get = Self::is_highlighted)]
        is_highlighted: PhantomData<bool>,
        /// The reactions on this event.
        #[property(get)]
        reactions: ReactionList,
        /// The read receipts on this event.
        #[property(get = Self::read_receipts_owned)]
        read_receipts: OnceCell<gio::ListStore>,
        /// Whether this event has any read receipt.
        #[property(get = Self::has_read_receipts)]
        has_read_receipts: PhantomData<bool>,
        /// The state of the header of the event in the room history.
        #[property(get, set = Self::set_header_state, explicit_notify, builder(EventHeaderState::default()))]
        header_state: Cell<EventHeaderState>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Event {
        const NAME: &'static str = "RoomEvent";
        type Type = super::Event;
        type ParentType = TimelineItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Event {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("item-changed").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            if let Some(session) = self.obj().room().session() {
                self.reactions.set_user(session.user().clone());
            }
        }
    }

    impl TimelineItemImpl for Event {}

    impl Event {
        /// Set the underlying SDK timeline item.
        pub(super) fn set_item(&self, item: EventTimelineItem) {
            let obj = self.obj();

            let item = Arc::new(item);
            let prev_item = self.item.replace(Some(item.clone()));

            self.reactions.update(item.content().reactions());
            self.update_read_receipts(item.read_receipts());

            let prev_source = prev_item.as_ref().and_then(|i| i.original_json());
            let source = item.original_json();
            if !raw_eq(prev_source, source) {
                obj.notify_source();
            }
            if prev_source.is_some() != source.is_some() {
                obj.notify_has_source();
            }

            if prev_item.as_ref().and_then(|i| i.event_id()) != item.event_id() {
                obj.notify_event_id_string();
            }
            if prev_item
                .as_ref()
                .is_some_and(|i| i.content().is_edited() != item.content().is_edited())
            {
                obj.notify_is_edited();
            }
            if prev_item
                .as_ref()
                .is_some_and(|i| i.is_highlighted() != item.is_highlighted())
            {
                obj.notify_is_highlighted();
            }
            if !raw_eq(
                prev_item
                    .as_ref()
                    .and_then(|i| i.latest_edit_raw())
                    .as_ref(),
                item.latest_edit_raw().as_ref(),
            ) {
                obj.notify_latest_edit_source();
                obj.notify_latest_edit_event_id_string();
                obj.notify_latest_edit_timestamp();
                obj.notify_latest_edit_formatted_timestamp();
            }

            self.update_state();
            obj.emit_by_name::<()>("item-changed", &[]);
        }

        /// The underlying SDK timeline item.
        pub(super) fn item(&self) -> Arc<EventTimelineItem> {
            self.item
                .borrow()
                .clone()
                .expect("event should have timeline item after construction")
        }

        /// The global permanent or temporary identifier of this event.
        pub(super) fn identifier(&self) -> TimelineEventItemId {
            self.item().identifier()
        }

        /// The global permanent ID of this event, if it has been received from
        /// the server.
        pub(super) fn event_id(&self) -> Option<OwnedEventId> {
            self.item().event_id().map(ToOwned::to_owned)
        }

        /// The global permanent ID of this event, if it has been received from
        /// the server, as a string.
        fn event_id_string(&self) -> Option<String> {
            self.item().event_id().map(ToString::to_string)
        }

        /// The temporary ID of this event, if it has been sent with this
        /// session.
        pub(crate) fn transaction_id(&self) -> Option<OwnedTransactionId> {
            self.item().transaction_id().map(ToOwned::to_owned)
        }

        /// The ID of the sender of this event.
        pub(super) fn sender_id(&self) -> OwnedUserId {
            self.item().sender().to_owned()
        }

        /// The ID of the sender of this event, as a string.
        fn sender_id_string(&self) -> String {
            self.item().sender().to_string()
        }

        /// The timestamp of this event, as the number of milliseconds
        /// since Unix Epoch.
        pub(super) fn origin_server_ts(&self) -> MilliSecondsSinceUnixEpoch {
            self.item().timestamp()
        }

        /// The timestamp of this event, as a `GDateTime`.
        fn timestamp(&self) -> glib::DateTime {
            timestamp_to_date(self.origin_server_ts())
        }

        /// The formatted timestamp of this event.
        fn formatted_timestamp(&self) -> String {
            self.timestamp()
                .format("%c")
                .map(Into::into)
                .unwrap_or_default()
        }

        /// The raw JSON source, if it has been echoed back by the server.
        pub(super) fn raw(&self) -> Option<Raw<AnySyncTimelineEvent>> {
            self.item().original_json().cloned()
        }

        /// The pretty-formatted JSON source, if it has been echoed back by the
        /// server.
        fn source(&self) -> Option<String> {
            self.item()
                .original_json()
                .map(raw_to_pretty_string)
                .into_clean_string()
        }

        /// Whether we have the JSON source.
        fn has_source(&self) -> bool {
            self.item().original_json().is_some()
        }

        /// Compute the current state of this event.
        fn compute_state(&self) -> MessageState {
            let item = self.item();

            if let Some(send_state) = item.send_state() {
                match send_state {
                    EventSendState::NotSentYet { .. } => return MessageState::Sending,
                    EventSendState::SendingFailed {
                        error,
                        is_recoverable,
                    } => {
                        if !matches!(
                            self.state.get(),
                            MessageState::PermanentError | MessageState::RecoverableError,
                        ) {
                            error!("Could not send message: {error}");
                        }

                        let new_state = if *is_recoverable {
                            MessageState::RecoverableError
                        } else {
                            MessageState::PermanentError
                        };

                        return new_state;
                    }
                    EventSendState::Sent { .. } => {}
                }
            }

            if item.content().is_edited() {
                MessageState::Edited
            } else {
                MessageState::None
            }
        }

        /// Update the state of this event.
        fn update_state(&self) {
            let state = self.compute_state();

            if self.state.get() == state {
                return;
            }

            self.state.set(state);
            self.obj().notify_state();
        }

        /// Whether this event was edited.
        fn is_edited(&self) -> bool {
            self.item().content().is_edited()
        }

        /// The JSON source for the latest edit of this event, if any.
        fn latest_edit_raw(&self) -> Option<Raw<AnySyncTimelineEvent>> {
            self.item().latest_edit_raw()
        }

        /// The pretty-formatted JSON source for the latest edit of this event.
        ///
        /// This string is empty if the event is not edited.
        fn latest_edit_source(&self) -> String {
            self.latest_edit_raw()
                .as_ref()
                .map(raw_to_pretty_string)
                .into_clean_string()
                .unwrap_or_default()
        }

        /// The ID of the latest edit of this `Event`.
        ///
        /// This string is empty if the event is not edited.
        fn latest_edit_event_id_string(&self) -> String {
            self.latest_edit_raw()
                .as_ref()
                .and_then(|r| r.get_field::<String>("event_id").ok().flatten())
                .unwrap_or_default()
        }

        /// The timestamp of the latest edit of this `Event`, as a `GDateTime`,
        /// if any.
        fn latest_edit_timestamp(&self) -> Option<glib::DateTime> {
            self.latest_edit_raw()
                .as_ref()
                .and_then(|r| {
                    r.get_field::<MilliSecondsSinceUnixEpoch>("origin_server_ts")
                        .ok()
                        .flatten()
                })
                .map(timestamp_to_date)
        }

        /// The formatted timestamp of the latest edit of this `Event`.
        fn latest_edit_formatted_timestamp(&self) -> String {
            self.latest_edit_timestamp()
                .and_then(|d| d.format("%c").ok())
                .map(Into::into)
                .unwrap_or_default()
        }

        /// Whether this `Event` should be highlighted.
        fn is_highlighted(&self) -> bool {
            self.item().is_highlighted()
        }

        /// The read receipts on this event.
        fn read_receipts(&self) -> &gio::ListStore {
            self.read_receipts
                .get_or_init(gio::ListStore::new::<glib::BoxedAnyObject>)
        }

        /// The owned read receipts on this event.
        fn read_receipts_owned(&self) -> gio::ListStore {
            self.read_receipts().clone()
        }

        /// Update the read receipts list with the given receipts.
        fn update_read_receipts(&self, new_read_receipts: &IndexMap<OwnedUserId, Receipt>) {
            let old_count = self.read_receipts().n_items();
            let new_count = new_read_receipts.len() as u32;

            if old_count == new_count {
                let mut is_all_same = true;
                for (i, new_user_id) in new_read_receipts.keys().enumerate() {
                    let Some(old_receipt) = self
                        .read_receipts()
                        .item(i as u32)
                        .and_downcast::<glib::BoxedAnyObject>()
                    else {
                        is_all_same = false;
                        break;
                    };

                    if old_receipt.borrow::<UserReadReceipt>().user_id != *new_user_id {
                        is_all_same = false;
                        break;
                    }
                }

                if is_all_same {
                    return;
                }
            }

            let new_read_receipts = new_read_receipts
                .into_iter()
                .map(|(user_id, receipt)| {
                    glib::BoxedAnyObject::new(UserReadReceipt {
                        user_id: user_id.clone(),
                        receipt: receipt.clone(),
                    })
                })
                .collect::<Vec<_>>();
            self.read_receipts()
                .splice(0, old_count, &new_read_receipts);

            let prev_has_read_receipts = old_count > 0;
            let has_read_receipts = new_count > 0;

            if prev_has_read_receipts != has_read_receipts {
                self.obj().notify_has_read_receipts();
            }
        }

        /// Whether this event has any read receipt.
        fn has_read_receipts(&self) -> bool {
            self.read_receipts().n_items() > 0
        }

        /// Set the state of the header of the event in the room history.
        fn set_header_state(&self, state: EventHeaderState) {
            if self.header_state.get() == state {
                return;
            }

            self.header_state.set(state);
            self.obj().notify_header_state();
        }
    }
}

glib::wrapper! {
    /// A Matrix room event.
    pub struct Event(ObjectSubclass<imp::Event>) @extends TimelineItem;
}

impl Event {
    /// Create a new `Event` in the given room with the given SDK timeline item.
    pub fn new(timeline: &Timeline, item: EventTimelineItem, timeline_id: &str) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("timeline", timeline)
            .property("timeline-id", timeline_id)
            .build();

        obj.imp().set_item(item);

        obj
    }

    /// Update this event with the given SDK timeline item.
    pub(crate) fn update_with(&self, item: EventTimelineItem) {
        self.imp().set_item(item);
    }

    /// The underlying SDK timeline item.
    pub(crate) fn item(&self) -> Arc<EventTimelineItem> {
        self.imp().item()
    }

    /// The global permanent or temporary identifier of this event.
    pub(crate) fn identifier(&self) -> TimelineEventItemId {
        self.imp().identifier()
    }

    /// Whether the given identifier matches this event.
    ///
    /// The result can be different from comparing two [`TimelineEventItemId`]s
    /// because an event can have a transaction ID and an event ID.
    pub(crate) fn matches_identifier(&self, identifier: &TimelineEventItemId) -> bool {
        let item = self.item();
        match identifier {
            TimelineEventItemId::TransactionId(txn_id) => {
                item.transaction_id().is_some_and(|id| id == txn_id)
            }
            TimelineEventItemId::EventId(event_id) => {
                item.event_id().is_some_and(|id| id == event_id)
            }
        }
    }

    /// The permanent global ID of this event, if it has been received from the
    /// server.
    pub(crate) fn event_id(&self) -> Option<OwnedEventId> {
        self.imp().event_id()
    }

    /// The temporary ID of this event, if it has been sent with this session.
    pub(crate) fn transaction_id(&self) -> Option<OwnedTransactionId> {
        self.imp().transaction_id()
    }

    /// The ID of the sender of this event.
    pub(crate) fn sender_id(&self) -> OwnedUserId {
        self.imp().sender_id()
    }

    /// The sender of this event.
    ///
    /// This should only be called when the event's room members list is
    /// available, otherwise it will be created on every call.
    pub(crate) fn sender(&self) -> Member {
        self.room()
            .get_or_create_members()
            .get_or_create(self.sender_id())
    }

    /// The ID of the user targeted by this event, if any.
    ///
    /// A targeted user is only encountered with `m.room.member` events. This
    /// only returns `Some(_)` if the targeted user is different from the
    /// sender.
    pub(crate) fn target_user_id(&self) -> Option<OwnedUserId> {
        let item = self.item();
        match item.content() {
            TimelineItemContent::MembershipChange(membership_change) => {
                let target_user_id = membership_change.user_id();
                (target_user_id != item.sender()).then(|| target_user_id.to_owned())
            }
            _ => None,
        }
    }

    /// The user targeted by this event, if any.
    ///
    /// A targeted user is only encountered with `m.room.member` events. This
    /// only returns `Some(_)` if the targettd user is different from the
    /// sender.
    ///
    /// This should only be called when the event's room members list is
    /// available, otherwise it will be created on every call.
    pub(crate) fn target_user(&self) -> Option<Member> {
        let target_user_id = self.target_user_id()?;
        Some(
            self.room()
                .get_or_create_members()
                .get_or_create(target_user_id),
        )
    }

    /// The timestamp of this event, as the number of milliseconds
    /// since Unix Epoch.
    pub(crate) fn origin_server_ts(&self) -> MilliSecondsSinceUnixEpoch {
        self.imp().origin_server_ts()
    }

    /// The raw JSON source for this event, if it has been echoed back
    /// by the server.
    pub(crate) fn raw(&self) -> Option<Raw<AnySyncTimelineEvent>> {
        self.imp().raw()
    }

    /// The content of this event.
    pub(crate) fn content(&self) -> TimelineItemContent {
        self.item().content().clone()
    }

    /// The content of this event, if it is a message.
    ///
    /// This definition matches the `m.room.message` event type.
    pub(crate) fn message(&self) -> Option<Message> {
        match self.item().content() {
            TimelineItemContent::MsgLike(msg_like) => msg_like.as_message(),
            _ => None,
        }
    }

    /// The state of the poll of this event, if it is a poll.
    ///
    /// This definition matches the `m.poll.start` event type and its unstable
    /// variant from MSC3381.
    pub(crate) fn poll(&self) -> Option<PollState> {
        self.item().content().as_poll().cloned()
    }

    /// Whether this event contains a message-like content.
    ///
    /// This definition matches the following event types:
    ///
    /// - `m.room.message`
    /// - `m.sticker`
    /// - `m.poll.start` and its unstable variant from MSC3381
    pub(crate) fn is_message_like(&self) -> bool {
        match self.item().content() {
            TimelineItemContent::MsgLike(msg_like) => {
                matches!(
                    msg_like.kind,
                    MsgLikeKind::Message(_) | MsgLikeKind::Sticker(_) | MsgLikeKind::Poll(_)
                )
            }
            _ => false,
        }
    }

    /// Whether this is a call event.
    pub(crate) fn is_call_event(&self) -> bool {
        matches!(
            self.item().content(),
            TimelineItemContent::RtcNotification { .. }
        )
    }

    /// Whether this is a state event.
    pub(crate) fn is_state_event(&self) -> bool {
        matches!(
            self.item().content(),
            TimelineItemContent::MembershipChange(_)
                | TimelineItemContent::ProfileChange(_)
                | TimelineItemContent::OtherState(_)
        )
    }

    /// Whether this is a state event that can be grouped with others.
    pub(crate) fn is_state_group_event(&self) -> bool {
        match self.item().content() {
            TimelineItemContent::MembershipChange(_) | TimelineItemContent::ProfileChange(_) => {
                true
            }
            TimelineItemContent::OtherState(other_state) => {
                // `m.room.create` should only occur once per room and it has special rendering
                // so we do not group it.
                !matches!(
                    other_state.content(),
                    AnyOtherStateEventContentChange::RoomCreate(_)
                )
            }
            _ => false,
        }
    }

    /// Whether this is the `m.room.create` event of the room.
    pub(crate) fn is_room_create(&self) -> bool {
        match self.item().content() {
            TimelineItemContent::OtherState(other_state) => {
                matches!(
                    other_state.content(),
                    AnyOtherStateEventContentChange::RoomCreate(_),
                )
            }
            _ => false,
        }
    }

    /// The membership change, if this is an `m.room.member` event that contains
    /// one.
    pub(crate) fn membership_change(&self) -> Option<MembershipChange> {
        match self.item().content() {
            TimelineItemContent::MembershipChange(membership_change) => membership_change.change(),
            _ => None,
        }
    }

    /// The media message of this event, if any.
    pub(crate) fn media_message(&self) -> Option<MediaMessage> {
        match self.item().content() {
            TimelineItemContent::MsgLike(msg_like) => match &msg_like.kind {
                MsgLikeKind::Message(message) => MediaMessage::from_message(message.msgtype()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether this event might contain an `@room` mention.
    ///
    /// This means that either it does not have intentional mentions, or it has
    /// intentional mentions and `room` is set to `true`.
    pub(crate) fn can_contain_at_room(&self) -> bool {
        self.item().content().can_contain_at_room()
    }

    /// Whether this event can show a header.
    pub(crate) fn can_show_header(&self) -> bool {
        self.item().content().can_show_header()
    }

    /// Get the ID of the root event of the thread this event is a reply in,
    /// if any.
    pub(crate) fn thread_root_id(&self) -> Option<OwnedEventId> {
        self.item().content().thread_root()
    }

    /// The number of replies in the thread with this event as its root.
    ///
    /// Returns `None` if this event is not the root of a thread.
    pub(crate) fn thread_replies_count(&self) -> Option<u32> {
        self.item()
            .content()
            .thread_summary()
            .map(|summary| summary.num_replies)
    }

    /// Get the ID of the event this event replies to, if any.
    pub(crate) fn reply_to_id(&self) -> Option<OwnedEventId> {
        match self.item().content() {
            TimelineItemContent::MsgLike(msg_like) => {
                msg_like.in_reply_to.as_ref().map(|d| d.event_id.clone())
            }
            _ => None,
        }
    }

    /// Get the details of the event this event replies to, if any.
    ///
    /// Returns `None(_)` if this event is not a reply.
    pub(crate) fn reply_to_event_content(&self) -> Option<TimelineDetails<Box<EmbeddedEvent>>> {
        match self.item().content() {
            TimelineItemContent::MsgLike(msg_like) => {
                msg_like.in_reply_to.as_ref().map(|d| d.event.clone())
            }
            _ => None,
        }
    }

    /// Fetch missing details for this event.
    ///
    /// This is a no-op if called for a local event.
    pub(crate) async fn fetch_missing_details(&self) -> Result<(), TimelineError> {
        let Some(event_id) = self.event_id() else {
            return Ok(());
        };

        let timeline = self.timeline().matrix_timeline();
        spawn_tokio!(async move { timeline.fetch_details_for_event(&event_id).await })
            .await
            .expect("task was not aborted")
    }

    /// Whether this event can be replied to.
    pub(crate) fn can_be_replied_to(&self) -> bool {
        let item = self.item();

        // We only allow to reply to messages (but not stickers).
        if !item.content().is_message() {
            return false;
        }

        // The SDK API has its own rules.
        if !item.can_be_replied_to() {
            return false;
        }

        // Finally, check that the current permissions allow us to send messages.
        self.room().permissions().can_send_message()
    }

    /// Whether this event can be reacted to.
    pub(crate) fn can_be_reacted_to(&self) -> bool {
        // We only allow to react to messages and polls (but not stickers).
        let content = self.content();
        if !content.is_message() && !content.is_poll() {
            return false;
        }

        // We cannot react to an event that is being sent.
        if self.event_id().is_none() {
            return false;
        }

        // Finally, check that the current permissions allow us to send messages.
        self.room().permissions().can_send_reaction()
    }

    /// Whether this event can be redacted.
    ///
    /// This uses the raw JSON to be able to redact even events that failed to
    /// deserialize.
    pub(crate) fn can_be_redacted(&self) -> bool {
        let Some(raw) = self.raw() else {
            // Events without raw JSON are already redacted events, and events that are not
            // sent yet, we can ignore them.
            return false;
        };

        let is_redacted = match raw.get_field::<UnsignedRedactedDeHelper>("unsigned") {
            Ok(Some(unsigned)) => unsigned.redacted_because.is_some(),
            Ok(None) => {
                debug!("Missing unsigned field in event");
                false
            }
            Err(error) => {
                error!("Could not deserialize unsigned field in event: {error}");
                false
            }
        };
        if is_redacted {
            // There is no point in redacting it twice.
            return false;
        }

        match raw.get_field::<TimelineEventType>("type") {
            Ok(Some(t)) => !NON_REDACTABLE_EVENTS.contains(&t),
            Ok(None) => {
                debug!("Missing type field in event");
                true
            }
            Err(error) => {
                error!("Could not deserialize type field in event: {error}");
                true
            }
        }
    }

    /// Whether this `Event` can count as an unread message.
    ///
    /// This follows the algorithm in [MSC2654], excluding events that we don't
    /// show in the timeline.
    ///
    /// [MSC2654]: https://github.com/matrix-org/matrix-spec-proposals/pull/2654
    pub(crate) fn counts_as_unread(&self) -> bool {
        let item = self.item();
        item.is_remote_event() && item.content().counts_as_unread()
    }

    /// Whether this `Event` can count as activity in a room.
    ///
    /// This includes content that counts as unread, plus membership changes for
    /// our own user towards joining a room, so that freshly joined rooms are at
    /// the top of the list.
    pub(crate) fn counts_as_activity(&self, own_user_id: &UserId) -> bool {
        let item = self.item();
        item.is_remote_event() && item.content().counts_as_activity(own_user_id)
    }

    /// The `matrix.to` URI representation for this event.
    ///
    /// Returns `None` if we don't have the ID of the event.
    pub(crate) async fn matrix_to_uri(&self) -> Option<MatrixToUri> {
        Some(self.room().matrix_to_event_uri(self.event_id()?).await)
    }

    /// Listen to the signal emitted when the SDK item changed.
    pub(crate) fn connect_item_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "item-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

/// Convert raw JSON to a pretty-formatted JSON string.
fn raw_to_pretty_string<T>(raw: &Raw<T>) -> String {
    // We have to convert it to a Value, because a RawValue cannot be
    // pretty-printed.
    let json = serde_json::to_value(raw).unwrap();

    serde_json::to_string_pretty(&json).unwrap()
}

/// List of events that should not be redacted to avoid bricking a room.
const NON_REDACTABLE_EVENTS: &[TimelineEventType] = &[
    TimelineEventType::RoomCreate,
    TimelineEventType::RoomEncryption,
    TimelineEventType::RoomServerAcl,
];

/// A helper type to know whether an event was redacted.
#[derive(Deserialize)]
struct UnsignedRedactedDeHelper {
    redacted_because: Option<IgnoredAny>,
}
