use std::sync::Arc;

use futures_util::{StreamExt, pin_mut};
use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk_ui::{
    eyeball_im::VectorDiff,
    spaces::{SpaceRoom, SpaceRoomList, room_list::SpaceRoomListPaginationState},
};
use ruma::OwnedRoomId;
use tokio::task::AbortHandle;
use tracing::warn;

mod child;

pub(crate) use self::child::SpaceHierarchyChild;
use crate::{
    session::Session,
    spawn, spawn_tokio,
    utils::{LoadingState, TokioDrop},
};

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::SpaceHierarchy)]
    pub struct SpaceHierarchy {
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The ID of the space of this hierarchy.
        space_id: OnceCell<OwnedRoomId>,
        /// The SDK room list of the space.
        room_list: RefCell<Option<TokioDrop<Arc<SpaceRoomList>>>>,
        /// The room of the space of this hierarchy.
        #[property(get)]
        space: RefCell<Option<SpaceHierarchyChild>>,
        /// The children of the space.
        list: RefCell<Vec<SpaceHierarchyChild>>,
        /// The loading state of the hierarchy.
        #[property(get, builder(LoadingState::default()))]
        loading_state: Cell<LoadingState>,
        /// Whether all the children of the space have been loaded.
        #[property(get)]
        complete: Cell<bool>,
        /// The abort handle for the task watching the updates of the
        /// children.
        updates_abort_handle: RefCell<Option<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SpaceHierarchy {
        const NAME: &'static str = "SpaceHierarchy";
        type Type = super::SpaceHierarchy;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for SpaceHierarchy {
        fn dispose(&self) {
            if let Some(abort_handle) = self.updates_abort_handle.take() {
                abort_handle.abort();
            }
        }
    }

    impl ListModelImpl for SpaceHierarchy {
        fn item_type(&self) -> glib::Type {
            SpaceHierarchyChild::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list
                .borrow()
                .get(position as usize)
                .cloned()
                .and_upcast()
        }
    }

    impl SpaceHierarchy {
        /// Set the current session.
        fn set_session(&self, session: &Session) {
            self.session.set(Some(session));
        }

        /// Initialize this hierarchy for the space with the given ID.
        pub(super) fn init(&self, space_id: OwnedRoomId) {
            self.space_id
                .set(space_id)
                .expect("space ID should be uninitialized");

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.setup().await;
                }
            ));
        }

        /// Create the SDK room list and watch its updates.
        async fn setup(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let client = session.client();
            let space_id = self
                .space_id
                .get()
                .expect("space ID should be initialized")
                .clone();

            let handle = spawn_tokio!(async move {
                let room_list = Arc::new(SpaceRoomList::new(client, space_id).await);
                let subscription = room_list.subscribe_to_room_updates().await;
                (room_list, subscription)
            });
            let (room_list, (initial_rooms, updates_stream)) =
                handle.await.expect("task was not aborted");

            self.update_space(room_list.space());

            if !initial_rooms.is_empty() {
                self.apply_diff_list(vec![VectorDiff::Reset {
                    values: initial_rooms,
                }]);
            }

            // Watch the updates of the children. The task ends when the SDK
            // room list is dropped.
            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let updates_handle = spawn_tokio!(async move {
                pin_mut!(updates_stream);
                while let Some(diff_list) = updates_stream.next().await {
                    let obj_weak = obj_weak.clone();
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        if let Some(obj) = obj_weak.upgrade() {
                            obj.imp().apply_diff_list(diff_list);
                        }
                    });
                }
            });
            self.updates_abort_handle
                .replace(Some(updates_handle.abort_handle()));

            self.room_list.replace(Some(TokioDrop::new(room_list)));

            // Load the first page.
            self.load_more();
        }

        /// The SDK room list, if it is initialized.
        fn sdk_room_list(&self) -> Option<Arc<SpaceRoomList>> {
            self.room_list
                .borrow()
                .as_ref()
                .map(|room_list| Arc::clone(room_list))
        }

        /// Load the next page of children, if any.
        pub(super) fn load_more(&self) {
            if self.loading_state.get() == LoadingState::Loading || self.complete.get() {
                return;
            }
            let Some(room_list) = self.sdk_room_list() else {
                // The SDK room list is still being initialized, it will load
                // the first page when it is done.
                return;
            };

            self.set_loading_state(LoadingState::Loading);

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    let room_list_clone = room_list.clone();
                    let handle = spawn_tokio!(async move {
                        let result = room_list_clone.paginate().await;
                        let pagination_state = room_list_clone.pagination_state();
                        let space = room_list_clone.space();
                        (result, pagination_state, space)
                    });
                    let (result, pagination_state, space) =
                        handle.await.expect("task was not aborted");

                    imp.update_space(space);

                    if let SpaceRoomListPaginationState::Idle { end_reached } = pagination_state {
                        imp.set_complete(end_reached);
                    }

                    match result {
                        Ok(()) => {
                            imp.set_loading_state(LoadingState::Ready);
                        }
                        Err(error) => {
                            warn!("Could not load space hierarchy: {error}");

                            // Only switch to the error state if we do not have
                            // anything to show.
                            if imp.list.borrow().is_empty() {
                                imp.set_loading_state(LoadingState::Error);
                            } else {
                                imp.set_loading_state(LoadingState::Ready);
                            }
                        }
                    }
                }
            ));
        }

        /// Reload the hierarchy from the beginning.
        pub(super) fn reload(&self) {
            if self.loading_state.get() == LoadingState::Loading {
                return;
            }
            let Some(room_list) = self.sdk_room_list() else {
                return;
            };

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    let room_list_clone = room_list.clone();
                    let handle = spawn_tokio!(async move { room_list_clone.reset().await });
                    handle.await.expect("task was not aborted");

                    imp.set_complete(false);
                    imp.load_more();
                }
            ));
        }

        /// Update the room of the space with the given data.
        fn update_space(&self, data: Option<SpaceRoom>) {
            let Some(data) = data else {
                return;
            };
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let space = self.space.borrow().clone();
            if let Some(space) = space {
                space.update_with(&data);
            } else {
                self.space
                    .replace(Some(SpaceHierarchyChild::new(&session, &data)));
                self.obj().notify_space();
            }
        }

        /// Apply the given list of diffs to the list of children.
        pub(super) fn apply_diff_list(&self, diff_list: Vec<VectorDiff<SpaceRoom>>) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            for diff in diff_list {
                self.apply_diff(&session, diff);
            }
        }

        /// Apply the given diff to the list of children.
        fn apply_diff(&self, session: &Session, diff: VectorDiff<SpaceRoom>) {
            let obj = self.obj();

            match diff {
                VectorDiff::Append { values } => {
                    let pos = self.list.borrow().len() as u32;
                    let added = values.len() as u32;
                    self.list.borrow_mut().extend(
                        values
                            .iter()
                            .map(|data| SpaceHierarchyChild::new(session, data)),
                    );
                    obj.items_changed(pos, 0, added);
                }
                VectorDiff::Clear => {
                    let removed = self.list.borrow().len() as u32;
                    self.list.borrow_mut().clear();
                    if removed > 0 {
                        obj.items_changed(0, removed, 0);
                    }
                }
                VectorDiff::PushFront { value } => {
                    self.list
                        .borrow_mut()
                        .insert(0, SpaceHierarchyChild::new(session, &value));
                    obj.items_changed(0, 0, 1);
                }
                VectorDiff::PushBack { value } => {
                    let pos = self.list.borrow().len() as u32;
                    self.list
                        .borrow_mut()
                        .push(SpaceHierarchyChild::new(session, &value));
                    obj.items_changed(pos, 0, 1);
                }
                VectorDiff::PopFront => {
                    self.remove_at(0);
                }
                VectorDiff::PopBack => {
                    let len = self.list.borrow().len();
                    self.remove_at(len.saturating_sub(1));
                }
                VectorDiff::Insert { index, value } => {
                    let len = self.list.borrow().len();
                    let index = index.min(len);
                    self.list
                        .borrow_mut()
                        .insert(index, SpaceHierarchyChild::new(session, &value));
                    obj.items_changed(index as u32, 0, 1);
                }
                VectorDiff::Set { index, value } => {
                    self.set_at(session, index, &value);
                }
                VectorDiff::Remove { index } => {
                    self.remove_at(index);
                }
                VectorDiff::Truncate { length } => {
                    let old_len = self.list.borrow().len();
                    if length < old_len {
                        self.list.borrow_mut().truncate(length);
                        obj.items_changed(length as u32, (old_len - length) as u32, 0);
                    }
                }
                VectorDiff::Reset { values } => {
                    let old_len = self.list.borrow().len() as u32;
                    let new_list = values
                        .iter()
                        .map(|data| SpaceHierarchyChild::new(session, data))
                        .collect::<Vec<_>>();
                    let new_len = new_list.len() as u32;
                    self.list.replace(new_list);
                    obj.items_changed(0, old_len, new_len);
                }
            }
        }

        /// Update the child at the given index with the given data.
        fn set_at(&self, session: &Session, index: usize, data: &SpaceRoom) {
            let child = self.list.borrow().get(index).cloned();

            if let Some(child) = child {
                if child.room_id() == &data.room_id {
                    // Update the existing child, its properties will notify
                    // the changes.
                    child.update_with(data);
                } else {
                    self.list.borrow_mut()[index] = SpaceHierarchyChild::new(session, data);
                    self.obj().items_changed(index as u32, 1, 1);
                }
            } else {
                warn!("Could not update item at unknown index {index} of space hierarchy");
            }
        }

        /// Remove the child at the given index.
        fn remove_at(&self, index: usize) {
            if index < self.list.borrow().len() {
                self.list.borrow_mut().remove(index);
                self.obj().items_changed(index as u32, 1, 0);
            } else {
                warn!("Could not remove item at unknown index {index} of space hierarchy");
            }
        }

        /// Set the loading state of the hierarchy.
        fn set_loading_state(&self, loading_state: LoadingState) {
            if self.loading_state.get() == loading_state {
                return;
            }

            self.loading_state.set(loading_state);
            self.obj().notify_loading_state();
        }

        /// Set whether all the children of the space have been loaded.
        fn set_complete(&self, complete: bool) {
            if self.complete.get() == complete {
                return;
            }

            self.complete.set(complete);
            self.obj().notify_complete();
        }
    }
}

glib::wrapper! {
    /// The list of children in the hierarchy of a space, loaded from the
    /// `/hierarchy` endpoint (MSC2946).
    ///
    /// This is a wrapper around the `SpaceRoomList` API of the Matrix Rust
    /// SDK. The children are loaded page by page with [`SpaceHierarchy::load_more()`].
    pub struct SpaceHierarchy(ObjectSubclass<imp::SpaceHierarchy>)
        @implements gio::ListModel;
}

impl SpaceHierarchy {
    /// Construct a new `SpaceHierarchy` for the space with the given ID.
    pub(crate) fn new(session: &Session, space_id: OwnedRoomId) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("session", session)
            .build();
        obj.imp().init(space_id);
        obj
    }

    /// Load the next page of children, if any.
    pub(crate) fn load_more(&self) {
        self.imp().load_more();
    }

    /// Reload the hierarchy from the beginning.
    pub(crate) fn reload(&self) {
        self.imp().reload();
    }

    /// Whether this hierarchy is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.n_items() == 0
    }
}
