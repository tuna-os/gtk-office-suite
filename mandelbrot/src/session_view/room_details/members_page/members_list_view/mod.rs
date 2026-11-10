use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{
    gio, glib,
    glib::{clone, closure},
};

mod item_row;
mod membership_subpage_row;

use self::{item_row::ItemRow, membership_subpage_row::MembershipSubpageRow};
use crate::{
    components::LoadingRow,
    prelude::*,
    session::{Member, MemberList, MembershipListKind, Room},
    session_view::room_details::MembershipSubpageItem,
    utils::{BoundObjectWeakRef, ExpressionListModel, LoadingState, expression},
};

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        collections::HashMap,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/members_page/members_list_view/mod.ui"
    )]
    #[properties(wrapper_type = super::MembersListView)]
    pub struct MembersListView {
        #[template_child]
        search_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        empty_stack_page: TemplateChild<gtk::StackPage>,
        #[template_child]
        empty_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        empty_listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        members_stack_page: TemplateChild<gtk::StackPage>,
        #[template_child]
        list_view: TemplateChild<gtk::ListView>,
        /// The room containing the members to present.
        #[property(get, set = Self::set_room, construct_only)]
        room: glib::WeakRef<Room>,
        /// The lists of members for the room.
        #[property(get, set = Self::set_members, construct_only)]
        members: BoundObjectWeakRef<MemberList>,
        /// The items to add to the membership list, if any.
        extra_items: OnceCell<gtk::FilterListModel>,
        /// The model with the search filter.
        filtered_model: gtk::FilterListModel,
        /// The kind of the membership list.
        #[property(get, set = Self::set_kind, construct_only, builder(MembershipListKind::default()))]
        kind: Cell<MembershipListKind>,
        /// Whether our own user can send an invite in the current room.
        #[property(get, set = Self::set_can_invite, explicit_notify)]
        can_invite: Cell<bool>,
        extra_members_state_handler: RefCell<Option<glib::SignalHandlerId>>,
        membership_items_changed_handlers:
            RefCell<HashMap<MembershipListKind, glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MembersListView {
        const NAME: &'static str = "ContentMembersListView";
        type Type = super::MembersListView;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            ItemRow::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("members-list");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MembersListView {
        fn constructed(&self) {
            self.parent_constructed();

            // Needed because the GtkSearchEntry is not the direct child of the
            // GtkSearchBear.
            self.search_bar.connect_entry(&*self.search_entry);

            let member_expr = gtk::ClosureExpression::new::<String>(
                &[] as &[gtk::Expression],
                closure!(|item: Option<glib::Object>| {
                    item.and_downcast_ref()
                        .map(Member::search_string)
                        .unwrap_or_default()
                }),
            );
            let search_filter = gtk::StringFilter::builder()
                .match_mode(gtk::StringFilterMatchMode::Substring)
                .expression(expression::normalize_string(member_expr))
                .ignore_case(true)
                .build();

            expression::normalize_string(self.search_entry.property_expression("text")).bind(
                &search_filter,
                "search",
                None::<&glib::Object>,
            );

            self.filtered_model.set_filter(Some(&search_filter));
            self.list_view.set_model(Some(&gtk::NoSelection::new(Some(
                self.filtered_model.clone(),
            ))));

            self.init_members_list();
        }

        fn dispose(&self) {
            if let Some(members) = self.members.obj() {
                if let Some(handler) = self.extra_members_state_handler.take() {
                    members.disconnect(handler);
                }

                for (kind, handler) in self.membership_items_changed_handlers.take() {
                    members.membership_list(kind).disconnect(handler);
                }
            }
        }
    }

    impl WidgetImpl for MembersListView {}
    impl NavigationPageImpl for MembersListView {}

    #[gtk::template_callbacks]
    impl MembersListView {
        /// Set the room containing the members to present.
        fn set_room(&self, room: &Room) {
            self.room.set(Some(room));

            // Show the invite button when we can invite but it is not a direct room.
            let can_invite_expr = room.permissions().property_expression("can-invite");
            let is_direct_expr = room.property_expression("is-direct");
            expression::and(can_invite_expr, expression::not(is_direct_expr)).bind(
                &*self.obj(),
                "can-invite",
                None::<&glib::Object>,
            );
        }

        /// Set the room containing the members to present.
        fn set_members(&self, members: &MemberList) {
            let state_handler = members.connect_state_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_view();
                }
            ));

            self.members.set(members, vec![state_handler]);
        }

        /// Set the kind of the membership list.
        fn set_kind(&self, kind: MembershipListKind) {
            self.kind.set(kind);
            self.obj().set_tag(Some(kind.tag()));
            self.update_empty_page();
        }

        /// Set whether our own user can send an invite in the current room.
        fn set_can_invite(&self, can_invite: bool) {
            if self.can_invite.get() == can_invite {
                return;
            }

            self.can_invite.set(can_invite);
            self.obj().notify_can_invite();
        }

        /// Initialize the members list used for this view.
        fn init_members_list(&self) {
            let Some(members) = self.members.obj() else {
                return;
            };

            self.init_extra_items();

            let kind = self.kind.get();
            let membership_list = members.membership_list(kind);

            let items_changed_handler = membership_list.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.update_view();
                }
            ));
            self.membership_items_changed_handlers
                .borrow_mut()
                .insert(kind, items_changed_handler);

            // Sort the members list by power level, then display name.
            let power_level_expr = Member::this_expression("power-level-i64");
            let sorter = gtk::MultiSorter::new();
            sorter.append(
                gtk::NumericSorter::builder()
                    .expression(&power_level_expr)
                    .sort_order(gtk::SortType::Descending)
                    .build(),
            );

            let display_name_expr = Member::this_expression("display-name");
            sorter.append(gtk::StringSorter::new(Some(&display_name_expr)));

            // We need to notify when a watched property changes so the sorter can update
            // the list.
            let expr_members = ExpressionListModel::new();
            expr_members
                .set_expressions(vec![power_level_expr.upcast(), display_name_expr.upcast()]);
            expr_members.set_model(Some(membership_list));

            let sorted_members = gtk::SortListModel::new(Some(expr_members), Some(sorter));

            let full_model = if let Some(extra_items) = self.extra_items.get() {
                let model_list = gio::ListStore::new::<gio::ListModel>();
                model_list.append(extra_items);
                model_list.append(&sorted_members);

                gtk::FlattenListModel::new(Some(model_list)).upcast::<gio::ListModel>()
            } else {
                sorted_members.upcast()
            };
            self.filtered_model.set_model(Some(&full_model));

            self.update_view();
            self.update_empty_listbox();
        }

        /// Initialize the items to add to the membership list, if necessary.
        fn init_extra_items(&self) {
            let Some(members) = self.members.obj() else {
                return;
            };

            // Only the list of joined members displays extra items.
            if self.kind.get() != MembershipListKind::Join {
                return;
            }

            let filter = gtk::CustomFilter::new(|item| {
                if let Some(loading_row) = item.downcast_ref::<LoadingRow>() {
                    loading_row.is_visible()
                } else if let Some(subpage_item) = item.downcast_ref::<MembershipSubpageItem>() {
                    subpage_item.model().n_items() != 0
                } else {
                    false
                }
            });

            let loading_row = LoadingRow::new();
            let extra_members_state_handler = members.connect_state_notify(clone!(
                #[weak]
                loading_row,
                #[weak]
                filter,
                move |members| {
                    let was_row_visible = loading_row.is_visible();

                    Self::update_loading_row(&loading_row, members.state());

                    // If the loading row visibility changed, so does the filtering.
                    if loading_row.is_visible() != was_row_visible {
                        filter.changed(gtk::FilterChange::Different);
                    }
                }
            ));
            self.extra_members_state_handler
                .replace(Some(extra_members_state_handler));
            Self::update_loading_row(&loading_row, members.state());

            let base_model = gio::ListStore::new::<glib::Object>();
            base_model.append(&loading_row);

            for &kind in &[
                MembershipListKind::Knock,
                MembershipListKind::Invite,
                MembershipListKind::Ban,
            ] {
                let list = members.membership_list(kind);
                let items_changed_handler = list.connect_items_changed(clone!(
                    #[weak]
                    filter,
                    move |list, _, _, added| {
                        let n_items = list.n_items();

                        // If the list is or was empty, the filtering changed.
                        if n_items == 0 || n_items == added {
                            filter.changed(gtk::FilterChange::Different);
                        }
                    }
                ));
                self.membership_items_changed_handlers
                    .borrow_mut()
                    .insert(kind, items_changed_handler);

                base_model.append(&MembershipSubpageItem::new(kind, &list));
            }

            let extra_items = self
                .extra_items
                .get_or_init(|| gtk::FilterListModel::new(Some(base_model), Some(filter)));

            extra_items.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.update_empty_listbox();
                }
            ));
        }

        /// Update the given loading row for the given loading state.
        fn update_loading_row(loading_row: &LoadingRow, state: LoadingState) {
            let error = (state == LoadingState::Error)
                .then(|| gettext("Could not load the full list of room members"));
            loading_row.set_error(error.as_deref());

            loading_row.set_visible(state != LoadingState::Ready);
        }

        /// Update the view for the current state.
        fn update_view(&self) {
            let Some(members) = self.members.obj() else {
                self.stack.set_visible_child_name("empty");
                return;
            };

            let kind = self.kind.get();
            let membership_list = members.membership_list(kind);
            let count = membership_list.n_items();
            let is_empty = count == 0;

            // We don't use the count in the strings so we use separate gettext calls for
            // singular and plural rather than using ngettext.
            let title = match kind {
                MembershipListKind::Join => {
                    if count == 1 {
                        gettext("Room Member")
                    } else {
                        gettext("Room Members")
                    }
                }
                MembershipListKind::Invite => {
                    if count == 1 {
                        gettext("Invited Room Member")
                    } else {
                        gettext("Invited Room Members")
                    }
                }
                MembershipListKind::Ban => {
                    if count == 1 {
                        gettext("Banned Room Member")
                    } else {
                        gettext("Banned Room Members")
                    }
                }
                MembershipListKind::Knock => {
                    if count == 1 {
                        gettext("Invite Request")
                    } else {
                        gettext("Invite Requests")
                    }
                }
            };

            self.obj().set_title(&title);
            self.members_stack_page.set_title(&title);

            let (visible_page, extra_items) = if is_empty {
                match members.state() {
                    LoadingState::Initial | LoadingState::Loading => ("loading", None),
                    LoadingState::Error => ("error", None),
                    LoadingState::Ready => ("empty", self.extra_items.get()),
                }
            } else {
                ("members", None)
            };

            self.empty_listbox.bind_model(extra_items, |item| {
                let row = MembershipSubpageRow::new();
                row.set_item(item.downcast_ref::<MembershipSubpageItem>().cloned());

                row.upcast()
            });

            // Hide the search button and bar if the list is empty, since there is no search
            // possible.
            self.search_button.set_visible(!is_empty);
            self.search_bar.set_visible(!is_empty);

            self.stack.set_visible_child_name(visible_page);
        }

        /// Update the "empty" page for the current state.
        fn update_empty_page(&self) {
            let kind = self.kind.get();

            let (title, description) = match kind {
                MembershipListKind::Join => {
                    let title = gettext("No Room Members");
                    let description = gettext("There are no members in this room");
                    (title, description)
                }
                MembershipListKind::Invite => {
                    let title = gettext("No Invited Room Members");
                    let description = gettext("There are no invited members in this room");
                    (title, description)
                }
                MembershipListKind::Ban => {
                    let title = gettext("No Banned Room Members");
                    let description = gettext("There are no banned members in this room");
                    (title, description)
                }
                MembershipListKind::Knock => {
                    let title = gettext("No Invite Requests");
                    let description = gettext("There are no invite requests in this room");
                    (title, description)
                }
            };

            self.empty_stack_page.set_title(&title);
            self.empty_page.set_title(&title);
            self.empty_page.set_description(Some(&description));
            self.empty_page.set_icon_name(Some(kind.icon_name()));
        }

        /// Update the `GtkListBox` of the "empty" page for the current state.
        fn update_empty_listbox(&self) {
            let has_extra_items = self
                .extra_items
                .get()
                .is_some_and(|model| model.n_items() > 0);
            self.empty_listbox.set_visible(has_extra_items);
        }

        /// Activate the row of the members `GtkListView` at the given position.
        #[template_callback]
        fn activate_listview_row(&self, pos: u32) {
            let Some(item) = self.filtered_model.item(pos) else {
                return;
            };
            let obj = self.obj();

            if let Some(member) = item.downcast_ref::<Member>() {
                obj.activate_action(
                    "details.show-member",
                    Some(&member.user_id().as_str().to_variant()),
                )
                .expect("action exists");
            } else if let Some(item) = item.downcast_ref::<MembershipSubpageItem>() {
                obj.activate_action(
                    "members.show-membership-list",
                    Some(&item.kind().to_variant()),
                )
                .expect("action exists");
            }
        }

        /// Activate the given row from the `GtkListBox`.
        #[template_callback]
        fn activate_listbox_row(&self, row: &gtk::ListBoxRow) {
            let row = row
                .downcast_ref::<MembershipSubpageRow>()
                .expect("list box contains only membership subpage rows");

            let Some(item) = row.item() else {
                return;
            };

            self.obj()
                .activate_action(
                    "members.show-membership-list",
                    Some(&item.kind().to_variant()),
                )
                .expect("action exists");
        }

        /// Reload the list of members of the room.
        #[template_callback]
        fn reload_members(&self) {
            let Some(members) = self.members.obj() else {
                return;
            };

            members.reload();
        }
    }
}

glib::wrapper! {
    /// A page to display a list of members.
    pub struct MembersListView(ObjectSubclass<imp::MembersListView>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MembersListView {
    /// Construct a new `MembersListView` with the given room, members list and
    /// kind.
    pub fn new(room: &Room, members: &MemberList, kind: MembershipListKind) -> Self {
        glib::Object::builder()
            .property("room", room)
            .property("members", members)
            .property("kind", kind)
            .build()
    }
}
