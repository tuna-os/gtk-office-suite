use std::slice;

use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use ruma::{Int, events::room::power_levels::UserPowerLevel};

use super::MemberPowerLevel;
use crate::{
    components::{
        Avatar, PowerLevelSelectionPopover, RoleBadge, confirm_mute_room_member_dialog,
        confirm_own_demotion_dialog, confirm_set_room_member_power_level_same_as_own_dialog,
    },
    prelude::*,
    session::Permissions,
    utils::{BoundObject, key_bindings},
};

mod imp {
    use std::cell::OnceCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/permissions/member_row.ui"
    )]
    #[properties(wrapper_type = super::PermissionsMemberRow)]
    pub struct PermissionsMemberRow {
        #[template_child]
        selected_level_label: TemplateChild<gtk::Label>,
        #[template_child]
        arrow_box: TemplateChild<gtk::Box>,
        #[template_child]
        popover: TemplateChild<PowerLevelSelectionPopover>,
        /// The permissions of the room.
        #[property(get, set = Self::set_permissions, construct_only)]
        permissions: OnceCell<Permissions>,
        /// The room member presented by this row.
        #[property(get, set = Self::set_member, explicit_notify, nullable)]
        member: BoundObject<MemberPowerLevel>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PermissionsMemberRow {
        const NAME: &'static str = "RoomDetailsPermissionsMemberRow";
        type Type = super::PermissionsMemberRow;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            Avatar::ensure_type();
            RoleBadge::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("permissions-member-row");

            klass.install_action("permissions-member.activate", None, |obj, _, _| {
                obj.imp().activate_row();
            });

            key_bindings::add_activate_bindings(klass, "permissions-member.activate");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PermissionsMemberRow {}

    impl WidgetImpl for PermissionsMemberRow {
        fn focus(&self, _direction_type: gtk::DirectionType) -> bool {
            // Regardless of the direction, we can only focus this widget and no children.
            let obj = self.obj();
            if obj.is_focus() {
                false
            } else {
                obj.grab_focus()
            }
        }
    }

    impl BoxImpl for PermissionsMemberRow {}

    #[gtk::template_callbacks]
    impl PermissionsMemberRow {
        /// Set the permissions of the room.
        fn set_permissions(&self, permissions: Permissions) {
            self.permissions
                .set(permissions.clone())
                .expect("permissions should be uninitialized");
            self.popover.set_permissions(Some(permissions));
        }

        /// Set the member displayed by this row.
        fn set_member(&self, member: Option<MemberPowerLevel>) {
            if self.member.obj() == member {
                return;
            }

            self.member.disconnect_signals();

            if let Some(member) = member {
                let power_level_handler = member.connect_power_level_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_power_level();
                    }
                ));
                let editable_handler = member.connect_editable_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_accessible_role();
                    }
                ));

                self.member
                    .set(member, vec![power_level_handler, editable_handler]);
                self.update_power_level();
                self.update_accessible_role();
            }

            self.obj().notify_member();
        }

        /// Update the power level.
        fn update_power_level(&self) {
            let Some(member) = self.member.obj() else {
                return;
            };

            // We should only show power levels with a value.
            let UserPowerLevel::Int(power_level) = member.power_level() else {
                return;
            };

            self.selected_level_label
                .set_label(&power_level.to_string());
            self.popover
                .set_selected_power_level(i64::from(power_level));
        }

        /// Update the accessible role of this row.
        fn update_accessible_role(&self) {
            let Some(member) = self.member.obj() else {
                return;
            };

            let editable = member.editable();

            let role = if editable {
                gtk::AccessibleRole::ComboBox
            } else {
                gtk::AccessibleRole::ListItem
            };
            self.obj().set_accessible_role(role);

            self.arrow_box.set_opacity(editable.into());
        }

        /// The row was activated.
        #[template_callback]
        fn activate_row(&self) {
            let Some(member) = self.member.obj() else {
                return;
            };

            if member.editable() {
                self.popover.popup();
            }
        }

        /// The popover's visibility changed.
        #[template_callback]
        fn popover_visible(&self) {
            let obj = self.obj();
            let is_visible = self.popover.is_visible();

            if is_visible {
                obj.add_css_class("has-open-popup");
            } else {
                obj.remove_css_class("has-open-popup");
            }
        }

        /// The popover's selected power level changed.
        #[template_callback]
        async fn power_level_changed(&self) {
            let Some(member) = self.member.obj() else {
                return;
            };

            let power_level = Int::new_saturating(self.popover.selected_power_level());

            let UserPowerLevel::Int(old_power_level) = member.power_level() else {
                // We should only edit power levels with a value.
                return;
            };

            if power_level == old_power_level {
                // Nothing changed.
                return;
            }

            let permissions = self
                .permissions
                .get()
                .expect("permissions should be initialized");
            let user = member.user();
            let room_power_level = permissions.user_power_level(user.user_id());

            if room_power_level == power_level {
                // The power level was reset to the one in the room, nothing to check.
                member.set_power_level(power_level.into());
                return;
            }

            let obj = self.obj();

            if user.is_own_user() {
                // Warn that demoting oneself is irreversible.
                if !confirm_own_demotion_dialog(&*obj).await {
                    // Reset the value in the popover.
                    self.popover
                        .set_selected_power_level(i64::from(old_power_level));
                    return;
                }
            } else {
                // Warn if user is muted but was not before.
                let mute_power_level = permissions.mute_power_level();
                let is_muted = i64::from(power_level) <= mute_power_level
                    && i64::from(old_power_level) > mute_power_level;
                if is_muted && !confirm_mute_room_member_dialog(slice::from_ref(&user), &*obj).await
                {
                    // Reset the value in the popover.
                    self.popover
                        .set_selected_power_level(i64::from(old_power_level));
                    return;
                }

                // Warn if power level is set at same level as own power level.
                let is_own_power_level = power_level == permissions.own_power_level();
                if is_own_power_level
                    && !confirm_set_room_member_power_level_same_as_own_dialog(
                        slice::from_ref(&user),
                        &*obj,
                    )
                    .await
                {
                    // Reset the value in the popover.
                    self.popover
                        .set_selected_power_level(i64::from(old_power_level));
                    return;
                }
            }

            member.set_power_level(power_level.into());
        }
    }
}

glib::wrapper! {
    /// A row presenting a room member's permission and allowing optionally to edit it.
    pub struct PermissionsMemberRow(ObjectSubclass<imp::PermissionsMemberRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl PermissionsMemberRow {
    pub fn new(permissions: &Permissions) -> Self {
        glib::Object::builder()
            .property("permissions", permissions)
            .build()
    }
}
