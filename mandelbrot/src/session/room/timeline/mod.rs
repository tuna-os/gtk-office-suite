use std::{collections::HashMap, ops::ControlFlow, sync::Arc};

use futures_util::StreamExt;
use gtk::{
    gio, glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use matrix_sdk_ui::{
    eyeball_im::VectorDiff,
    timeline::{
        RoomExt, Timeline as SdkTimeline, TimelineEventItemId, TimelineFocus,
        TimelineItem as SdkTimelineItem, default_event_filter,
    },
};
use ruma::{
    EventId, OwnedEventId, UserId,
    events::{
        AnySyncMessageLikeEvent, AnySyncStateEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
        SyncStateEvent, room::message::MessageType,
    },
    room_version_rules::RoomVersionRules,
};
use tokio::task::AbortHandle;
use tracing::error;

mod event;
mod timeline_diff_minimizer;
mod timeline_item;
mod virtual_item;

use self::timeline_diff_minimizer::{TimelineDiff, TimelineDiffItemStore};
pub(crate) use self::{
    event::*,
    timeline_item::{TimelineItem, TimelineItemExt, TimelineItemImpl},
    virtual_item::{VirtualItem, VirtualItemKind},
};
use super::Room;
use crate::{
    prelude::*,
    spawn, spawn_tokio,
    utils::{LoadingState, SingleItemListModel},
};

/// The number of events to request when loading more history.
const MAX_BATCH_SIZE: u16 = 20;
/// The maximum time between contiguous events before we show their header, in
/// milliseconds.
///
/// This matches 20 minutes.
const MAX_TIME_BETWEEN_HEADERS: u64 = 20 * 60 * 1000;

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        iter,
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::Timeline)]
    pub struct Timeline {
        /// The room containing this timeline.
        #[property(get, set = Self::set_room, construct_only)]
        room: OnceCell<Room>,
        /// The ID of the thread root event, if this timeline is focused on a
        /// thread, as a string.
        #[property(get = Self::thread_root_id_string, set = Self::set_thread_root_id_string, construct_only, nullable, type = Option<String>)]
        thread_root_id_string: OnceCell<Option<OwnedEventId>>,
        /// The underlying SDK timeline.
        matrix_timeline: OnceCell<Arc<SdkTimeline>>,
        /// Items added at the start of the timeline.
        ///
        /// Currently this can only contain one item at a time.
        start_items: OnceCell<SingleItemListModel>,
        /// Items provided by the SDK timeline.
        sdk_items: OnceCell<gio::ListStore>,
        /// Filter for the list of items provided by the SDK timeline.
        filter: gtk::CustomFilter,
        /// Filtered list of items provided by the SDK timeline.
        filtered_sdk_items: gtk::FilterListModel,
        /// Items added at the end of the timeline.
        ///
        /// Currently this can only contain one item at a time.
        end_items: OnceCell<SingleItemListModel>,
        /// The `GListModel` containing all the timeline items.
        #[property(get = Self::items)]
        items: OnceCell<gtk::FlattenListModel>,
        /// A Hashmap linking a `TimelineEventItemId` to the corresponding
        /// `Event`.
        pub(super) event_map: RefCell<HashMap<TimelineEventItemId, Event>>,
        /// The loading state of the timeline.
        #[property(get, builder(LoadingState::default()))]
        state: Cell<LoadingState>,
        /// Whether we are loading events at the start of the timeline.
        #[property(get)]
        is_loading_start: Cell<bool>,
        /// Whether the timeline is empty.
        #[property(get = Self::is_empty)]
        is_empty: PhantomData<bool>,
        /// Whether the timeline should be pre-loaded when it is ready.
        #[property(get, set = Self::set_preload, explicit_notify)]
        preload: Cell<bool>,
        /// Whether we have reached the start of the timeline.
        #[property(get)]
        has_reached_start: Cell<bool>,
        /// Whether we have the `m.room.create` event in the timeline.
        #[property(get)]
        has_room_create: Cell<bool>,
        diff_handle: OnceCell<AbortHandle>,
        back_pagination_status_handle: OnceCell<AbortHandle>,
        read_receipts_changed_handle: OnceCell<AbortHandle>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Timeline {
        const NAME: &'static str = "Timeline";
        type Type = super::Timeline;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Timeline {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("read-change-trigger").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            if self.thread_root_id().is_none() {
                self.room().typing_list().connect_is_empty_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |list| {
                        if !list.is_empty() {
                            imp.add_typing_row();
                        }
                    }
                ));
            }

            self.filter.set_filter_func(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                true,
                move |obj| {
                    // Hide the timeline start item if we have the `m.room.create` event too.
                    obj.downcast_ref::<VirtualItem>().is_none_or(|item| {
                        !(imp.has_room_create.get()
                            && item.kind() == VirtualItemKind::TimelineStart)
                    })
                }
            ));
            self.filtered_sdk_items.set_filter(Some(&self.filter));
        }

        fn dispose(&self) {
            if let Some(handle) = self.diff_handle.get() {
                handle.abort();
            }
            if let Some(handle) = self.back_pagination_status_handle.get() {
                handle.abort();
            }
            if let Some(handle) = self.read_receipts_changed_handle.get() {
                handle.abort();
            }
        }
    }

    impl Timeline {
        /// Set the room containing this timeline.
        fn set_room(&self, room: Room) {
            self.room.get_or_init(|| room);
        }

        /// Set the ID of the thread root event of this timeline, as a string.
        fn set_thread_root_id_string(&self, thread_root_id: Option<String>) {
            let thread_root_id = thread_root_id
                .map(|id| EventId::parse(id).expect("thread root ID should be a valid event ID"));
            self.thread_root_id_string
                .set(thread_root_id)
                .expect("thread root ID is uninitialized");
        }

        /// The ID of the thread root event, if this timeline is focused on a
        /// thread, as a string.
        fn thread_root_id_string(&self) -> Option<String> {
            self.thread_root_id().map(ToString::to_string)
        }

        /// The ID of the thread root event, if this timeline is focused on a
        /// thread.
        pub(super) fn thread_root_id(&self) -> Option<&EventId> {
            self.thread_root_id_string.get().and_then(Option::as_deref)
        }

        /// The room containing this timeline.
        fn room(&self) -> &Room {
            self.room.get().expect("room should be initialized")
        }

        /// Initialize the underlying SDK timeline.
        pub(super) async fn init_matrix_timeline(&self) {
            let room = self.room();

            let own_user_id = room.own_member().user_id().to_owned();
            let filter = {
                let own_user_id = own_user_id.clone();
                move |any: &AnySyncTimelineEvent, rules: &RoomVersionRules| -> bool {
                    show_in_timeline(any, rules, &own_user_id)
                }
            };
            let matrix_room = room.matrix_room().clone();
            let focus = match self.thread_root_id() {
                Some(root_event_id) => TimelineFocus::Thread {
                    root_event_id: root_event_id.to_owned(),
                },
                None => TimelineFocus::Live {
                    // We support opening thread timelines from the thread
                    // roots, so we can hide in-thread replies.
                    hide_threaded_events: true,
                },
            };
            let handle = spawn_tokio!(async move {
                matrix_room
                    .timeline_builder()
                    .with_focus(focus)
                    .event_filter(filter)
                    .add_failed_to_parse(false)
                    .build()
                    .await
            });

            let matrix_timeline = match handle.await.expect("task was not aborted") {
                Ok(timeline) => timeline,
                Err(error) => {
                    error!("Could not create timeline: {error}");
                    return;
                }
            };

            let matrix_timeline = Arc::new(matrix_timeline);
            self.matrix_timeline
                .set(matrix_timeline.clone())
                .expect("matrix timeline is uninitialized");

            let (values, timeline_stream) = matrix_timeline.subscribe().await;

            if *IS_AT_TRACE_LEVEL {
                tracing::trace!(
                    room = self.room().human_readable_id(),
                    items = ?sdk_items_to_log(&values),
                    "Initial timeline items",
                );
            }

            if !values.is_empty() {
                self.update_with_single_diff(VectorDiff::Append { values });
            }

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let room_id = room.room_id().to_owned();
            let fut = timeline_stream.for_each(move |diff_list| {
                let obj_weak = obj_weak.clone();
                let room_id = room_id.clone();
                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().update_with_diff_list(diff_list);
                            } else {
                                error!(
                                    "Could not send timeline diff for room {room_id}: \
                                     could not upgrade weak reference"
                                );
                            }
                        });
                    });
                }
            });

            let diff_handle = spawn_tokio!(fut);
            self.diff_handle
                .set(diff_handle.abort_handle())
                .expect("handle should be uninitialized");

            self.watch_read_receipts().await;

            if self.preload.get() {
                self.preload().await;
            }

            self.set_state(LoadingState::Ready);
        }

        /// The underlying SDK timeline.
        pub(super) fn matrix_timeline(&self) -> &Arc<SdkTimeline> {
            self.matrix_timeline
                .get()
                .expect("matrix timeline should be initialized")
        }

        /// Items added at the start of the timeline.
        fn start_items(&self) -> &SingleItemListModel {
            self.start_items.get_or_init(|| {
                let model = SingleItemListModel::new(Some(&VirtualItem::spinner(&self.obj())));
                model.set_is_hidden(true);
                model
            })
        }

        /// Items provided by the SDK timeline.
        pub(super) fn sdk_items(&self) -> &gio::ListStore {
            self.sdk_items.get_or_init(|| {
                let sdk_items = gio::ListStore::new::<TimelineItem>();
                self.filtered_sdk_items.set_model(Some(&sdk_items));
                sdk_items
            })
        }

        /// Items added at the end of the timeline.
        fn end_items(&self) -> &SingleItemListModel {
            self.end_items.get_or_init(|| {
                let model = SingleItemListModel::new(Some(&VirtualItem::typing(&self.obj())));
                model.set_is_hidden(true);
                model
            })
        }

        /// The `GListModel` containing all the timeline items.
        fn items(&self) -> gtk::FlattenListModel {
            self.items
                .get_or_init(|| {
                    let model_list = gio::ListStore::new::<gio::ListModel>();
                    model_list.append(self.start_items());
                    model_list.append(&self.filtered_sdk_items);
                    model_list.append(self.end_items());
                    gtk::FlattenListModel::new(Some(model_list))
                })
                .clone()
        }

        /// Whether the timeline is empty.
        fn is_empty(&self) -> bool {
            self.filtered_sdk_items.n_items() == 0
        }

        /// Set the loading state of the timeline.
        fn set_state(&self, state: LoadingState) {
            if self.state.get() == state {
                return;
            }

            self.state.set(state);

            self.obj().notify_state();
        }

        /// Update the loading state of the timeline.
        fn update_loading_state(&self) {
            let is_loading = self.is_loading_start.get();

            if is_loading {
                self.set_state(LoadingState::Loading);
            } else if self.state.get() != LoadingState::Error {
                self.set_state(LoadingState::Ready);
            }
        }

        /// Set whether we are loading events at the start of the timeline.
        fn set_loading_start(&self, is_loading_start: bool) {
            if self.is_loading_start.get() == is_loading_start {
                return;
            }

            self.is_loading_start.set(is_loading_start);

            self.update_loading_state();
            self.start_items().set_is_hidden(!is_loading_start);
            self.obj().notify_is_loading_start();
        }

        /// Set whether we have reached the start of the timeline.
        fn set_has_reached_start(&self, has_reached_start: bool) {
            if self.has_reached_start.get() == has_reached_start {
                // Nothing to do.
                return;
            }

            self.has_reached_start.set(has_reached_start);

            self.obj().notify_has_reached_start();
        }

        /// Set whether the timeline has the `m.room.create` event of the room.
        fn set_has_room_create(&self, has_room_create: bool) {
            if self.has_room_create.get() == has_room_create {
                return;
            }

            self.has_room_create.set(has_room_create);

            let change = if has_room_create {
                gtk::FilterChange::MoreStrict
            } else {
                gtk::FilterChange::LessStrict
            };
            self.filter.changed(change);

            self.obj().notify_has_room_create();
        }

        /// Clear the state of the timeline.
        ///
        /// This doesn't handle removing items in `sdk_items` because it can be
        /// optimized by the caller of the function.
        fn clear(&self) {
            self.event_map.borrow_mut().clear();
            self.set_has_reached_start(false);
            self.set_has_room_create(false);
        }

        /// Set whether the timeline should be pre-loaded when it is ready.
        fn set_preload(&self, preload: bool) {
            if self.preload.get() == preload {
                return;
            }

            self.preload.set(preload);
            self.obj().notify_preload();

            if preload && self.can_paginate_backwards() {
                spawn!(
                    glib::Priority::DEFAULT_IDLE,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        async move {
                            imp.preload().await;
                        }
                    )
                );
            }
        }

        /// Preload the timeline, if there are not enough items.
        async fn preload(&self) {
            if self.filtered_sdk_items.n_items() < u32::from(MAX_BATCH_SIZE) {
                self.paginate_backwards(|| ControlFlow::Break(())).await;
            }
        }

        /// Update this timeline with the given diff list.
        fn update_with_diff_list(&self, diff_list: Vec<VectorDiff<Arc<SdkTimelineItem>>>) {
            if *IS_AT_TRACE_LEVEL {
                self.log_diff_list(&diff_list);
            }

            let was_empty = self.is_empty();

            if let Some(diff_list) = self.try_minimize_diff_list(diff_list) {
                // The diff could not be minimized, handle it manually.
                for diff in diff_list {
                    self.update_with_single_diff(diff);
                }
            }

            if *IS_AT_TRACE_LEVEL {
                self.log_items();
            }

            let obj = self.obj();
            if self.is_empty() != was_empty {
                obj.notify_is_empty();
            }

            obj.emit_read_change_trigger();
        }

        /// Attempt to minimize the given list of diffs.
        ///
        /// This is necessary because the SDK diffs are not always optimized,
        /// e.g. an item is removed then re-added, which creates jumps in the
        /// room history.
        ///
        /// Returns the list of diffs if it could not be minimized.
        fn try_minimize_diff_list(
            &self,
            diff_list: Vec<VectorDiff<Arc<SdkTimelineItem>>>,
        ) -> Option<Vec<VectorDiff<Arc<SdkTimelineItem>>>> {
            if !self.can_minimize_diff_list(&diff_list) {
                return Some(diff_list);
            }

            self.minimize_diff_list(diff_list);

            None
        }

        /// Update this timeline with the given diff.
        fn update_with_single_diff(&self, diff: VectorDiff<Arc<SdkTimelineItem>>) {
            match diff {
                VectorDiff::Append { values } => {
                    let new_list = values
                        .into_iter()
                        .map(|item| self.create_item(&item))
                        .collect::<Vec<_>>();

                    self.update_items(self.sdk_items().n_items(), 0, &new_list);
                }
                VectorDiff::Clear => {
                    self.sdk_items().remove_all();
                    self.clear();
                }
                VectorDiff::PushFront { value } => {
                    let item = self.create_item(&value);
                    self.update_items(0, 0, &[item]);
                }
                VectorDiff::PushBack { value } => {
                    let item = self.create_item(&value);
                    self.update_items(self.sdk_items().n_items(), 0, &[item]);
                }
                VectorDiff::PopFront => {
                    self.update_items(0, 1, &[]);
                }
                VectorDiff::PopBack => {
                    self.update_items(self.sdk_items().n_items().saturating_sub(1), 1, &[]);
                }
                VectorDiff::Insert { index, value } => {
                    let item = self.create_item(&value);
                    self.update_items(index as u32, 0, &[item]);
                }
                VectorDiff::Set { index, value } => {
                    let pos = index as u32;
                    let item = self
                        .item_at(pos)
                        .expect("there should be an item at the given position");

                    if item.timeline_id() == value.unique_id().0 {
                        // This is the same item, update it.
                        self.update_item(&item, &value);
                        // The header visibility might have changed.
                        self.update_items_headers(pos, 1);
                    } else {
                        let item = self.create_item(&value);
                        self.update_items(pos, 1, &[item]);
                    }
                }
                VectorDiff::Remove { index } => {
                    self.update_items(index as u32, 1, &[]);
                }
                VectorDiff::Truncate { length } => {
                    let length = length as u32;
                    let old_len = self.sdk_items().n_items();
                    self.update_items(length, old_len.saturating_sub(length), &[]);
                }
                VectorDiff::Reset { values } => {
                    // Reset the state.
                    self.clear();

                    let removed = self.sdk_items().n_items();
                    let new_list = values
                        .into_iter()
                        .map(|item| self.create_item(&item))
                        .collect::<Vec<_>>();

                    self.update_items(0, removed, &new_list);
                }
            }
        }

        /// Get the item at the given position.
        fn item_at(&self, pos: u32) -> Option<TimelineItem> {
            self.sdk_items().item(pos).and_downcast()
        }

        /// Update the items at the given position by removing the given number
        /// of items and adding the given items.
        fn update_items(&self, pos: u32, n_removals: u32, additions: &[TimelineItem]) {
            for i in pos..pos + n_removals {
                let Some(item) = self.item_at(i) else {
                    // This should not happen.
                    error!("Timeline item at position {i} not found");
                    break;
                };

                self.remove_item(&item);
            }

            self.sdk_items().splice(pos, n_removals, additions);

            // Update the header visibility of all the new additions, and the first item
            // after this batch.
            self.update_items_headers(pos, additions.len() as u32);

            // Try to update the latest unread message.
            if !additions.is_empty() {
                self.room().update_latest_activity(
                    additions.iter().filter_map(|i| i.downcast_ref::<Event>()),
                );
            }
        }

        /// Update the headers of the item at the given position and the given
        /// number of items after it.
        fn update_items_headers(&self, pos: u32, nb: u32) {
            let sdk_items = self.sdk_items();

            let (mut previous_sender, mut previous_timestamp) = if pos > 0 {
                sdk_items
                    .item(pos - 1)
                    .and_downcast::<Event>()
                    .filter(Event::can_show_header)
                    .map(|event| (event.sender_id(), event.origin_server_ts()))
            } else {
                None
            }
            .unzip();

            // Update the headers of changed events plus the first event after them.
            for i in pos..=pos + nb {
                let Some(current) = self.item_at(i) else {
                    break;
                };
                let Ok(current) = current.downcast::<Event>() else {
                    previous_sender = None;
                    continue;
                };

                let current_sender = current.sender_id();

                if !current.can_show_header() {
                    current.set_header_state(EventHeaderState::Hidden);
                    previous_sender = None;
                    previous_timestamp = None;
                    continue;
                }

                let header_state = if previous_sender
                    .as_ref()
                    .is_none_or(|previous_sender| current_sender != *previous_sender)
                {
                    // The sender is different, show the full header.
                    EventHeaderState::Full
                } else if previous_timestamp
                    .and_then(|ts| current.origin_server_ts().0.checked_sub(ts.0))
                    .is_some_and(|elapsed| u64::from(elapsed) >= MAX_TIME_BETWEEN_HEADERS)
                {
                    // Too much time has passed, show the timestamp.
                    EventHeaderState::TimestampOnly
                } else {
                    // Do not show header.
                    EventHeaderState::Hidden
                };

                current.set_header_state(header_state);
                previous_sender = Some(current_sender);
                previous_timestamp = Some(current.origin_server_ts());
            }
        }

        /// Remove the given item from this `Timeline`.
        fn remove_item(&self, item: &TimelineItem) {
            if let Some(event) = item.downcast_ref::<Event>() {
                let mut removed_from_map = false;
                let mut event_map = self.event_map.borrow_mut();

                // We need to remove both the transaction ID and the event ID.
                let identifiers = event
                    .transaction_id()
                    .map(TimelineEventItemId::TransactionId)
                    .into_iter()
                    .chain(event.event_id().map(TimelineEventItemId::EventId));

                for id in identifiers {
                    // We check if we are removing the right event, in case we receive a diff that
                    // adds an existing event to another place, making us create a new event, before
                    // another diff that removes it from its old place, making us remove the old
                    // event.
                    let found = event_map.get(&id).is_some_and(|e| e == event);

                    if found {
                        event_map.remove(&id);
                        removed_from_map = true;
                    }
                }

                if removed_from_map && event.is_room_create() {
                    self.set_has_room_create(false);
                }
            }
        }

        /// Whether we can load more events at the start of the timeline with
        /// the current state.
        pub(super) fn can_paginate_backwards(&self) -> bool {
            // We do not want to load twice at the same time, and it's useless to try to
            // load more history before the timeline is ready or if we have
            // reached the start of the timeline.
            self.state.get() != LoadingState::Initial
                && !self.is_loading_start.get()
                && !self.has_reached_start.get()
        }

        /// Load more events at the start of the timeline until the given
        /// function tells us to stop.
        pub(super) async fn paginate_backwards<F>(&self, continue_fn: F)
        where
            F: Fn() -> ControlFlow<()>,
        {
            self.set_loading_start(true);

            loop {
                if !self.paginate_backwards_inner().await {
                    break;
                }

                if continue_fn().is_break() {
                    break;
                }
            }

            self.set_loading_start(false);
        }

        /// Load more events at the start of the timeline.
        ///
        /// Returns `true` if more events can be loaded.
        async fn paginate_backwards_inner(&self) -> bool {
            let matrix_timeline = self.matrix_timeline().clone();
            let handle =
                spawn_tokio!(
                    async move { matrix_timeline.paginate_backwards(MAX_BATCH_SIZE).await }
                );

            match handle.await.expect("task was not aborted") {
                Ok(reached_start) => {
                    if reached_start {
                        self.set_has_reached_start(true);
                    }

                    !reached_start
                }
                Err(error) => {
                    error!("Could not load timeline: {error}");
                    self.set_state(LoadingState::Error);
                    false
                }
            }
        }

        /// Add the typing row to the timeline, if it isn't present already.
        fn add_typing_row(&self) {
            self.end_items().set_is_hidden(false);
        }

        /// Remove the typing row from the timeline.
        pub(super) fn remove_empty_typing_row(&self) {
            if !self.room().typing_list().is_empty() {
                return;
            }

            self.end_items().set_is_hidden(true);
        }

        /// Listen to read receipts changes.
        async fn watch_read_receipts(&self) {
            let room_id = self.room().room_id().to_owned();
            let matrix_timeline = self.matrix_timeline();

            let stream = matrix_timeline
                .subscribe_own_user_read_receipts_changed()
                .await;

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = stream.for_each(move |()| {
                let obj_weak = obj_weak.clone();
                let room_id = room_id.clone();
                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.emit_read_change_trigger();
                            } else {
                                error!(
                                    "Could not emit read change trigger for room {room_id}: \
                                     could not upgrade weak reference"
                                );
                            }
                        });
                    });
                }
            });

            let handle = spawn_tokio!(fut);
            self.read_receipts_changed_handle
                .set(handle.abort_handle())
                .expect("handle is uninitialized");
        }
    }

    impl TimelineDiffItemStore for Timeline {
        type Item = TimelineItem;
        type Data = Arc<SdkTimelineItem>;

        fn items(&self) -> Vec<TimelineItem> {
            self.sdk_items()
                .snapshot()
                .into_iter()
                .map(|obj| {
                    obj.downcast::<TimelineItem>()
                        .expect("SDK items are TimelineItems")
                })
                .collect()
        }

        fn create_item(&self, data: &Arc<SdkTimelineItem>) -> TimelineItem {
            let item = TimelineItem::new(data, &self.obj());

            if let Some(event) = item.downcast_ref::<Event>() {
                self.event_map
                    .borrow_mut()
                    .insert(event.identifier(), event.clone());

                // Keep track of the activity of the sender.
                if event.counts_as_unread()
                    && let Some(members) = self.room().members()
                {
                    let member = members.get_or_create(event.sender_id());
                    member.set_latest_activity(u64::from(event.origin_server_ts().get()));
                }

                if event.is_room_create() {
                    self.set_has_room_create(true);
                }
            }

            item
        }

        fn update_item(&self, item: &TimelineItem, data: &Arc<SdkTimelineItem>) {
            item.update_with(data);

            if let Some(event) = item.downcast_ref::<Event>() {
                // Update the identifier in the event map, in case we switched from a
                // transaction ID to an event ID.
                self.event_map
                    .borrow_mut()
                    .insert(event.identifier(), event.clone());

                // Try to update the latest unread message.
                self.room().update_latest_activity(iter::once(event));
            }
        }

        fn apply_item_diff_list(&self, item_diff_list: Vec<TimelineDiff<TimelineItem>>) {
            for item_diff in item_diff_list {
                match item_diff {
                    TimelineDiff::Splice(splice) => {
                        self.update_items(splice.pos, splice.n_removals, &splice.additions);
                    }
                    TimelineDiff::Update(update) => {
                        self.update_items_headers(update.pos, update.n_items);
                    }
                }
            }
        }
    }

    /// The default log filter initialized with the `RUST_LOG` environment
    /// variable.
    ///
    /// Used to know if we are likely to need to log the diff.
    static IS_AT_TRACE_LEVEL: LazyLock<bool> = LazyLock::new(|| {
        tracing_subscriber::EnvFilter::try_from_default_env()
            // If the env variable is not set, we know that we are not at trace level.
            .ok()
            .and_then(|filter| filter.max_level_hint())
            .is_some_and(|max| max == tracing::level_filters::LevelFilter::TRACE)
    });

    /// Temporary methods to debug items in the timeline.
    impl Timeline {
        /// Log the given diff list.
        fn log_diff_list(&self, diff_list: &[VectorDiff<Arc<SdkTimelineItem>>]) {
            let mut log_list = Vec::with_capacity(diff_list.len());

            for diff in diff_list {
                let log = match diff {
                    VectorDiff::Append { values } => {
                        format!("append: {:?}", sdk_items_to_log(values))
                    }
                    VectorDiff::Clear => "clear".to_owned(),
                    VectorDiff::PushFront { value } => {
                        format!("push_front: {}", sdk_item_to_log(value))
                    }
                    VectorDiff::PushBack { value } => {
                        format!("push_back: {}", sdk_item_to_log(value))
                    }
                    VectorDiff::PopFront => "pop_front".to_owned(),
                    VectorDiff::PopBack => "pop_back".to_owned(),
                    VectorDiff::Insert { index, value } => {
                        format!("insert at {index}: {}", sdk_item_to_log(value))
                    }
                    VectorDiff::Set { index, value } => {
                        format!("set at {index}: {}", sdk_item_to_log(value))
                    }
                    VectorDiff::Remove { index } => format!("remove at {index}"),
                    VectorDiff::Truncate { length } => format!("truncate at {length}"),
                    VectorDiff::Reset { values } => {
                        format!("reset: {:?}", sdk_items_to_log(values))
                    }
                };

                log_list.push(log);
            }

            tracing::trace!(
                room = self.room().human_readable_id(),
                "Diff list: {log_list:#?}"
            );
        }

        /// Log the items in this timeline.
        fn log_items(&self) {
            let items = self
                .sdk_items()
                .iter::<TimelineItem>()
                .filter_map(|item| item.as_ref().map(item_to_log).ok())
                .collect::<Vec<_>>();

            tracing::trace!(
                room = self.room().human_readable_id(),
                "Timeline: {items:#?}"
            );
        }
    }

    // Helper methods for logging items.
    fn sdk_items_to_log(
        items: &matrix_sdk_ui::eyeball_im::Vector<Arc<SdkTimelineItem>>,
    ) -> Vec<String> {
        items.iter().map(|item| sdk_item_to_log(item)).collect()
    }

    fn sdk_item_to_log(item: &SdkTimelineItem) -> String {
        match item.kind() {
            matrix_sdk_ui::timeline::TimelineItemKind::Event(event) => {
                format!("event::{:?}", event.identifier())
            }
            matrix_sdk_ui::timeline::TimelineItemKind::Virtual(virtual_item) => {
                format!("virtual::{virtual_item:?}")
            }
        }
    }

    fn item_to_log(item: &TimelineItem) -> String {
        if let Some(virtual_item) = item.downcast_ref::<VirtualItem>() {
            format!("virtual::{:?}", virtual_item.kind())
        } else if let Some(event) = item.downcast_ref::<Event>() {
            format!("event::{:?}", event.identifier())
        } else {
            "Unknown item".to_owned()
        }
    }
}

