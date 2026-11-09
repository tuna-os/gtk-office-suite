use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    glib,
    glib::{clone, closure, closure_local},
};
use ruma::{Int, OwnedUserId, events::room::power_levels::UserPowerLevel};
use tracing::error;

use super::{MemberPowerLevel, PermissionsSelectMemberRow, PrivilegedMembers};
use crate::{
    components::{
        PillSearchEntry, PowerLevelSelectionComboBox, confirm_mute_room_member_dialog,
        confirm_set_room_member_power_level_same_as_own_dialog,
    },
    prelude::*,
    session::{Member, Permissions},
    utils::expression,
};

mod imp {
    use std::{cell::RefCell, collections::HashMap, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/permissions/add_members_subpage.ui"
    )]
    #[properties(wrapper_type = super::PermissionsAddMembersSubpage)]
    pub struct PermissionsAddMembersSubpage {
        #[template_child]
        search_entry: TemplateChild<PillSearchEntry>,
        #[template_child]
        power_level_combo: TemplateChild<PowerLevelSelectionComboBox>,
        #[template_child]
        list_view: TemplateChild<gtk::ListView>,
        #[template_child]
        add_button: TemplateChild<gtk::Button>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        /// The permissions of the room.
        #[property(get, set = Self::set_permissions, explicit_notify, nullable)]
        permissions: glib::WeakRef<Permissions>,
        power_level_filter: gtk::CustomFilter,
        filtered_model: gtk::FilterListModel,
        /// The list of members with custom power levels.
        #[property(get, set = Self::set_privileged_members, explicit_notify, nullable)]
        privileged_members: glib::WeakRef<PrivilegedMembers>,
        /// The selected members in the list.
        selected_members: RefCell<HashMap<OwnedUserId, Member>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PermissionsAddMembersSubpage {
        const NAME: &'static str = "RoomDetailsPermissionsAddMembersSubpage";
        type Type = super::PermissionsAddMembersSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PermissionsAddMembersSubpage {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("selection-changed").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.search_entry.connect_pill_removed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, source| {
                    if let Ok(member) = source.downcast::<Member>() {
                        imp.remove_selected(&member);
                    }
                }
            ));

            self.initialize_filtered_model();
            self.initialize_list_view();
        }
    }

    impl WidgetImpl for PermissionsAddMembersSubpage {}

    impl NavigationPageImpl for PermissionsAddMembersSubpage {
        fn shown(&self) {
            self.search_entry.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl PermissionsAddMembersSubpage {
        /// Set the permissions of the room.
        fn set_permissions(&self, permissions: Option<&Permissions>) {
            if self.permissions.upgrade().as_ref() == permissions {
                return;
            }

            self.permissions.set(permissions);

            if let Some(permissions) = permissions {
                self.power_level_filter.set_filter_func(clone!(
                    #[weak]
                    permissions,
                    #[upgrade_or]
                    true,
                    move |obj| {
                        let Some(member) = obj.downcast_ref::<Member>() else {
                            return false;
                        };

                        // Since this is a view to add custom power levels, filter out members with
                        // a custom power level already.
                        if let UserPowerLevel::Int(power_level) = member.power_level() {
                            i64::from(power_level) == permissions.default_power_level()
                        } else {
                            // There should not be members with infinite power level.
                            false
                        }
                    }
                ));
            }

            let members = permissions
                .and_then(Permissions::room)
                .map(|r| r.get_or_create_members());
            self.filtered_model.set_model(members.as_ref());

            self.power_level_combo.set_selected_power_level(
                permissions
                    .map(Permissions::default_power_level)
                    .unwrap_or_default(),
            );
            self.power_level_combo.set_permissions(permissions);

            self.update_visible_page();
            self.obj().notify_permissions();
        }

        /// Set the list of members with custom power levels.
        fn set_privileged_members(&self, members: Option<&PrivilegedMembers>) {
            if self.privileged_members.upgrade().as_ref() == members {
                return;
            }

            self.privileged_members.set(members);
            self.obj().notify_privileged_members();
        }

        /// Update the visible page of the stack.
        fn update_visible_page(&self) {
            let is_empty = self.filtered_model.n_items() == 0;

            let visible_page = if is_empty { "no-match" } else { "list" };
            self.stack.set_visible_child_name(visible_page);
        }

        /// Whether the member with the given ID is selected.
        fn is_selected(&self, user_id: &OwnedUserId) -> bool {
            self.selected_members.borrow().contains_key(user_id)
        }

        /// Toggle whether the given member is selected.
        fn toggle_selected(&self, member: Member) {
            let is_selected = self.is_selected(member.user_id());

            if is_selected {
                self.remove_selected(&member);
            } else {
                self.add_selected(member);
            }
        }

        /// Add the given member to the selected list.
        fn add_selected(&self, member: Member) {
            {
                let mut selected_members = self.selected_members.borrow_mut();
                let user_id = member.user_id();

                if selected_members.contains_key(user_id) {
                    // Nothing to do.
                    return;
                }

                self.search_entry.add_pill(&member);
                selected_members.insert(user_id.clone(), member);
            }

            self.add_button.set_sensitive(true);
            self.obj().emit_by_name::<()>("selection-changed", &[]);
        }

        /// Remove the given member from the selected list.
        fn remove_selected(&self, member: &Member) {
            let is_empty = {
                let mut selected_members = self.selected_members.borrow_mut();

                if selected_members.remove(member.user_id()).is_none() {
                    // Nothing happened.
                    return;
                }

                self.search_entry.remove_pill(&member.identifier());

                selected_members.is_empty()
            };

            self.add_button.set_sensitive(!is_empty);
            self.obj().emit_by_name::<()>("selection-changed", &[]);
        }

        /// Add the selected members to the list of members with custom power
        /// levels.
        #[template_callback]
        async fn add_members(&self) {
            let Some(permissions) = self.permissions.upgrade() else {
                return;
            };
            let Some(privileged_members) = self.privileged_members.upgrade() else {
                return;
            };

            let obj = self.obj();
            let power_level = self.power_level_combo.selected_power_level();

            let members = self
                .selected_members
                .borrow()
                .values()
                .cloned()
                .collect::<Vec<_>>();

            // Warn if users are muted.
            let is_muted = power_level <= permissions.mute_power_level();
            if is_muted && !confirm_mute_room_member_dialog(&members, &*obj).await {
                return;
            }

            let power_level = Int::new_saturating(power_level);

            // Warn if power level is set at same level as own power level.
            let is_own_power_level = power_level == permissions.own_power_level();
            if is_own_power_level
                && !confirm_set_room_member_power_level_same_as_own_dialog(&members, &*obj).await
            {
                return;
            }

            let members = self
                .selected_members
                .take()
                .into_iter()
                .map(|(user_id, member)| {
                    let member = MemberPowerLevel::new(&member, &permissions);
                    member.set_power_level(power_level.into());

                    (user_id, member)
                });
            privileged_members.add_members(members);

            let _ = obj.activate_action("navigation.pop", None);
            self.search_entry.clear();
            self.add_button.set_sensitive(false);
            obj.emit_by_name::<()>("selection-changed", &[]);
        }

        fn initialize_filtered_model(&self) {
            let user_expr = gtk::ClosureExpression::new::<String>(
                &[] as &[gtk::Expression],
                closure!(|item: Option<glib::Object>| {
                    item.and_downcast_ref()
                        .map(Member::search_string)
                        .unwrap_or_default()
                }),
            );
            let search_filter = gtk::StringFilter::builder()
                .match_mode(gtk::StringFilterMatchMode::Substring)
                .expression(expression::normalize_string(user_expr))
                .ignore_case(true)
                .build();

            expression::normalize_string(self.search_entry.property_expression("text")).bind(
                &search_filter,
                "search",
                None::<&glib::Object>,
            );

            let filter = gtk::EveryFilter::new();
            filter.append(self.power_level_filter.clone());
            filter.append(search_filter);

            self.filtered_model.set_filter(Some(&filter));

            self.filtered_model.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| {
                    imp.update_visible_page();
                }
            ));
        }

        fn initialize_list_view(&self) {
            self.list_view.connect_activate(clone!(
                #[weak(rename_to = imp)]
                self,
                move |list_view, index| {
                    let Some(member) = list_view
                        .model()
                        .and_then(|m| m.item(index))
                        .and_downcast::<Member>()
                    else {
                        return;
                    };

                    imp.toggle_selected(member);
                }
            ));

            // Sort members by display name, then user ID.
            let display_name_expr = Member::this_expression("display-name");
            let display_name_sorter = gtk::StringSorter::new(Some(display_name_expr));

            let user_id_expr = Member::this_expression("user-id-string");
            let user_id_sorter = gtk::StringSorter::new(Some(user_id_expr));

            let sorter = gtk::MultiSorter::new();
            sorter.append(display_name_sorter);
            sorter.append(user_id_sorter);

            let sorted_model =
                gtk::SortListModel::new(Some(self.filtered_model.clone()), Some(sorter));

            self.list_view
                .set_model(Some(&gtk::NoSelection::new(Some(sorted_model))));

            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, item| {
                    let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                        error!("List item factory did not receive a list item: {item:?}");
                        return;
                    };

                    let row = PermissionsSelectMemberRow::new();
                    item.set_child(Some(&row));
                    item.bind_property("item", &row, "member")
                        .sync_create()
                        .build();
                    item.set_selectable(false);

                    // Toggle the selection when the checkbox is toggled.
                    row.connect_selected_notify(clone!(
                        #[weak]
                        imp,
                        move |row| {
                            let Some(member) = row.member() else {
                                return;
                            };

                            if row.selected() {
                                imp.add_selected(member);
                            } else {
                                imp.remove_selected(&member);
                            }
                        }
                    ));

                    // Toggle the checkbox when the selection changed.
                    imp.obj().connect_selection_changed(clone!(
                        #[weak]
                        row,
                        move |obj| {
                            let Some(member) = row.member() else {
                                return;
                            };

                            let selected = obj.imp().is_selected(member.user_id());
                            row.set_selected(selected);
                        }
                    ));
                }
            ));
            self.list_view.set_factory(Some(&factory));
        }
    }
}

glib::wrapper! {
    /// Subpage to add members with custom permissions.
    pub struct PermissionsAddMembersSubpage(ObjectSubclass<imp::PermissionsAddMembersSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PermissionsAddMembersSubpage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal emitted when the selection changes.
    pub fn connect_selection_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "selection-changed",
            true,
            closure_local!(|obj: Self| {
                f(&obj);
            }),
        )
    }
}
