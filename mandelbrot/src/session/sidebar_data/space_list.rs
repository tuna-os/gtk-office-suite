use std::{collections::HashMap, time::Duration};

use futures_util::StreamExt;
use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::{
    Client,
    deserialized_responses::RawSyncOrStrippedState,
    sync::{RoomUpdates, State},
};
use ruma::{
    OwnedRoomId, RoomId,
    events::{AnyRoomAccountDataEvent, SyncStateEvent, space::child::SpaceChildEventContent},
    serde::Raw,
};
use serde::Deserialize;
use tokio::task::AbortHandle;
use tokio_stream::wrappers::BroadcastStream;
use tracing::warn;

use super::{
    SidebarSpaceItem,
    space_graph::{JoinedSpace, SpaceGraph, SpaceVisibility, is_valid_order, relative_position},
};
use crate::{
    session::{Room, RoomList, SessionSettings},
    spawn, spawn_tokio,
};

/// The delay to wait for more changes before rebuilding the graph of spaces.
const REBUILD_DELAY: Duration = Duration::from_millis(250);
/// The type of the state events that define the children of a space.
const SPACE_CHILD_EVENT_TYPE: &str = "m.space.child";
/// The types of the room account data that define the order of a space, the
/// stable type first.
const SPACE_ORDER_EVENT_TYPES: [&str; 2] = ["m.space_order", "org.matrix.msc3230.space_order"];

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SidebarSpaceList)]
    pub struct SidebarSpaceList {
        /// The list of rooms of the session.
        #[property(get, set = Self::set_room_list, construct_only)]
        room_list: OnceCell<RoomList>,
        /// The "Home" item.
        home: OnceCell<SidebarSpaceItem>,
        /// The items of the top-level spaces.
        top_level: RefCell<Vec<SidebarSpaceItem>>,
        /// The graph of the joined spaces.
        ///
        /// Its top-level spaces are exactly the ones of the items of this
        /// list.
        graph: RefCell<SpaceGraph>,
        /// The navigation path requested by the user, from a top-level space
        /// to the selected space.
        ///
        /// It is kept even if it is not valid in the current graph, because
        /// spaces might still be loading or be rejoined.
        requested_path: RefCell<Vec<OwnedRoomId>>,
        /// The navigation path that is applied, resolved from the requested
        /// path.
        path: RefCell<Vec<OwnedRoomId>>,
        /// The selected space, or `None` if "Home" is selected.
        #[property(get)]
        current_space: RefCell<Option<Room>>,
        /// Whether the selected space is a subspace, so it is possible to go
        /// back to its parent.
        #[property(get)]
        can_go_back: Cell<bool>,
        /// Whether the user has joined any space.
        #[property(get)]
        has_spaces: Cell<bool>,
        /// The position of the selected item in this list.
        #[property(get = Self::selected_position)]
        selected_position: PhantomData<u32>,
        /// Whether the room search is active.
        ///
        /// When it is active, the rooms of all spaces are visible.
        search_active: Cell<bool>,
        /// The filter for rooms that are not spaces.
        pub(super) room_filter: gtk::CustomFilter,
        /// The filter for spaces.
        pub(super) subspace_filter: gtk::CustomFilter,
        /// Which rooms are visible in the selected space.
        visibility: RefCell<SpaceVisibility>,
        /// The source of the scheduled rebuild of the graph.
        rebuild_source: RefCell<Option<glib::SourceId>>,
        /// Whether the graph is being rebuilt.
        is_rebuilding: Cell<bool>,
        /// Whether the graph needs to be rebuilt again after the ongoing
        /// rebuild.
        needs_rebuild: Cell<bool>,
        /// The abort handle of the task watching room updates.
        updates_abort_handle: RefCell<Option<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarSpaceList {
        const NAME: &'static str = "SidebarSpaceList";
        type Type = super::SidebarSpaceList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarSpaceList {
        fn constructed(&self) {
            self.parent_constructed();

            self.room_filter.set_filter_func(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                true,
                move |item| {
                    let Some(room) = item.downcast_ref::<Room>() else {
                        return true;
                    };

                    imp.visibility.borrow().room_matches(
                        imp.search_active.get(),
                        room.room_id(),
                        room.predecessor_id().map(|id| &**id),
                    )
                }
            ));

            self.subspace_filter.set_filter_func(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                false,
                move |item| {
                    let Some(room) = item.downcast_ref::<Room>() else {
                        return false;
                    };

                    imp.visibility
                        .borrow()
                        .subspace_matches(imp.search_active.get(), room.room_id())
                }
            ));

            if let Some(settings) = self.session_settings() {
                self.requested_path.replace(settings.space_path());
            }
        }

        fn dispose(&self) {
            if let Some(source) = self.rebuild_source.take() {
                source.remove();
            }
            if let Some(handle) = self.updates_abort_handle.take() {
                handle.abort();
            }
        }
    }

    impl ListModelImpl for SidebarSpaceList {
        fn item_type(&self) -> glib::Type {
            SidebarSpaceItem::static_type()
        }

        fn n_items(&self) -> u32 {
            1 + self.top_level.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            if position == 0 {
                return Some(self.home().clone().upcast());
            }

            self.top_level
                .borrow()
                .get(position as usize - 1)
                .cloned()
                .and_upcast()
        }
    }

    impl SidebarSpaceList {
        /// The list of rooms of the session.
        fn room_list(&self) -> &RoomList {
            self.room_list
                .get()
                .expect("room list should be initialized")
        }

        /// Set the list of rooms of the session.
        fn set_room_list(&self, room_list: RoomList) {
            let room_list = self.room_list.get_or_init(|| room_list);

            self.home
                .set(SidebarSpaceItem::new(room_list, None))
                .expect("home item should be uninitialized");

            // Rooms that are added or removed might be spaces. This is also the
            // first trigger: the client of the session is ready once there are
            // rooms.
            room_list.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.schedule_rebuild();
                }
            ));
        }

        /// The "Home" item.
        fn home(&self) -> &SidebarSpaceItem {
            self.home.get().expect("home item should be initialized")
        }

        /// The settings of the current session.
        fn session_settings(&self) -> Option<SessionSettings> {
            Some(self.room_list().session()?.settings())
        }

        /// The position of the selected item in this list.
        fn selected_position(&self) -> u32 {
            self.graph.borrow().switcher_position(&self.path.borrow())
        }

        /// The current navigation path.
        pub(super) fn path(&self) -> Vec<OwnedRoomId> {
            self.path.borrow().clone()
        }

        /// The ID of the top-level space at the given position in this list,
        /// or `None` for "Home".
        pub(super) fn top_level_id_at(&self, position: u32) -> Option<OwnedRoomId> {
            self.graph.borrow().switcher_space_id(position).cloned()
        }

        /// The graph of the joined spaces.
        pub(super) fn graph(&self) -> std::cell::Ref<'_, SpaceGraph> {
            self.graph.borrow()
        }

        /// Request to navigate to the given path.
        ///
        /// The path is persisted in the settings of the session.
        pub(super) fn request_path(&self, path: Vec<OwnedRoomId>) {
            if let Some(settings) = self.session_settings() {
                settings.set_space_path(path.clone());
            }

            self.requested_path.replace(path);
            self.apply_path(false);
        }

        /// Apply the requested path, according to the current graph.
        fn apply_path(&self, graph_changed: bool) {
            let new_path = self
                .graph
                .borrow()
                .resolve_path(&self.requested_path.borrow());

            if !graph_changed && *self.path.borrow() == new_path {
                return;
            }

            let obj = self.obj();
            let _guard = obj.freeze_notify();

            let current_space = new_path.last().and_then(|id| self.room_list().get(id));
            if *self.current_space.borrow() != current_space {
                self.current_space.replace(current_space);
                obj.notify_current_space();
            }

            let can_go_back = new_path.len() > 1;
            if self.can_go_back.get() != can_go_back {
                self.can_go_back.set(can_go_back);
                obj.notify_can_go_back();
            }

            let visibility = SpaceVisibility::new(&self.graph.borrow(), &new_path);
            self.path.replace(new_path);

            let prev_visibility = self.visibility.replace(visibility);
            let (rooms_changed, subspaces_changed) = {
                let visibility = self.visibility.borrow();
                (
                    prev_visibility.rooms != visibility.rooms,
                    prev_visibility.subspaces != visibility.subspaces,
                )
            };

            if rooms_changed {
                self.room_filter.changed(gtk::FilterChange::Different);
            }
            if subspaces_changed {
                self.subspace_filter.changed(gtk::FilterChange::Different);
            }

            obj.notify_selected_position();
        }

        /// Set whether the room search is active.
        pub(super) fn set_search_active(&self, active: bool) {
            if self.search_active.get() == active {
                return;
            }

            self.search_active.set(active);

            let change = if active {
                gtk::FilterChange::LessStrict
            } else {
                gtk::FilterChange::MoreStrict
            };
            self.room_filter.changed(change);
            self.subspace_filter.changed(change);
        }

        /// Schedule a rebuild of the graph of spaces.
        ///
        /// Changes are batched during a short delay.
        fn schedule_rebuild(&self) {
            if self.rebuild_source.borrow().is_some() {
                return;
            }

            let source = glib::timeout_add_local_once(
                REBUILD_DELAY,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move || {
                        imp.rebuild_source.take();
                        imp.rebuild();
                    }
                ),
            );
            self.rebuild_source.replace(Some(source));
        }

        /// Rebuild the graph of spaces from the data in the store.
        fn rebuild(&self) {
            if self.is_rebuilding.get() {
                self.needs_rebuild.set(true);
                return;
            }
            let Some(session) = self.room_list().session() else {
                return;
            };
            let client = session.client();

            self.watch_room_updates(&client);
            self.is_rebuilding.set(true);

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    let handle = spawn_tokio!(async move { load_joined_spaces(&client).await });
                    let spaces = handle.await.expect("task was not aborted");

                    imp.set_graph(SpaceGraph::new(spaces));
                    imp.is_rebuilding.set(false);

                    if imp.needs_rebuild.take() {
                        imp.schedule_rebuild();
                    }
                }
            ));
        }

        /// Watch the room updates of the given client, to rebuild the graph
        /// when spaces change.
        fn watch_room_updates(&self, client: &Client) {
            if self.updates_abort_handle.borrow().is_some() {
                return;
            }

            let stream = BroadcastStream::new(client.subscribe_to_all_room_updates());
            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let client = client.clone();

            let fut = stream.for_each(move |updates| {
                // If we lagged behind, we might have missed changes to spaces.
                let spaces_changed = updates
                    .as_ref()
                    .map_or(true, |updates| room_updates_change_spaces(&client, updates));
                let obj_weak = obj_weak.clone();

                async move {
                    if !spaces_changed {
                        return;
                    }

                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        if let Some(obj) = obj_weak.upgrade() {
                            obj.imp().schedule_rebuild();
                        }
                    });
                }
            });

            let handle = spawn_tokio!(fut).abort_handle();
            self.updates_abort_handle.replace(Some(handle));
        }

        /// Set the graph of spaces.
        fn set_graph(&self, mut graph: SpaceGraph) {
            let room_list = self.room_list();

            let mut old_items = self
                .top_level
                .borrow()
                .iter()
                .filter_map(|item| Some((item.space()?.room_id().to_owned(), item.clone())))
                .collect::<HashMap<_, _>>();

            // Top-level spaces that are not in the room list yet cannot be
            // shown. Drop them from the graph so the positions in this list
            // match its top-level spaces. They will be added by the rebuild
            // triggered when they are added to the room list.
            let mut new_items = Vec::with_capacity(graph.top_level_spaces().len());
            for space_id in graph.top_level_spaces() {
                let item = old_items.remove(space_id).or_else(|| {
                    room_list
                        .get(space_id)
                        .map(|room| SidebarSpaceItem::new(room_list, Some(&room)))
                });

                if let Some(item) = item {
                    item.set_room_ids(Some(graph.descendants(space_id)));
                    new_items.push(item);
                }
            }
            graph.retain_top_level(|id| {
                new_items
                    .iter()
                    .any(|item| item.space().is_some_and(|space| *space.room_id() == **id))
            });

            let has_spaces = !new_items.is_empty();
            self.graph.replace(graph);

            if *self.top_level.borrow() != new_items {
                let removed = self.top_level.borrow().len() as u32;
                let added = new_items.len() as u32;
                self.top_level.replace(new_items);
                self.obj().items_changed(1, removed, added);
            }

            if self.has_spaces.get() != has_spaces {
                self.has_spaces.set(has_spaces);
                self.obj().notify_has_spaces();
            }

            self.apply_path(true);
        }
    }
}

