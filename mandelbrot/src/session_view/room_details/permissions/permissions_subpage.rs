use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::{
    Int,
    events::{
        StateEventType, TimelineEventType,
        room::power_levels::{PowerLevelAction, RoomPowerLevels, UserPowerLevel},
    },
};
use tracing::error;

use super::{PermissionsAddMembersSubpage, PermissionsMembersSubpage, PrivilegedMembers};
use crate::{
    components::{
        ButtonCountRow, LoadingButton, PowerLevelSelectionRow, UnsavedChangesResponse,
        unsaved_changes_dialog,
    },
    session::{POWER_LEVEL_MAX, Permissions},
    toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::{Cell, OnceCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/permissions/permissions_subpage.ui"
    )]
    #[properties(wrapper_type = super::PermissionsSubpage)]
    pub struct PermissionsSubpage {
        #[template_child]
        save_button: TemplateChild<LoadingButton>,
        #[template_child]
        messages_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        redact_own_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        redact_others_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        notify_room_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        state_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        name_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        topic_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        avatar_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        aliases_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        history_visibility_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        encryption_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        power_levels_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        server_acl_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        upgrade_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        invite_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        kick_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        ban_row: TemplateChild<PowerLevelSelectionRow>,
        #[template_child]
        members_default_spin_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        members_default_adjustment: TemplateChild<gtk::Adjustment>,
        #[template_child]
        members_default_text_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        members_default_label: TemplateChild<gtk::Label>,
        #[template_child]
        members_privileged_button: TemplateChild<ButtonCountRow>,
        /// The subpage to view and edit members with custom power levels.
        #[template_child]
        members_subpage: TemplateChild<PermissionsMembersSubpage>,
        /// The subpage to add members with custom power levels.
        #[template_child]
        add_members_subpage: TemplateChild<PermissionsAddMembersSubpage>,
        /// The permissions to watch.
        #[property(get, set = Self::set_permissions, construct_only)]
        permissions: BoundObjectWeakRef<Permissions>,
        /// Whether our own user can change the power levels in this room.
        #[property(get)]
        editable: Cell<bool>,
        /// Whether the permissions were changed by the user.
        #[property(get)]
        changed: Cell<bool>,
        /// The list of members with custom power levels.
        #[property(get)]
        privileged_members: OnceCell<PrivilegedMembers>,
        /// Whether an update is in progress.
        ///
        /// Avoids to call `Self::update_changed()` too often when several rows
        /// might be changed at once.
        update_in_progress: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PermissionsSubpage {
        const NAME: &'static str = "RoomDetailsPermissionsSubpage";
        type Type = super::PermissionsSubpage;
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
    impl ObjectImpl for PermissionsSubpage {}

    impl WidgetImpl for PermissionsSubpage {}
    impl NavigationPageImpl for PermissionsSubpage {}

    #[gtk::template_callbacks]
    impl PermissionsSubpage {
        /// Set the permissions to watch.
        fn set_permissions(&self, permissions: &Permissions) {
            let changed_handler = permissions.connect_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update();
                }
            ));

            self.permissions.set(permissions, vec![changed_handler]);

            let privileged_members = PrivilegedMembers::new(permissions);
            self.privileged_members
                .set(privileged_members.clone())
                .unwrap();

            privileged_members.connect_changed_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_changed();
                }
            ));

            self.members_subpage
                .set_list(Some(privileged_members.clone()));

            self.add_members_subpage.set_permissions(Some(permissions));
            self.add_members_subpage
                .set_privileged_members(Some(privileged_members));

            self.update();
        }

        /// The list of members with custom power levels.
        fn privileged_members(&self) -> &PrivilegedMembers {
            self.privileged_members
                .get()
                .expect("privileged members should be initialized")
        }

        /// Update all the permissions.
        fn update(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            self.update_in_progress.set(true);

            let can_change = permissions
                .is_allowed_to(PowerLevelAction::SendState(StateEventType::RoomPowerLevels));
            self.set_editable(can_change);

            self.update_room_actions();
            self.update_member_actions();
            self.update_members_power_levels();

            self.save_button.set_is_loading(false);

            self.update_in_progress.set(false);
            self.update_changed();
        }

        /// Set whether our own user can change the power levels in this room.
        fn set_editable(&self, editable: bool) {
            if self.editable.get() == editable {
                return;
            }

            self.editable.set(editable);
            self.obj().notify_editable();
        }

        /// Update whether the permissions were changed by the user.
        fn update_changed(&self) {
            if self.update_in_progress.get() {
                // Do not update, it will be called when all updates are done.
                return;
            }

            let changed = self.compute_changed();

            if self.changed.get() == changed {
                return;
            }

            self.changed.set(changed);
            self.obj().notify_changed();
        }

        /// Compute whether the user changed the permissions.
        #[allow(clippy::too_many_lines)]
        fn compute_changed(&self) -> bool {
            let Some(privileged_members) = self.privileged_members.get() else {
                return false;
            };

            if privileged_members.changed() {
                return true;
            }

            let Some(permissions) = self.permissions.obj() else {
                return false;
            };
            let power_levels = permissions.power_levels();

            let events_default = UserPowerLevel::from(power_levels.events_default);
            if self.messages_row.selected_power_level() != events_default {
                return true;
            }

            let redact_own = event_power_level(
                &power_levels,
                &TimelineEventType::RoomRedaction,
                events_default,
            );
            if self.redact_own_row.selected_power_level() != redact_own {
                return true;
            }

            let redact_others = redact_own.max(power_levels.redact.into());
            if self.redact_others_row.selected_power_level() != redact_others {
                return true;
            }

            let notify_room = power_levels.notifications.room;
            if self.notify_room_row.selected_power_level() != notify_room {
                return true;
            }

            let state_default = UserPowerLevel::from(power_levels.state_default);
            if self.state_row.selected_power_level() != state_default {
                return true;
            }

            let name =
                event_power_level(&power_levels, &TimelineEventType::RoomName, state_default);
            if self.name_row.selected_power_level() != name {
                return true;
            }

            let topic =
                event_power_level(&power_levels, &TimelineEventType::RoomTopic, state_default);
            if self.topic_row.selected_power_level() != topic {
                return true;
            }

            let avatar =
                event_power_level(&power_levels, &TimelineEventType::RoomAvatar, state_default);
            if self.avatar_row.selected_power_level() != avatar {
                return true;
            }

            let aliases = event_power_level(
                &power_levels,
                &TimelineEventType::RoomCanonicalAlias,
                state_default,
            );
            if self.aliases_row.selected_power_level() != aliases {
                return true;
            }

            let history_visibility = event_power_level(
                &power_levels,
                &TimelineEventType::RoomHistoryVisibility,
                state_default,
            );
            if self.history_visibility_row.selected_power_level() != history_visibility {
                return true;
            }

            let encryption = event_power_level(
                &power_levels,
                &TimelineEventType::RoomEncryption,
                state_default,
            );
            if self.encryption_row.selected_power_level() != encryption {
                return true;
            }

            let pl = event_power_level(
                &power_levels,
                &TimelineEventType::RoomPowerLevels,
                state_default,
            );
            if self.power_levels_row.selected_power_level() != pl {
                return true;
            }

            let server_acl = event_power_level(
                &power_levels,
                &TimelineEventType::RoomServerAcl,
                state_default,
            );
            if self.server_acl_row.selected_power_level() != server_acl {
                return true;
            }

            let upgrade = event_power_level(
                &power_levels,
                &TimelineEventType::RoomTombstone,
                state_default,
            );
            if self.upgrade_row.selected_power_level() != upgrade {
                return true;
            }

            let invite = power_levels.invite;
            if self.invite_row.selected_power_level() != invite {
                return true;
            }

            let kick = power_levels.kick;
            if self.kick_row.selected_power_level() != kick {
                return true;
            }

            let ban = power_levels.ban;
            if self.ban_row.selected_power_level() != ban {
                return true;
            }

            let default_pl = i64::from(power_levels.users_default);
            self.members_default_adjustment.value() as i64 != default_pl
        }

        /// Update the room actions section.
        fn update_room_actions(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let editable = self.editable.get();
            let power_levels = permissions.power_levels();
            let own_pl = permissions.own_power_level();

            let events_default = power_levels.events_default;
            self.messages_row
                .set_selected_power_level(events_default.into());
            self.messages_row
                .set_read_only(!editable || own_pl < events_default);

            let redact_own = event_power_level(
                &power_levels,
                &TimelineEventType::RoomRedaction,
                events_default.into(),
            );
            self.redact_own_row.set_selected_power_level(redact_own);
            self.redact_own_row
                .set_read_only(!editable || own_pl < redact_own);

            let redact_others = redact_own.max(power_levels.redact.into());
            self.redact_others_row
                .set_selected_power_level(redact_others);
            self.redact_others_row
                .set_read_only(!editable || own_pl < redact_others);

            let notify_room = power_levels.notifications.room.into();
            self.notify_room_row.set_selected_power_level(notify_room);
            self.notify_room_row
                .set_read_only(!editable || own_pl < notify_room);

            let state_default = power_levels.state_default.into();
            self.state_row.set_selected_power_level(state_default);
            self.state_row
                .set_read_only(!editable || own_pl < state_default);

            self.update_state_rows();
        }

        /// Update the rows about state events, except the default one.
        fn update_state_rows(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let editable = self.editable.get();
            let power_levels = permissions.power_levels();
            let own_pl = permissions.own_power_level();
            let state_default = self.state_row.selected_power_level();

            let name =
                event_power_level(&power_levels, &TimelineEventType::RoomName, state_default);
            self.name_row.set_selected_power_level(name);
            self.name_row.set_read_only(!editable || own_pl < name);

            let topic =
                event_power_level(&power_levels, &TimelineEventType::RoomTopic, state_default);
            self.topic_row.set_selected_power_level(topic);
            self.topic_row.set_read_only(!editable || own_pl < topic);

            let avatar =
                event_power_level(&power_levels, &TimelineEventType::RoomAvatar, state_default);
            self.avatar_row.set_selected_power_level(avatar);
            self.avatar_row.set_read_only(!editable || own_pl < avatar);

            let aliases = event_power_level(
                &power_levels,
                &TimelineEventType::RoomCanonicalAlias,
                state_default,
            );
            self.aliases_row.set_selected_power_level(aliases);
            self.aliases_row
                .set_read_only(!editable || own_pl < aliases);

            let history_visibility = event_power_level(
                &power_levels,
                &TimelineEventType::RoomHistoryVisibility,
                state_default,
            );
            self.history_visibility_row
                .set_selected_power_level(history_visibility);
            self.history_visibility_row
                .set_read_only(!editable || own_pl < history_visibility);

            let encryption = event_power_level(
                &power_levels,
                &TimelineEventType::RoomEncryption,
                state_default,
            );
            self.encryption_row.set_selected_power_level(encryption);
            self.encryption_row
                .set_read_only(!editable || own_pl < encryption);

            let pl = event_power_level(
                &power_levels,
                &TimelineEventType::RoomPowerLevels,
                state_default,
            );
            self.power_levels_row.set_selected_power_level(pl);
            self.power_levels_row
                .set_read_only(!editable || own_pl < pl);

            let server_acl = event_power_level(
                &power_levels,
                &TimelineEventType::RoomServerAcl,
                state_default,
            );
            self.server_acl_row.set_selected_power_level(server_acl);
            self.server_acl_row
                .set_read_only(!editable || own_pl < server_acl);

            let upgrade = event_power_level(
                &power_levels,
                &TimelineEventType::RoomTombstone,
                state_default,
            );
            self.upgrade_row.set_selected_power_level(upgrade);
            self.upgrade_row
                .set_read_only(!editable || own_pl < upgrade);
        }

        /// Update the member actions section.
        fn update_member_actions(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let editable = self.editable.get();
            let power_levels = permissions.power_levels();
            let own_pl = permissions.own_power_level();

            let invite = power_levels.invite.into();
            self.invite_row.set_selected_power_level(invite);
            self.invite_row.set_read_only(!editable || own_pl < invite);

            let kick = power_levels.kick.into();
            self.kick_row.set_selected_power_level(kick);
            self.kick_row.set_read_only(!editable || own_pl < kick);

            let ban = power_levels.ban.into();
            self.ban_row.set_selected_power_level(ban);
            self.ban_row.set_read_only(!editable || own_pl < ban);
        }

        /// Update the member roles section.
        fn update_members_power_levels(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };
            let power_levels = permissions.power_levels();

            let default_pl = power_levels.users_default;
            self.members_default_adjustment
                .set_value(i64::from(default_pl) as f64);
            self.members_default_label
                .set_label(&default_pl.to_string());

            let own_pl = permissions.own_power_level();
            let own_max = if let UserPowerLevel::Int(pl) = own_pl {
                // We cannot change any power level to something higher than ours.
                i64::from(pl)
            } else {
                // We can change the power level to any valid value.
                POWER_LEVEL_MAX
            };
            let max = i64::from(default_pl).max(own_max);
            self.members_default_adjustment.set_upper(max as f64);

            let editable = self.editable.get();
            let can_change_default = editable && own_pl >= default_pl;
            self.members_default_spin_row
                .set_visible(can_change_default);
            self.members_default_text_row
                .set_visible(!can_change_default);

            self.members_privileged_button
                .set_count(power_levels.users.len().to_string());
        }

        /// Go back to the previous page in the room details.
        ///
        /// If there are changes in the page, ask the user to confirm.
        #[template_callback]
        async fn go_back(&self) {
            let obj = self.obj();
            let mut reset_after = false;

            if self.changed.get() {
                match unsaved_changes_dialog(&*obj).await {
                    UnsavedChangesResponse::Save => self.save().await,
                    UnsavedChangesResponse::Discard => reset_after = true,
                    UnsavedChangesResponse::Cancel => return,
                }
            }

            let _ = obj.activate_action("navigation.pop", None);

            if reset_after {
                self.update();
            }
        }

        /// Save the changes of this page.
        #[template_callback]
        async fn save(&self) {
            if !self.compute_changed() {
                return;
            }

            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            self.save_button.set_is_loading(true);

            let Some(power_levels) = self.collect_power_levels() else {
                return;
            };

            if permissions.set_power_levels(power_levels).await.is_err() {
                toast!(self.obj(), gettext("Could not save permissions"));
            }
        }

        /// Collect the current power levels.
        ///
        /// Returns `None` if the permissions could not be upgraded.
        #[allow(clippy::too_many_lines)]
        fn collect_power_levels(&self) -> Option<RoomPowerLevels> {
            macro_rules! set_power_level {
                ($power_levels:ident, $field:ident, $value:ident) => {
                    set_power_level_inner(&mut $power_levels.$field, stringify!($field), $value);
                };
                ($power_levels:ident, $field:ident.$nested:ident , $value:ident) => {
                    set_power_level_inner(
                        &mut $power_levels.$field.$nested,
                        stringify!($field.$nested),
                        $value,
                    );
                };
            }

            let permissions = self.permissions.obj()?;

            let mut power_levels = permissions.power_levels();

            let events_default = self.messages_row.selected_power_level();
            set_power_level!(power_levels, events_default, events_default);

            let mut redact_own = self.redact_own_row.selected_power_level();
            let redact_others = self.redact_others_row.selected_power_level();

            // redact_own cannot be higher than redact_others because redact_others depends
            // also on redact_own.
            redact_own = redact_own.min(redact_others);
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomRedaction,
                redact_own,
                events_default,
            );

            set_power_level!(power_levels, redact, redact_others);

            let notify_room = self.notify_room_row.selected_power_level();
            set_power_level!(power_levels, notifications.room, notify_room);

            let state_default = self.state_row.selected_power_level();
            set_power_level!(power_levels, state_default, state_default);

            let name = self.name_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomName,
                name,
                state_default,
            );

            let topic = self.topic_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomTopic,
                topic,
                state_default,
            );

            let avatar = self.avatar_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomAvatar,
                avatar,
                state_default,
            );

            let aliases = self.aliases_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomCanonicalAlias,
                aliases,
                state_default,
            );

            let history_visibility = self.history_visibility_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomHistoryVisibility,
                history_visibility,
                state_default,
            );

            let encryption = self.encryption_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomEncryption,
                encryption,
                state_default,
            );

            let pl = self.power_levels_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomPowerLevels,
                pl,
                state_default,
            );

            let server_acl = self.server_acl_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomServerAcl,
                server_acl,
                state_default,
            );

            let upgrade = self.upgrade_row.selected_power_level();
            set_event_power_level(
                &mut power_levels,
                TimelineEventType::RoomTombstone,
                upgrade,
                state_default,
            );

            let invite = self.invite_row.selected_power_level();
            set_power_level!(power_levels, invite, invite);

            let kick = self.kick_row.selected_power_level();
            set_power_level!(power_levels, kick, kick);

            let ban = self.ban_row.selected_power_level();
            set_power_level!(power_levels, ban, ban);

            let default_pl = self.members_default_adjustment.value() as i64;
            power_levels.users_default = Int::new_saturating(default_pl);

            let privileged_members = self.privileged_members();
            power_levels.users = privileged_members.collect();

            Some(power_levels)
        }

        /// Handle when a value in the page has changed.
        #[template_callback]
        fn value_changed(&self) {
            if self.update_in_progress.get() {
                // No need to run checks.
                return;
            }

            self.update_changed();
        }

        /// Handle when the `redact_own` row has changed.
        #[template_callback]
        fn redact_own_changed(&self) {
            if self.update_in_progress.get() {
                // No need to run checks.
                return;
            }

            let redact_own = self.redact_own_row.selected_power_level();
            let redact_others = self.redact_others_row.selected_power_level();

            // redact_own cannot be higher than redact_others because redact_others depends
            // also on redact_own.
            if redact_others < redact_own {
                self.update_in_progress.set(true);

                self.redact_others_row.set_selected_power_level(redact_own);

                self.update_in_progress.set(false);
            }

            self.update_changed();
        }

        /// Handle when the `redact_others` row has changed.
        #[template_callback]
        fn redact_others_changed(&self) {
            if self.update_in_progress.get() {
                // No need to run checks.
                return;
            }

            let redact_own = self.redact_own_row.selected_power_level();
            let redact_others = self.redact_others_row.selected_power_level();

            // redact_own cannot be higher than redact_others because redact_others depends
            // also on redact_own.
            if redact_others < redact_own {
                self.update_in_progress.set(true);

                self.redact_own_row.set_selected_power_level(redact_others);

                self.update_in_progress.set(false);
            }

            self.update_changed();
        }

        /// Handle when the state default has changed.
        #[template_callback]
        fn state_default_changed(&self) {
            if self.update_in_progress.get() {
                // No need to run checks.
                return;
            }

            self.update_in_progress.set(true);

            self.update_state_rows();

            self.update_in_progress.set(false);
            self.update_changed();
        }
    }
}