glib::wrapper! {
    /// All loaded items in a room.
    ///
    /// There is no strict message ordering enforced by the Timeline; items
    /// will be appended/prepended to existing items in the order they are
    /// received by the server.
    pub struct Timeline(ObjectSubclass<imp::Timeline>);
}

impl Timeline {
    /// Construct a new `Timeline` for the given room.
    pub(crate) fn new(room: &Room) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("room", room)
            .build();

        let imp = obj.imp();
        spawn!(clone!(
            #[weak]
            imp,
            async move {
                imp.init_matrix_timeline().await;
            }
        ));

        obj
    }

    /// Construct a new `Timeline` for the given room, focused on the thread
    /// with the given root event ID.
    pub(crate) fn with_thread_root(room: &Room, thread_root_id: &EventId) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("room", room)
            .property("thread-root-id-string", thread_root_id.as_str())
            .build();

        let imp = obj.imp();
        spawn!(clone!(
            #[weak]
            imp,
            async move {
                imp.init_matrix_timeline().await;
            }
        ));

        obj
    }

    /// The ID of the thread root event, if this timeline is focused on a
    /// thread.
    pub(crate) fn thread_root_id(&self) -> Option<OwnedEventId> {
        self.imp().thread_root_id().map(ToOwned::to_owned)
    }

    /// Whether this timeline is focused on a thread.
    pub(crate) fn is_thread(&self) -> bool {
        self.imp().thread_root_id().is_some()
    }

    /// The underlying SDK timeline.
    pub(crate) fn matrix_timeline(&self) -> Arc<SdkTimeline> {
        self.imp().matrix_timeline().clone()
    }

    /// Load more events at the start of the timeline until the given function
    /// tells us to stop.
    pub(crate) async fn paginate_backwards<F>(&self, continue_fn: F)
    where
        F: Fn() -> ControlFlow<()>,
    {
        let imp = self.imp();

        if !imp.can_paginate_backwards() {
            return;
        }

        imp.paginate_backwards(continue_fn).await;
    }

    /// Get the event with the given identifier from this `Timeline`.
    ///
    /// Use this method if you are sure the event has already been received.
    /// Otherwise use `fetch_event_by_id`.
    pub(crate) fn event_by_identifier(&self, identifier: &TimelineEventItemId) -> Option<Event> {
        self.imp().event_map.borrow().get(identifier).cloned()
    }

    /// Get the position of the event with the given identifier in this
    /// `Timeline`.
    pub(crate) fn find_event_position(&self, identifier: &TimelineEventItemId) -> Option<usize> {
        self.items()
            .iter::<glib::Object>()
            .enumerate()
            .find_map(|(index, item)| {
                item.ok()
                    .and_downcast::<Event>()
                    .is_some_and(|event| event.matches_identifier(identifier))
                    .then_some(index)
            })
    }

    /// Remove the typing row from the timeline.
    pub(crate) fn remove_empty_typing_row(&self) {
        self.imp().remove_empty_typing_row();
    }

    /// Whether this timeline has unread messages.
    ///
    /// Returns `None` if it is not possible to know, for example if there are
    /// no events in the Timeline.
    pub(crate) async fn has_unread_messages(&self) -> Option<bool> {
        let session = self.room().session()?;
        let own_user_id = session.user_id().clone();
        let matrix_timeline = self.matrix_timeline();

        let user_receipt_item = spawn_tokio!(async move {
            matrix_timeline
                .latest_user_read_receipt_timeline_event_id(&own_user_id)
                .await
        })
        .await
        .expect("task was not aborted");

        let sdk_items = self.imp().sdk_items();
        let count = sdk_items.n_items();

        for pos in (0..count).rev() {
            let Some(event) = sdk_items.item(pos).and_downcast::<Event>() else {
                continue;
            };

            if user_receipt_item.is_some() && event.event_id() == user_receipt_item {
                // The event is the oldest one, we have read it all.
                return Some(false);
            }
            if event.counts_as_unread() {
                // There is at least one unread event.
                return Some(true);
            }
        }

        // This should only happen if we do not have a read receipt item in the
        // timeline, and there are not enough events in the timeline to know if there
        // are unread messages.
        None
    }

    /// The IDs of redactable events sent by the given user in this timeline.
    pub(crate) fn redactable_events_for(&self, user_id: &UserId) -> Vec<OwnedEventId> {
        let mut events = vec![];

        for item in self.imp().sdk_items().iter::<glib::Object>() {
            let Ok(item) = item else {
                // The iterator is broken.
                break;
            };
            let Ok(event) = item.downcast::<Event>() else {
                continue;
            };

            if event.sender_id() != user_id {
                continue;
            }

            if event.can_be_redacted()
                && let Some(event_id) = event.event_id()
            {
                events.push(event_id);
            }
        }

        events
    }

    /// Emit the trigger that a read change might have occurred.
    fn emit_read_change_trigger(&self) {
        self.emit_by_name::<()>("read-change-trigger", &[]);
    }

    /// Connect to the trigger emitted when a read change might have occurred.
    pub(crate) fn connect_read_change_trigger<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "read-change-trigger",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}