glib::wrapper! {
    /// The list of items of the space switcher of the sidebar: "Home" and the
    /// top-level joined spaces.
    ///
    /// It also holds the navigation state between spaces, and provides the
    /// filters for the rooms of the sidebar according to the selected space.
    pub struct SidebarSpaceList(ObjectSubclass<imp::SidebarSpaceList>)
        @implements gio::ListModel;
}

impl SidebarSpaceList {
    /// Construct a new `SidebarSpaceList` for the given list of rooms.
    pub(crate) fn new(room_list: &RoomList) -> Self {
        glib::Object::builder()
            .property("room-list", room_list)
            .build()
    }

    /// The filter for the rooms of the sidebar that are not spaces.
    ///
    /// It only lets through the rooms that are in the selected space or in
    /// any of its subspaces.
    pub(crate) fn room_filter(&self) -> &gtk::CustomFilter {
        &self.imp().room_filter
    }

    /// The filter for the spaces of the sidebar.
    ///
    /// It only lets through the direct child spaces of the selected space.
    pub(crate) fn subspace_filter(&self) -> &gtk::CustomFilter {
        &self.imp().subspace_filter
    }

    /// Select the item at the given position.
    ///
    /// This selects the top level of the space, even if one of its subspaces
    /// was selected.
    pub(crate) fn select(&self, position: u32) {
        let space_id = self.imp().top_level_id_at(position);
        self.imp().request_path(space_id.into_iter().collect());
    }

