use std::collections::HashSet;

use gettextrs::gettext;
use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};
use ruma::OwnedRoomId;

use super::space_graph::{UnreadSummary, contains_room};
use crate::{
    prelude::*,
    session::{HighlightFlags, Room, RoomList},
    utils::ExpressionListModel,
};

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::SidebarSpaceItem)]
    pub struct SidebarSpaceItem {
        /// The space of this item, or `None` for the "Home" item that
        /// contains all the rooms.
        #[property(get, construct_only, nullable)]
        space: RefCell<Option<Room>>,
        /// Whether this is the "Home" item.
        #[property(get = Self::is_home)]
        is_home: PhantomData<bool>,
        /// The display name of this item.
        #[property(get = Self::display_name)]
        display_name: PhantomData<String>,
        /// The total number of unread notifications in the rooms of this item.
        #[property(get)]
        notification_count: Cell<u64>,
        /// Whether any of the rooms of this item have unread notifications.
        #[property(get)]
        has_notifications: Cell<bool>,
        /// The aggregated highlight state of the rooms of this item.
        #[property(get)]
        highlight: Cell<HighlightFlags>,
        /// Whether all the rooms of this item are read.
        #[property(get)]
        is_read: Cell<bool>,
        /// The IDs of the rooms in this item, or `None` for all the rooms.
        room_ids: RefCell<Option<HashSet<OwnedRoomId>>>,
        /// The filter of the rooms in this item.
        filter: gtk::CustomFilter,
        /// The model watching the unread state of the rooms in this item.
        expr_model: ExpressionListModel,
        /// The handler of the display name of the space.
        display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    impl Default for SidebarSpaceItem {
        fn default() -> Self {
            Self {
                space: Default::default(),
                is_home: PhantomData,
                display_name: PhantomData,
                notification_count: Default::default(),
                has_notifications: Default::default(),
                highlight: Default::default(),
                is_read: Cell::new(true),
                room_ids: Default::default(),
                filter: Default::default(),
                expr_model: Default::default(),
                display_name_handler: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarSpaceItem {
        const NAME: &'static str = "SidebarSpaceItem";
        type Type = super::SidebarSpaceItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarSpaceItem {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            if let Some(space) = obj.space() {
                let handler = space.connect_display_name_notify(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.notify_display_name();
                    }
                ));
                self.display_name_handler.replace(Some(handler));
            }

            self.filter.set_filter_func(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                false,
                move |item| {
                    let Some(room) = item.downcast_ref::<Room>() else {
                        return false;
                    };

                    imp.room_ids.borrow().as_ref().is_none_or(|room_ids| {
                        contains_room(
                            room_ids,
                            room.room_id(),
                            room.predecessor_id().map(|id| &**id),
                        )
                    })
                }
            ));

            self.expr_model.set_expressions(vec![
                Room::this_expression("category").upcast(),
                Room::this_expression("notification-count").upcast(),
                Room::this_expression("highlight").upcast(),
                Room::this_expression("is-read").upcast(),
            ]);
            self.expr_model.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |model, _, _, _| {
                    imp.update_unread_state(model);
                }
            ));
        }

        fn dispose(&self) {
            if let Some(handler) = self.display_name_handler.take()
                && let Some(space) = self.space.borrow().as_ref()
            {
                space.disconnect(handler);
            }

            // Stop watching the rooms.
            self.expr_model.set_model(None::<gio::ListModel>);
        }
    }

    impl SidebarSpaceItem {
        /// Set the list of rooms to watch.
        pub(super) fn set_room_list(&self, room_list: &RoomList) {
            let filter_model =
                gtk::FilterListModel::new(Some(room_list.clone()), Some(self.filter.clone()));
            self.expr_model.set_model(Some(filter_model));
        }

        /// Whether this is the "Home" item.
        fn is_home(&self) -> bool {
            self.space.borrow().is_none()
        }

        /// The display name of this item.
        fn display_name(&self) -> String {
            match &*self.space.borrow() {
                Some(space) => space.display_name(),
                // Translators: The item of the space switcher that shows all the rooms.
                None => gettext("Home"),
            }
        }

        /// Set the IDs of the rooms in this item.
        pub(super) fn set_room_ids(&self, room_ids: Option<HashSet<OwnedRoomId>>) {
            if *self.room_ids.borrow() == room_ids {
                return;
            }

            self.room_ids.replace(room_ids);
            self.filter.changed(gtk::FilterChange::Different);
        }

        /// Update the aggregated unread state of the rooms of this item.
        fn update_unread_state(&self, model: &ExpressionListModel) {
            let mut summary = UnreadSummary::default();

            for room in model.iter::<glib::Object>() {
                if let Some(room) = room.ok().and_downcast::<Room>() {
                    summary.add(
                        room.category(),
                        room.notification_count(),
                        room.highlight(),
                        room.is_read(),
                    );
                }
            }

            let obj = self.obj();
            let _guard = obj.freeze_notify();

            if self.notification_count.get() != summary.notification_count {
                self.notification_count.set(summary.notification_count);
                obj.notify_notification_count();
            }

            if self.has_notifications.get() != summary.has_notifications() {
                self.has_notifications.set(summary.has_notifications());
                obj.notify_has_notifications();
            }

            if self.highlight.get() != summary.highlight {
                self.highlight.set(summary.highlight);
                obj.notify_highlight();
            }

            if self.is_read.get() != summary.is_read {
                self.is_read.set(summary.is_read);
                obj.notify_is_read();
            }
        }
    }
}

glib::wrapper! {
    /// An item of the space switcher of the sidebar: a space, or "Home".
    ///
    /// It aggregates the unread state of all the rooms it contains, including
    /// the rooms of its subspaces.
    pub struct SidebarSpaceItem(ObjectSubclass<imp::SidebarSpaceItem>);
}

impl SidebarSpaceItem {
    /// Construct a new `SidebarSpaceItem` for the given space, or "Home" if
    /// it is `None`, watching the rooms of the given list.
    pub(crate) fn new(room_list: &RoomList, space: Option<&Room>) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("space", space)
            .build();
        obj.imp().set_room_list(room_list);
        obj
    }

    /// Set the IDs of the rooms in this item.
    ///
    /// `None` means that it contains all the rooms.
    pub(crate) fn set_room_ids(&self, room_ids: Option<HashSet<OwnedRoomId>>) {
        self.imp().set_room_ids(room_ids);
    }
}
