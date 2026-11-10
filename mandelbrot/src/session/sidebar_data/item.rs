use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};

use super::{SidebarIconItem, SidebarSection};
use crate::{
    session::RoomCategory,
    utils::{BoundConstructOnlyObject, SingleItemListModel},
};

mod imp {
    use std::cell::{Cell, OnceCell};

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::SidebarItem)]
    pub struct SidebarItem {
        /// The item wrapped by this `SidebarItem`.
        #[property(get, set = Self::set_inner_item, construct_only)]
        inner_item: BoundConstructOnlyObject<glib::Object>,
        /// Whether this item is visible.
        #[property(get)]
        is_visible: Cell<bool>,
        /// Whether to inhibit the expanded state.
        ///
        /// It means that all the sections will be expanded regardless of
        /// their "is-expanded" property.
        #[property(get, set = Self::set_inhibit_expanded, explicit_notify)]
        inhibit_expanded: Cell<bool>,
        is_visible_filter: gtk::CustomFilter,
        is_expanded_filter: gtk::CustomFilter,
        /// The inner model.
        model: OnceCell<gtk::FilterListModel>,
    }

    impl Default for SidebarItem {
        fn default() -> Self {
            Self {
                inner_item: Default::default(),
                is_visible: Cell::new(true),
                inhibit_expanded: Default::default(),
                is_visible_filter: Default::default(),
                is_expanded_filter: Default::default(),
                model: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarItem {
        const NAME: &'static str = "SidebarItem";
        type Type = super::SidebarItem;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarItem {}

    impl ListModelImpl for SidebarItem {
        fn item_type(&self) -> glib::Type {
            glib::Object::static_type()
        }

        fn n_items(&self) -> u32 {
            self.model.get().unwrap().n_items()
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.model.get().unwrap().item(position)
        }
    }

    impl SidebarItem {
        /// Set the item wrapped by this `SidebarItem`.
        fn set_inner_item(&self, item: glib::Object) {
            let mut handlers = Vec::new();

            let inner_model = if let Some(section) = item.downcast_ref::<SidebarSection>() {
                // Create a list model to have an item for the section itself.
                let section_model = SingleItemListModel::new(Some(section));

                // Filter the children depending on whether the section is expanded or not.
                self.is_expanded_filter.set_filter_func(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[weak]
                    section,
                    #[upgrade_or]
                    false,
                    move |_| imp.inhibit_expanded.get() || section.is_expanded()
                ));
                let children_model = gtk::FilterListModel::new(
                    Some(section.clone()),
                    Some(self.is_expanded_filter.clone()),
                );

                let is_expanded_handler = section.connect_is_expanded_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.is_expanded_filter.changed(gtk::FilterChange::Different);
                    }
                ));
                handlers.push(is_expanded_handler);

                // Merge the models for the category and its children.
                let wrapper_model = gio::ListStore::new::<glib::Object>();
                wrapper_model.append(&section_model);
                wrapper_model.append(&children_model);

                gtk::FlattenListModel::new(Some(wrapper_model)).upcast::<gio::ListModel>()
            } else {
                // Create a list model for the item.
                SingleItemListModel::new(Some(&item)).upcast()
            };

            self.inner_item.set(item, handlers);

            self.is_visible_filter.set_filter_func(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                false,
                move |_| imp.is_visible.get()
            ));
            let model =
                gtk::FilterListModel::new(Some(inner_model), Some(self.is_visible_filter.clone()));

            let obj = self.obj();
            model.connect_items_changed(clone!(
                #[weak]
                obj,
                move |_model, pos, removed, added| {
                    obj.items_changed(pos, removed, added);
                }
            ));

            self.model.set(model).unwrap();
        }

        /// Set whether this item is visible.
        pub(super) fn set_visible(&self, visible: bool) {
            if self.is_visible.get() == visible {
                return;
            }

            self.is_visible.set(visible);

            self.obj().notify_is_visible();
            self.is_visible_filter.changed(gtk::FilterChange::Different);
        }

        /// Set whether to inhibit the expanded state.
        fn set_inhibit_expanded(&self, inhibit: bool) {
            if self.inhibit_expanded.get() == inhibit {
                return;
            }

            self.inhibit_expanded.set(inhibit);

            self.obj().notify_inhibit_expanded();
            self.is_expanded_filter
                .changed(gtk::FilterChange::Different);
        }
    }
}

glib::wrapper! {
    /// A top-level item in the sidebar.
    ///
    /// This wraps the inner item to handle its visibility and whether it should
    /// show its children (i.e. whether it is "expanded").
    pub struct SidebarItem(ObjectSubclass<imp::SidebarItem>)
        @implements gio::ListModel;
}

impl SidebarItem {
    /// Construct a new `SidebarItem` for the given item.
    pub fn new(item: impl IsA<glib::Object>) -> Self {
        glib::Object::builder()
            .property("inner-item", &item)
            .build()
    }

    /// Update the visibility of this item for the drag-n-drop of a room with
    /// the given category.
    pub(crate) fn update_visibility_for_room_category(
        &self,
        source_category: Option<RoomCategory>,
    ) {
        let inner_item = self.inner_item();
        let visible = if let Some(section) = inner_item.downcast_ref::<SidebarSection>() {
            section.visible_for_room_category(source_category)
        } else if let Some(icon_item) = inner_item.downcast_ref::<SidebarIconItem>() {
            icon_item.visible_for_room_category(source_category)
        } else {
            true
        };

        self.imp().set_visible(visible);
    }
}