    /// Select the item before or after the selected item, wrapping around.
    pub(crate) fn select_relative(&self, forward: bool) {
        let position = relative_position(self.selected_position(), self.n_items(), forward);
        self.select(position);
    }

    /// Navigate to the given space, which should be a joined space.
    ///
    /// If it is a child of the selected space, it is added to the current
    /// path, otherwise the shortest path to it is used.
    pub(crate) fn enter_space(&self, space: &Room) {
        let imp = self.imp();

        let path = imp.graph().path_into(&imp.path(), space.room_id());
        if let Some(path) = path {
            imp.request_path(path);
        }
    }

    /// Go back to the parent space of the selected subspace.
    pub(crate) fn go_back(&self) {
        let imp = self.imp();

        let mut path = imp.path();
        if path.len() > 1 {
            path.pop();
            imp.request_path(path);
        }
    }

    /// Make sure that the given room is visible in the sidebar.
    ///
    /// If the room is a joined space, navigate to it. If the room is not in
    /// the selected space, select "Home".
    pub(crate) fn show_room(&self, room: &Room) {
        let imp = self.imp();

        let path = imp.graph().path_to_show_room(
            &imp.path(),
            room.room_id(),
            room.predecessor_id().map(|id| &**id),
        );
        if let Some(path) = path {
            imp.request_path(path);
        }
    }

