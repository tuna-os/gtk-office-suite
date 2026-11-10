use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};

mod name;
mod room_category_filter;

pub use self::name::SidebarSectionName;
use self::room_category_filter::RoomCategoryFilter;
use crate::{
    session::{
        Room, RoomCategory, RoomList, SessionSettings, VerificationList, room::HighlightFlags,
    },
    utils::ExpressionListModel,
};

mod imp {
    use std::{
        cell::{Cell, OnceCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SidebarSection)]
    pub struct SidebarSection {
        /// The source model of this section.
        #[property(get, set = Self::set_model, construct_only)]
        model: OnceCell<gio::ListModel>,
        /// The inner model of this section.
        inner_model: OnceCell<gio::ListModel>,
        /// The filter of this section.
        filter: RoomCategoryFilter,
        /// All the filters of this section: the category filter and the
        /// extra filters.
        pub(super) filters: gtk::EveryFilter,
        /// The name of this section.
        #[property(get, set = Self::set_name, construct_only, builder(SidebarSectionName::default()))]
        name: Cell<SidebarSectionName>,
        /// The display name of this section.
        #[property(get = Self::display_name)]
        display_name: PhantomData<String>,
        /// Whether this section is empty.
        #[property(get)]
        is_empty: Cell<bool>,
        /// Whether this section is expanded.
        #[property(get, set = Self::set_is_expanded, explicit_notify)]
        is_expanded: Cell<bool>,
        /// Whether any of the rooms in this section have unread notifications.
        #[property(get)]
        has_notifications: Cell<bool>,
        /// Total number of unread notifications over all the rooms in this
        /// section.
        #[property(get)]
        notification_count: Cell<u64>,
        /// Whether all the messages of all the rooms in this section are read.
        #[property(get)]
        is_read: Cell<bool>,
        /// The highlight state of the section.
        #[property(get)]
        highlight: Cell<HighlightFlags>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarSection {
        const NAME: &'static str = "SidebarSection";
        type Type = super::SidebarSection;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarSection {
        fn constructed(&self) {
            self.parent_constructed();

            let Some(settings) = self.session_settings() else {
                return;
            };

            let is_expanded = settings.is_section_expanded(self.name.get());
            self.set_is_expanded(is_expanded);
        }
    }

    impl ListModelImpl for SidebarSection {
        fn item_type(&self) -> glib::Type {
            glib::Object::static_type()
        }

        fn n_items(&self) -> u32 {
            self.inner_model().n_items()
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.inner_model().item(position)
        }
    }

    impl SidebarSection {
        /// The source model of this section.
        fn model(&self) -> &gio::ListModel {
            self.model.get().expect("model should be initialized")
        }

        /// Set the source model of this section.
        fn set_model(&self, model: gio::ListModel) {
            let model = self.model.get_or_init(|| model).clone();
            let obj = self.obj();

            // Special-case room lists so that they are sorted and in the right section.
            let inner_model = if model.is::<RoomList>() {
                // Filter the list to only show rooms for the proper category.
                self.filter
                    .set_expression(Some(Room::this_expression("category").upcast()));
                self.filters.append(self.filter.clone());
                let filter_model = gtk::FilterListModel::builder()
                    .model(&model)
                    .filter(&self.filters)
                    .watch_items(true)
                    .build();

                // Sort the list by activity.
                let room_latest_activity = Room::this_expression("latest-activity");
                let sorter = gtk::NumericSorter::builder()
                    .expression(&room_latest_activity)
                    .sort_order(gtk::SortType::Descending)
                    .build();

                let latest_activity_expr_model = ExpressionListModel::new();
                latest_activity_expr_model.set_expressions(vec![room_latest_activity.upcast()]);
                latest_activity_expr_model.set_model(Some(filter_model.clone()));

                let sort_model =
                    gtk::SortListModel::new(Some(latest_activity_expr_model), Some(sorter));

                // Watch for notification count and highlight changes in the filtered room list.
                let room_notification_count = Room::this_expression("notification-count");
                let room_highlight = Room::this_expression("highlight");
                let notification_and_highlight_expr_model = ExpressionListModel::new();
                notification_and_highlight_expr_model.set_expressions(vec![
                    room_notification_count.upcast(),
                    room_highlight.upcast(),
                ]);
                notification_and_highlight_expr_model.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |model, _, _, _| {
                        imp.update_notification_count_and_highlight(model);
                    }
                ));
                notification_and_highlight_expr_model.set_model(Some(filter_model));

                sort_model.upcast()
            } else {
                model
            };

            inner_model.connect_items_changed(clone!(
                #[weak]
                obj,
                move |model, pos, removed, added| {
                    obj.items_changed(pos, removed, added);
                    obj.imp().set_is_empty(model.n_items() == 0);
                }
            ));

            self.set_is_empty(inner_model.n_items() == 0);
            self.inner_model
                .set(inner_model)
                .expect("inner model should be uninitialized");
        }

