use gtk::{
    gio, glib,
    glib::{clone, closure},
    prelude::*,
    subclass::prelude::*,
};

use crate::{
    components::PillSource,
    session::{Member, MembershipListKind, Room},
    utils::{BoundObjectWeakRef, ExpressionListModel, SingleItemListModel, expression},
};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::CompletionMemberList)]
    pub struct CompletionMemberList {
        /// The current room.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: BoundObjectWeakRef<Room>,
        /// The list of room members used for completion.
        #[property(get)]
        joined_members: glib::WeakRef<gio::ListModel>,
        /// The filtered members list.
        filtered_members: gtk::FilterListModel,
        permissions_handler: RefCell<Option<glib::SignalHandlerId>>,
        /// The list model for the `@room` item.
        at_room_model: SingleItemListModel,
        /// The search filter.
        search_filter: gtk::StringFilter,
        /// The list of sorted and filtered room members.
        #[property(get)]
        list: gtk::FilterListModel,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CompletionMemberList {
        const NAME: &'static str = "ContentCompletionMemberList";
        type Type = super::CompletionMemberList;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CompletionMemberList {
        fn constructed(&self) {
            self.parent_constructed();

            // Filter the members, the criteria:
            // - not our user
            // - not ignored
            let not_own_user = gtk::BoolFilter::builder()
                .expression(Member::this_expression("is-own-user"))
                .invert(true)
                .build();

            let not_ignored = gtk::BoolFilter::builder()
                .expression(Member::this_expression("is-ignored"))
                .invert(true)
                .build();

            let filter = gtk::EveryFilter::new();
            filter.append(not_own_user);
            filter.append(not_ignored);

            self.filtered_members.set_filter(Some(&filter));
            self.filtered_members.set_watch_items(true);

            // Watch the activity and display name of members.
            let latest_activity_expr = Member::this_expression("latest-activity");
            let display_name_expr = Member::this_expression("display-name");

            let expr_model = ExpressionListModel::new();
            expr_model.set_expressions(vec![
                latest_activity_expr.clone().upcast(),
                display_name_expr.clone().upcast(),
            ]);
            expr_model.set_model(Some(self.filtered_members.clone()));

            // Sort the members list by activity, then display name.
            let activity = gtk::NumericSorter::builder()
                .sort_order(gtk::SortType::Descending)
                .expression(latest_activity_expr)
                .build();

            let display_name = gtk::StringSorter::builder()
                .ignore_case(true)
                .expression(display_name_expr)
                .build();

            let sorter = gtk::MultiSorter::new();
            sorter.append(activity);
            sorter.append(display_name);
            let sorted_members_model = gtk::SortListModel::builder()
                .sorter(&sorter)
                .model(&expr_model)
                .build();

            // Add `@room` model.
            let models_list = gio::ListStore::new::<gio::ListModel>();
            models_list.append(&self.at_room_model);
            models_list.append(&sorted_members_model);
            let flatten_model = gtk::FlattenListModel::new(Some(models_list));

            // Setup the search filter.
            let item_search_string_expr = gtk::ClosureExpression::new::<String>(
                &[
                    PillSource::this_expression("identifier"),
                    PillSource::this_expression("display-name"),
                ],
                closure!(
                    |_: Option<glib::Object>, identifier: &str, display_name: &str| {
                        format!("{display_name} {identifier}")
                    }
                ),
            );
            self.search_filter.set_ignore_case(true);
            self.search_filter
                .set_match_mode(gtk::StringFilterMatchMode::Substring);
            self.search_filter
                .set_expression(Some(expression::normalize_string(item_search_string_expr)));

            self.list.set_filter(Some(&self.search_filter));
            self.list.set_model(Some(&flatten_model));
        }

        fn dispose(&self) {
            if let Some(room) = self.room.obj()
                && let Some(handler) = self.permissions_handler.take()
            {
                room.permissions().disconnect(handler);
            }
        }
    }

    impl CompletionMemberList {
        /// Set the current room.
        fn set_room(&self, room: Option<&Room>) {
            let prev_room = self.room.obj();

            if prev_room.as_ref() == room {
                return;
            }

            if let Some(room) = prev_room
                && let Some(handler) = self.permissions_handler.take()
            {
                room.permissions().disconnect(handler);
            }
            self.room.disconnect_signals();

            if let Some(room) = room {
                let permissions_handler =
                    room.permissions().connect_can_notify_room_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_at_room_model();
                        }
                    ));
                self.permissions_handler.replace(Some(permissions_handler));

                let is_direct_handler = room.connect_is_direct_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_at_room_model();
                    }
                ));

                self.room.set(room, vec![is_direct_handler]);
            }

            let joined_members = room
                .map(Room::get_or_create_members)
                .map(|members| members.membership_list(MembershipListKind::Join));
            self.filtered_members.set_model(joined_members.as_ref());

            self.update_at_room_model();
            self.obj().notify_joined_members();
        }

        /// Update whether `@room` should be present in the suggestions.
        fn update_at_room_model(&self) {
            // Only present `@room` if it's not a DM and user can notify the room.
            let room = self
                .room
                .obj()
                .filter(|r| !r.is_direct() && r.permissions().can_notify_room());

            self.at_room_model.set_item(room.map(|room| room.at_room()));
        }

        /// Set the search term.
        pub(super) fn set_search_term(&self, term: Option<&str>) {
            self.search_filter.set_search(term);
        }
    }
}

glib::wrapper! {
    /// The filtered and sorted members list for completion.
    ///
    /// Also includes an `@room` item for notifying the whole room.
    pub struct CompletionMemberList(ObjectSubclass<imp::CompletionMemberList>);
}

impl CompletionMemberList {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the search term.
    pub(crate) fn set_search_term(&self, term: Option<&str>) {
        self.imp().set_search_term(term);
    }
}

impl Default for CompletionMemberList {
    fn default() -> Self {
        Self::new()
    }
}