    /// Set whether the room search is active.
    ///
    /// When it is active, the rooms of all spaces are visible.
    pub(crate) fn set_search_active(&self, active: bool) {
        self.imp().set_search_active(active);
    }
}

/// Whether the given room updates might change the graph of spaces.
///
/// That is the case when a space is left, or when a joined space receives
/// `m.space.child` state events or changes to its order.
fn room_updates_change_spaces(client: &Client, updates: &RoomUpdates) -> bool {
    let is_space = |room_id: &RoomId| client.get_room(room_id).is_some_and(|room| room.is_space());

    if updates.left.keys().any(|room_id| is_space(room_id)) {
        return true;
    }

    updates.joined.iter().any(|(room_id, update)| {
        if !is_space(room_id) {
            return false;
        }

        let state_events = match &update.state {
            State::Before(events) | State::After(events) => events,
        };

        state_events
            .iter()
            .any(|event| is_space_child_type(event.get_field::<String>("type")))
            || update
                .timeline
                .events
                .iter()
                .any(|event| is_space_child_type(event.raw().get_field::<String>("type")))
            || update
                .account_data
                .iter()
                .any(|event| is_space_order_type(event.get_field::<String>("type")))
    })
}

/// Whether the given event type is the type of `m.space.child` events.
fn is_space_child_type(event_type: serde_json::Result<Option<String>>) -> bool {
    event_type.ok().flatten().as_deref() == Some(SPACE_CHILD_EVENT_TYPE)
}