/// Whether the given event should be shown in the timeline.
fn show_in_timeline(
    any: &AnySyncTimelineEvent,
    rules: &RoomVersionRules,
    own_user_id: &UserId,
) -> bool {
    // Make sure we do not show events that cannot be shown.
    if !default_event_filter(any, rules) {
        return false;
    }

    // Only show events we want.
    match any {
        AnySyncTimelineEvent::MessageLike(msg) => match msg {
            AnySyncMessageLikeEvent::RoomMessage(SyncMessageLikeEvent::Original(ev)) => {
                matches!(
                    ev.content.msgtype,
                    MessageType::Audio(_)
                        | MessageType::Emote(_)
                        | MessageType::File(_)
                        | MessageType::Image(_)
                        | MessageType::Location(_)
                        | MessageType::Notice(_)
                        | MessageType::ServerNotice(_)
                        | MessageType::Text(_)
                        | MessageType::Video(_)
                )
            }
            AnySyncMessageLikeEvent::Sticker(SyncMessageLikeEvent::Original(_))
            | AnySyncMessageLikeEvent::RoomEncrypted(SyncMessageLikeEvent::Original(_)) => true,
            AnySyncMessageLikeEvent::RtcNotification(SyncMessageLikeEvent::Original(ev)) => {
                ev.sender == own_user_id
                    || ev.content.mentions.as_ref().is_some_and(|mentions| {
                        mentions.room || mentions.user_ids.contains(own_user_id)
                    })
            }
            _ => false,
        },
        AnySyncTimelineEvent::State(AnySyncStateEvent::RoomMember(SyncStateEvent::Original(
            member_event,
        ))) => {
            // Do not show member events if the content that we support has not
            // changed. This avoids duplicate "user has joined" events in the
            // timeline which are confusing and wrong.
            !member_event
                .unsigned
                .prev_content
                .as_ref()
                .is_some_and(|prev_content| {
                    prev_content.membership == member_event.content.membership
                        && prev_content.displayname == member_event.content.displayname
                        && prev_content.avatar_url == member_event.content.avatar_url
                })
        }
        AnySyncTimelineEvent::State(state) => matches!(
            state,
            AnySyncStateEvent::RoomMember(_)
                | AnySyncStateEvent::RoomCreate(_)
                | AnySyncStateEvent::RoomEncryption(_)
                | AnySyncStateEvent::RoomThirdPartyInvite(_)
        ),
    }
}
