use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};

use super::SidebarItemList;
use crate::{
    session::{IdentityVerification, Room},
    utils::{BoundObjectWeakRef, FixedSelection, expression},
};

mod imp {
    use std::cell::{Cell, OnceCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SidebarListModel)]
    pub struct SidebarListModel {
        /// The list of items in the sidebar.
        #[property(get, set = Self::set_item_list, construct_only)]
        item_list: OnceCell<SidebarItemList>,
        /// The string filter.
        #[property(get)]
        string_filter: gtk::StringFilter,
        /// Whether the string filter is active.
        #[property(get)]
        is_filtered: Cell<bool>,
        /// The selection model.
        #[property(get)]
        selection_model: FixedSelection,
        /// The selected item, if it has signal handlers.
        selected_item: BoundObjectWeakRef<glib::Object>,
        item_type_filter: gtk::CustomFilter,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarListModel {
        const NAME: &'static str = "SidebarListModel";
        type Type = super::SidebarListModel;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarListModel {
        fn constructed(&self) {
            self.parent_constructed();

            // When a verification is replaced, select the replacement automatically.
            self.selection_model.connect_selected_item_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |selection_model| {
                    imp.selected_item.disconnect_signals();

                    if let Some(item) = &selection_model.selected_item()
                        && let Some(verification) = item.downcast_ref::<IdentityVerification>()
                    {
                        let verification_handler = verification.connect_replaced(clone!(
                            #[weak]
                            selection_model,
                            move |_, new_verification| {
                                selection_model.set_selected_item(Some(new_verification.clone()));
                            }
                        ));
                        imp.selected_item.set(item, vec![verification_handler]);
                    }
                }
            ));

            // Disable the expanded filters of the items during search.
            self.string_filter.connect_search_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |string_filter| {
                    imp.set_is_filtered(string_filter.search().is_some_and(|s| !s.is_empty()));
                }
            ));
        }
    }

    impl SidebarListModel {
        /// The list of items in the sidebar.
        fn item_list(&self) -> &SidebarItemList {
            self.item_list.get().unwrap()
        }

        /// Set the list of items in the sidebar.
        fn set_item_list(&self, item_list: SidebarItemList) {
            let item_list = self.item_list.get_or_init(|| item_list);

            let flattened_model = gtk::FlattenListModel::new(Some(item_list.clone()));

            // When search is active, only show rooms.
            self.item_type_filter.set_filter_func(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                false,
                move |item| !imp.is_filtered.get() || item.is::<Room>()
            ));

            // Set up search.
            let room_name_expression = Room::this_expression("display-name");
            self.string_filter
                .set_match_mode(gtk::StringFilterMatchMode::Substring);
            self.string_filter
                .set_expression(Some(expression::normalize_string(room_name_expression)));
            self.string_filter.set_ignore_case(true);
            // Default to an empty string to be able to bind to GtkEditable::text.
            self.string_filter.set_search(Some(""));

            let multi_filter = gtk::EveryFilter::new();
            multi_filter.append(self.item_type_filter.clone());
            multi_filter.append(self.string_filter.clone());

            let filter_model = gtk::FilterListModel::new(Some(flattened_model), Some(multi_filter));

            self.selection_model.set_model(Some(filter_model));
        }

        /// Set whether the string filter is active.
        fn set_is_filtered(&self, is_filtered: bool) {
            if self.is_filtered.get() == is_filtered {
                return;
            }

            self.is_filtered.set(is_filtered);

            self.obj().notify_is_filtered();
            self.item_list().inhibit_expanded(is_filtered);
            // Search in all the rooms, regardless of the selected space.
            self.item_list().space_list().set_search_active(is_filtered);
            self.item_type_filter.changed(gtk::FilterChange::Different);
        }
    }
}

glib::wrapper! {
    /// A wrapper for the sidebar list model of a `Session`.
    ///
    /// It allows to keep the state for selection and filtering.
    pub struct SidebarListModel(ObjectSubclass<imp::SidebarListModel>);
}

impl SidebarListModel {
    /// Create a new `SidebarListModel`.
    pub fn new(item_list: &SidebarItemList) -> Self {
        glib::Object::builder()
            .property("item-list", item_list)
            .build()
    }
}