/// Whether the given event type is one of the types of MSC3230 space order
/// account data.
fn is_space_order_type(event_type: serde_json::Result<Option<String>>) -> bool {
    event_type
        .ok()
        .flatten()
        .is_some_and(|event_type| SPACE_ORDER_EVENT_TYPES.contains(&event_type.as_str()))
}

/// Load the data of the joined spaces of the given client from the store.
async fn load_joined_spaces(client: &Client) -> Vec<JoinedSpace> {
    let mut spaces = Vec::new();

    for room in client.joined_space_rooms() {
        let children = match room
            .get_state_events_static::<SpaceChildEventContent>()
            .await
        {
            Ok(events) => space_children(events),
            Err(error) => {
                warn!(
                    "Could not load the children of space {}: {error}",
                    room.room_id()
                );
                Vec::new()
            }
        };

        let mut order_events = Vec::with_capacity(SPACE_ORDER_EVENT_TYPES.len());
        for event_type in SPACE_ORDER_EVENT_TYPES {
            match room.account_data(event_type.into()).await {
                Ok(event) => order_events.push(event),
                Err(error) => {
                    warn!(
                        "Could not load the order of space {}: {error}",
                        room.room_id()
                    );
                    order_events.push(None);
                }
            }
        }

        spaces.push(JoinedSpace {
            room_id: room.room_id().to_owned(),
            children,
            order: space_order(order_events),
        });
    }

    spaces
}

/// The IDs of the children of a space from its `m.space.child` state events.
///
/// Redacted events and events without `via` are ignored, since they are not
/// valid children per the Matrix specification.
fn space_children(events: Vec<RawSyncOrStrippedState<SpaceChildEventContent>>) -> Vec<OwnedRoomId> {
    events
        .into_iter()
        .filter_map(|event| {
            let RawSyncOrStrippedState::Sync(raw) = event else {
                return None;
            };

            match raw.deserialize() {
                Ok(SyncStateEvent::Original(event)) if !event.content.via.is_empty() => {
                    Some(event.state_key)
                }
                _ => None,
            }
        })
        .collect()
}

/// The MSC3230 order of a space from its room account data, in the order of
/// [`SPACE_ORDER_EVENT_TYPES`].
///
/// The first valid order is used.
fn space_order(events: Vec<Option<Raw<AnyRoomAccountDataEvent>>>) -> Option<String> {
    #[derive(Deserialize)]
    struct SpaceOrderEvent {
        content: SpaceOrderEventContent,
    }

    #[derive(Deserialize)]
    struct SpaceOrderEventContent {
        order: String,
    }

    events
        .into_iter()
        .flatten()
        .filter_map(|raw| raw.deserialize_as_unchecked::<SpaceOrderEvent>().ok())
        .map(|event| event.content.order)
        .find(|order| is_valid_order(order))
}

#[cfg(test)]
mod tests {
    use ruma::owned_room_id;
    use serde_json::{Value as JsonValue, json};

    use super::*;

    fn raw<T>(value: &JsonValue) -> Raw<T> {
        Raw::from_json_string(value.to_string()).unwrap()
    }

    fn child_event(
        child: &str,
        content: &JsonValue,
    ) -> RawSyncOrStrippedState<SpaceChildEventContent> {
        RawSyncOrStrippedState::Sync(raw(&json!({
            "type": "m.space.child",
            "state_key": child,
            "content": content,
            "event_id": format!("$event-{child}"),
            "sender": "@admin:x",
            "origin_server_ts": 1,
        })))
    }