glib::wrapper! {
    /// Subpage to view and change the permissions of a room.
    pub struct PermissionsSubpage(ObjectSubclass<imp::PermissionsSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PermissionsSubpage {
    pub fn new(permissions: &Permissions) -> Self {
        glib::Object::builder()
            .property("permissions", permissions)
            .build()
    }
}

/// Set the power level for the given field.
fn set_power_level_inner(current_value: &mut Int, field_name: &str, new_value: UserPowerLevel) {
    let UserPowerLevel::Int(new_value) = new_value else {
        error!("Cannot set power level for field `{field_name}` to infinite");
        return;
    };

    *current_value = new_value;
}

/// Set the power level for the given event type in the given power levels.
fn set_event_power_level(
    power_levels: &mut RoomPowerLevels,
    event_type: TimelineEventType,
    value: UserPowerLevel,
    default: UserPowerLevel,
) {
    if value == default {
        power_levels.events.remove(&event_type);
    } else {
        let UserPowerLevel::Int(value) = value else {
            error!("Cannot set power level for event `{event_type}` to infinite");
            return;
        };

        power_levels.events.insert(event_type, value);
    }
}

/// Get the necessary power level for the given event type in the given power
/// levels.
fn event_power_level(
    power_levels: &RoomPowerLevels,
    event_type: &TimelineEventType,
    default: UserPowerLevel,
) -> UserPowerLevel {
    power_levels
        .events
        .get(event_type)
        .copied()
        .map_or(default, Into::into)
}