        /// Update each of the properties if needed and emit corresponding
        /// signals.
        fn update_notification_count_and_highlight(&self, model: &ExpressionListModel) {
            // Aggregate properties over rooms in the section.

            // property:notification-count
            let mut notification_count = 0;
            // property:is-read
            let mut is_read = true;
            // property:highlight
            let mut highlight = HighlightFlags::empty();

            for room in model.iter::<glib::Object>() {
                if let Some(room) = room.ok().and_downcast::<Room>() {
                    notification_count += room.notification_count();
                    is_read &= room.is_read();
                    highlight |= room.highlight();
                }
            }

            // property:has-notification
            let has_notifications = notification_count > 0;

            if self.notification_count.get() != notification_count {
                self.notification_count.set(notification_count);
                self.obj().notify_notification_count();
            }

            if self.has_notifications.get() != has_notifications {
                self.has_notifications.set(has_notifications);
                self.obj().notify_has_notifications();
            }

            if self.highlight.get() != highlight {
                self.highlight.set(highlight);
                self.obj().notify_highlight();
            }

            if self.is_read.get() != is_read {
                self.is_read.set(is_read);
                self.obj().notify_is_read();
            }
        }

        /// The inner model of this section.
        fn inner_model(&self) -> &gio::ListModel {
            self.inner_model.get().unwrap()
        }

        /// Set the name of this section.
        fn set_name(&self, name: SidebarSectionName) {
            if let Some(room_category) = name.into_room_category() {
                self.filter.set_room_category(room_category);
            }

            self.name.set(name);
            self.obj().notify_name();
        }

        /// The display name of this section.
        fn display_name(&self) -> String {
            self.name.get().to_string()
        }

        /// Set whether this section is empty.
        fn set_is_empty(&self, is_empty: bool) {
            if is_empty == self.is_empty.get() {
                return;
            }

            self.is_empty.set(is_empty);
            self.obj().notify_is_empty();
        }

        /// Set whether this section is expanded.
        fn set_is_expanded(&self, expanded: bool) {
            if self.is_expanded.get() == expanded {
                return;
            }

            self.is_expanded.set(expanded);
            self.obj().notify_is_expanded();

            if let Some(settings) = self.session_settings() {
                settings.set_section_expanded(self.name.get(), expanded);
            }
        }

        /// The settings of the current session.
        fn session_settings(&self) -> Option<SessionSettings> {
            let model = self.model();
            let session = model
                .downcast_ref::<RoomList>()
                .and_then(RoomList::session)
                .or_else(|| {
                    model
                        .downcast_ref::<VerificationList>()
                        .and_then(VerificationList::session)
                })?;
            Some(session.settings())
        }
    }
}

glib::wrapper! {
    /// A list of items in the same section of the sidebar.
    pub struct SidebarSection(ObjectSubclass<imp::SidebarSection>)
        @implements gio::ListModel;
}

impl SidebarSection {
    /// Constructs a new `SidebarSection` with the given name and source model.
    pub fn new(name: SidebarSectionName, model: &impl IsA<gio::ListModel>) -> Self {
        glib::Object::builder()
            .property("name", name)
            .property("model", model)
            .build()
    }

    /// Add a filter to the rooms of this section, in addition to the filter
    /// of its category.
    ///
    /// This has no effect if the source model of this section is not a
    /// [`RoomList`].
    pub(crate) fn add_room_filter(&self, filter: &impl IsA<gtk::Filter>) {
        if self.model().is::<RoomList>() {
            self.imp().filters.append(filter.clone());
        }
    }

    /// Whether this section should be shown for the drag-n-drop of a room with
    /// the given category.
    pub(crate) fn visible_for_room_category(&self, source_category: Option<RoomCategory>) -> bool {
        if !self.is_empty() {
            return true;
        }

        source_category
            .zip(self.name().into_target_room_category())
            .is_some_and(|(source_category, target_category)| {
                source_category.can_change_to(target_category)
            })
    }
}