    fn order_event(event_type: &str, order: &JsonValue) -> Raw<AnyRoomAccountDataEvent> {
        raw(&json!({
            "type": event_type,
            "content": { "order": order },
        }))
    }

    #[test]
    fn space_children_ignores_invalid_children() {
        let redacted = RawSyncOrStrippedState::Sync(raw(&json!({
            "type": "m.space.child",
            "state_key": "!redacted:x",
            "content": {},
            "event_id": "$redacted",
            "sender": "@admin:x",
            "origin_server_ts": 1,
            "unsigned": {
                "redacted_because": {
                    "type": "m.room.redaction",
                    "redacts": "$redacted",
                    "content": {},
                    "event_id": "$redaction",
                    "sender": "@admin:x",
                    "origin_server_ts": 2,
                },
            },
        })));
        let stripped = RawSyncOrStrippedState::Stripped(raw(&json!({
            "type": "m.space.child",
            "state_key": "!stripped:x",
            "content": { "via": ["x"] },
            "sender": "@admin:x",
        })));

        let children = space_children(vec![
            child_event("!valid:x", &json!({ "via": ["x"] })),
            child_event("!no-via:x", &json!({ "via": [] })),
            child_event("!missing-via:x", &json!({})),
            child_event("!suggested:x", &json!({ "via": ["x"], "suggested": true })),
            redacted,
            stripped,
        ]);

        assert_eq!(
            children,
            vec![owned_room_id!("!valid:x"), owned_room_id!("!suggested:x")]
        );
    }

    #[test]
    fn space_order_prefers_valid_stable_order() {
        let stable = order_event(SPACE_ORDER_EVENT_TYPES[0], &json!("a"));
        let unstable = order_event(SPACE_ORDER_EVENT_TYPES[1], &json!("b"));
        assert_eq!(
            space_order(vec![Some(stable), Some(unstable)]),
            Some("a".to_owned())
        );
    }

    #[test]
    fn space_order_falls_back_to_unstable_order() {
        let unstable = order_event(SPACE_ORDER_EVENT_TYPES[1], &json!("b"));
        assert_eq!(
            space_order(vec![None, Some(unstable)]),
            Some("b".to_owned())
        );
    }

    #[test]
    fn invalid_stable_order_does_not_shadow_unstable_order() {
        let too_long = order_event(SPACE_ORDER_EVENT_TYPES[0], &json!("a".repeat(51)));
        let unstable = order_event(SPACE_ORDER_EVENT_TYPES[1], &json!("b"));
        assert_eq!(
            space_order(vec![Some(too_long), Some(unstable)]),
            Some("b".to_owned())
        );

        let not_a_string = order_event(SPACE_ORDER_EVENT_TYPES[0], &json!(1));
        let unstable = order_event(SPACE_ORDER_EVENT_TYPES[1], &json!("b"));
        assert_eq!(
            space_order(vec![Some(not_a_string), Some(unstable)]),
            Some("b".to_owned())
        );
    }

    #[test]
    fn space_order_without_valid_order() {
        assert_eq!(space_order(vec![None, None]), None);

        let non_ascii = order_event(SPACE_ORDER_EVENT_TYPES[1], &json!("\u{e9}"));
        assert_eq!(space_order(vec![None, Some(non_ascii)]), None);
    }

    #[test]
    fn space_event_types() {
        assert!(is_space_child_type(Ok(Some("m.space.child".to_owned()))));
        assert!(!is_space_child_type(Ok(Some("m.space.parent".to_owned()))));
        assert!(!is_space_child_type(Ok(Some("m.room.message".to_owned()))));
        assert!(!is_space_child_type(Ok(None)));

        assert!(is_space_order_type(Ok(Some("m.space_order".to_owned()))));
        assert!(is_space_order_type(Ok(Some(
            "org.matrix.msc3230.space_order".to_owned()
        ))));
        assert!(!is_space_order_type(Ok(Some("m.tag".to_owned()))));
        assert!(!is_space_order_type(Ok(None)));
    }
}
