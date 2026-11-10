use std::ops::ControlFlow;

use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use matrix_sdk::{
    room::MessagesOptions,
    ruma::{
        api::client::filter::{RoomEventFilter, UrlFilter},
        assign,
        events::MessageLikeEventType,
        uint,
    },
};
use tracing::error;

use super::HistoryViewerEvent;
use crate::{
    components::LoadingRow,
    session::Room,
    spawn_tokio,
    utils::{LoadingState, SingleItemListModel},
};

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::HistoryViewerTimeline)]
    pub struct HistoryViewerTimeline {
        /// The room that this timeline belongs to.
        #[property(get, construct_only)]
        room: OnceCell<Room>,
        /// The loading state of this timeline.
        #[property(get, builder(LoadingState::default()))]
        state: Cell<LoadingState>,
        /// Whether we have reached the start of the timeline.
        #[property(get)]
        has_reached_start: Cell<bool>,
        list: RefCell<Vec<HistoryViewerEvent>>,
        last_token: RefCell<Option<String>>,
        /// A wrapper model with an extra loading item at the end when
        /// applicable.
        ///
        /// The loading item is a [`LoadingRow`], all other items are
        /// [`HistoryViewerEvent`]s.
        model_with_loading_item: OnceCell<gtk::FlattenListModel>,
        /// A model containing a [`LoadingRow`] when the timeline is loading.
        loading_item_model: OnceCell<SingleItemListModel>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HistoryViewerTimeline {
        const NAME: &'static str = "HistoryViewerTimeline";
        type Type = super::HistoryViewerTimeline;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for HistoryViewerTimeline {}

    impl ListModelImpl for HistoryViewerTimeline {
        fn item_type(&self) -> glib::Type {
            HistoryViewerEvent::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let list = self.list.borrow();
            list.get(position as usize)
                .map(|o| o.clone().upcast::<glib::Object>())
        }
    }

    impl HistoryViewerTimeline {
        /// The room that this timeline belongs to.
        fn room(&self) -> &Room {
            self.room.get().expect("room should be initialized")
        }

        /// Set the loading state of the timeline.
        fn set_state(&self, state: LoadingState) {
            if state == self.state.get() {
                return;
            }

            self.state.set(state);

            self.loading_item_model()
                .set_is_hidden(state != LoadingState::Loading);

            self.obj().notify_state();
        }

        /// Set whether we have reached the start of the timeline.
        pub(super) fn set_has_reached_start(&self, has_reached_start: bool) {
            if self.has_reached_start.get() == has_reached_start {
                return;
            }

            self.has_reached_start.set(has_reached_start);
            self.obj().notify_has_reached_start();
        }

        /// Append the given batch to the timeline.
        pub(super) fn append(&self, batch: Vec<HistoryViewerEvent>) {
            if batch.is_empty() {
                return;
            }

            let index = self.n_items();
            let added = batch.len();

            self.list.borrow_mut().extend(batch);

            self.obj().items_changed(index, 0, added as u32);
        }

        /// A model containing a [`LoadingRow`] when the timeline is loading.
        pub(super) fn loading_item_model(&self) -> &SingleItemListModel {
            self.loading_item_model.get_or_init(|| {
                let model = SingleItemListModel::new(Some(&LoadingRow::new()));
                model.set_is_hidden(true);
                model
            })
        }

        /// A wrapper model with an extra loading item at the end when
        /// applicable.
        ///
        /// The loading item is a [`LoadingRow`], all other items are
        /// [`HistoryViewerEvent`]s.
        pub(super) fn model_with_loading_item(&self) -> &gtk::FlattenListModel {
            self.model_with_loading_item.get_or_init(|| {
                let wrapper_model = gio::ListStore::new::<glib::Object>();
                wrapper_model.append(&*self.obj());
                wrapper_model.append(self.loading_item_model());

                gtk::FlattenListModel::new(Some(wrapper_model))
            })
        }

        /// Load more events in the timeline until the given function tells us
        /// to stop.
        pub(super) async fn load<F>(&self, continue_fn: F)
        where
            F: Fn() -> ControlFlow<()>,
        {
            if self.state.get() == LoadingState::Loading || self.has_reached_start.get() {
                return;
            }

            self.set_state(LoadingState::Loading);

            loop {
                if !self.load_inner().await {
                    return;
                }

                if continue_fn().is_break() {
                    self.set_state(LoadingState::Ready);
                    return;
                }
            }
        }

        /// Load more events in the timeline.
        ///
        /// Returns `true` if more events can be loaded.
        async fn load_inner(&self) -> bool {
            let room = self.room();
            let matrix_room = room.matrix_room().clone();
            let last_token = self.last_token.borrow().clone();
            let is_encrypted = room.is_encrypted();

            let handle = spawn_tokio!(async move {
                // If the room is encrypted, the messages content cannot be filtered with URLs
                let filter = if is_encrypted {
                    let filter_types = vec![
                        MessageLikeEventType::RoomEncrypted.to_string(),
                        MessageLikeEventType::RoomMessage.to_string(),
                    ];
                    assign!(RoomEventFilter::default(), {
                        types: Some(filter_types),
                    })
                } else {
                    let filter_types = vec![MessageLikeEventType::RoomMessage.to_string()];
                    assign!(RoomEventFilter::default(), {
                        types: Some(filter_types),
                        url_filter: Some(UrlFilter::EventsWithUrl),
                    })
                };
                let options = assign!(MessagesOptions::backward().from(last_token.as_deref()), {
                    limit: uint!(20),
                    filter,
                });

                matrix_room.messages(options).await
            });

            match handle.await.expect("task was not aborted") {
                Ok(events) => {
                    if let Some(end_token) = events.end {
                        self.last_token.replace(Some(end_token));

                        let events = events
                            .chunk
                            .into_iter()
                            .filter_map(|event| HistoryViewerEvent::try_new(room, &event))
                            .collect();

                        self.append(events);
                        true
                    } else {
                        self.set_has_reached_start(true);
                        self.set_state(LoadingState::Ready);
                        false
                    }
                }
                Err(error) => {
                    error!("Could not load history viewer timeline events: {error}");
                    self.set_state(LoadingState::Error);
                    false
                }
            }
        }
    }
}

glib::wrapper! {
    /// A room timeline for the history viewers.
    pub struct HistoryViewerTimeline(ObjectSubclass<imp::HistoryViewerTimeline>)
        @implements gio::ListModel;
}

impl HistoryViewerTimeline {
    pub fn new(room: &Room) -> Self {
        glib::Object::builder().property("room", room).build()
    }

    /// Load more events in the timeline until the given function tells us to
    /// stop.
    pub(crate) async fn load<F>(&self, continue_fn: F)
    where
        F: Fn() -> ControlFlow<()>,
    {
        self.imp().load(continue_fn).await;
    }

    /// This model with an extra loading item at the end when applicable.
    ///
    /// The loading item is a [`LoadingRow`], all other items are
    /// [`HistoryViewerEvent`]s.
    pub(crate) fn with_loading_item(&self) -> &gio::ListModel {
        self.imp().model_with_loading_item().upcast_ref()
    }
}
