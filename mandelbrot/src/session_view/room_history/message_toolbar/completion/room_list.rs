use gtk::{glib, glib::closure, prelude::*, subclass::prelude::*};

use crate::{
    session::{JoinRule, Member, Membership, Room, RoomAliases, RoomCategory, RoomList},
    utils::{ExpressionListModel, expression},
};

mod imp {
    use std::marker::PhantomData;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::CompletionRoomList)]
    pub struct CompletionRoomList {
        /// The rooms used for completion.
        #[property(get = Self::rooms, set = Self::set_rooms, explicit_notify, nullable)]
        rooms: PhantomData<Option<RoomList>>,
        /// The filtered room list.
        filtered_rooms: gtk::FilterListModel,
        /// The search filter.
        search_filter: gtk::StringFilter,
        /// The list of sorted and filtered rooms.
        #[property(get)]
        list: gtk::FilterListModel,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CompletionRoomList {
        const NAME: &'static str = "ContentCompletionRoomList";
        type Type = super::CompletionRoomList;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CompletionRoomList {
        fn constructed(&self) {
            self.parent_constructed();

            // Filter the rooms, the criteria:
            // - not a space or upgraded
            // - joined
            // - anyone can join

            let category_filter = gtk::BoolFilter::new(Some(
                Room::this_expression("category").chain_closure::<bool>(closure!(
                    |_obj: Option<glib::Object>, category: RoomCategory| {
                        !matches!(category, RoomCategory::Space | RoomCategory::Outdated)
                    }
                )),
            ));

            let joined_filter = gtk::BoolFilter::new(Some(
                Room::this_expression("own-member")
                    .chain_property::<Member>("membership")
                    .chain_closure::<bool>(closure!(
                        |_obj: Option<glib::Object>, membership: Membership| {
                            membership == Membership::Join
                        }
                    )),
            ));

            let anyone_can_join_filter = gtk::BoolFilter::new(Some(
                Room::this_expression("join-rule").chain_property::<JoinRule>("anyone-can-join"),
            ));

            let filter = gtk::EveryFilter::new();
            filter.append(category_filter);
            filter.append(joined_filter);
            filter.append(anyone_can_join_filter);

            self.filtered_rooms.set_filter(Some(&filter));

            // Watch display name to update the sorter.
            let display_name_expr = Room::this_expression("display-name");

            let expr_model = ExpressionListModel::new();
            expr_model.set_expressions(vec![display_name_expr.clone().upcast()]);
            expr_model.set_model(Some(self.filtered_rooms.clone()));

            // Sort list by display name.
            let display_name_sorter = gtk::StringSorter::builder()
                .ignore_case(true)
                .expression(&display_name_expr)
                .build();

            let sorted_model = gtk::SortListModel::builder()
                .sorter(&display_name_sorter)
                .model(&expr_model)
                .build();

            // Set up the search filter.
            let alias_expr =
                Room::this_expression("aliases").chain_property::<RoomAliases>("alias-string");
            let room_search_string_expr = gtk::ClosureExpression::new::<String>(
                &[alias_expr, display_name_expr],
                closure!(
                    |_: Option<glib::Object>, alias: Option<&str>, display_name: &str| {
                        if let Some(alias) = alias {
                            format!("{display_name} {alias}")
                        } else {
                            display_name.to_owned()
                        }
                    }
                ),
            );
            self.search_filter.set_ignore_case(true);
            self.search_filter
                .set_match_mode(gtk::StringFilterMatchMode::Substring);
            self.search_filter
                .set_expression(Some(expression::normalize_string(room_search_string_expr)));

            self.list.set_filter(Some(&self.search_filter));
            self.list.set_watch_items(true);
            self.list.set_model(Some(&sorted_model));
        }
    }

    impl CompletionRoomList {
        /// The rooms used for completion.
        fn rooms(&self) -> Option<RoomList> {
            self.filtered_rooms.model().and_downcast()
        }

        /// Set the rooms used for completion.
        #[allow(clippy::needless_pass_by_value)]
        fn set_rooms(&self, rooms: Option<RoomList>) {
            if self.rooms() == rooms {
                return;
            }

            self.filtered_rooms.set_model(rooms.as_ref());
            self.obj().notify_rooms();
        }

        /// Set the search term.
        pub(super) fn set_search_term(&self, term: Option<&str>) {
            self.search_filter.set_search(term);
        }
    }
}

glib::wrapper! {
    /// The filtered and sorted rooms list for completion.
    pub struct CompletionRoomList(ObjectSubclass<imp::CompletionRoomList>);
}

impl CompletionRoomList {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the search term.
    pub(crate) fn set_search_term(&self, term: Option<&str>) {
        self.imp().set_search_term(term);
    }
}

impl Default for CompletionRoomList {
    fn default() -> Self {
        Self::new()
    }
}
