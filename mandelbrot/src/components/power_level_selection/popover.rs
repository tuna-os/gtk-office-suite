use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};
use ruma::events::room::power_levels::UserPowerLevel;

use crate::{
    session::{POWER_LEVEL_ADMIN, POWER_LEVEL_MAX, POWER_LEVEL_MOD, Permissions},
    utils::BoundObject,
};

mod imp {
    use std::cell::Cell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/power_level_selection/popover.ui")]
    #[properties(wrapper_type = super::PowerLevelSelectionPopover)]
    pub struct PowerLevelSelectionPopover {
        #[template_child]
        admin_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        admin_selected: TemplateChild<gtk::Image>,
        #[template_child]
        mod_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        mod_selected: TemplateChild<gtk::Image>,
        #[template_child]
        default_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        default_pl_label: TemplateChild<gtk::Label>,
        #[template_child]
        default_selected: TemplateChild<gtk::Image>,
        #[template_child]
        muted_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        muted_pl_label: TemplateChild<gtk::Label>,
        #[template_child]
        muted_selected: TemplateChild<gtk::Image>,
        #[template_child]
        custom_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        custom_adjustment: TemplateChild<gtk::Adjustment>,
        #[template_child]
        custom_confirm: TemplateChild<gtk::Button>,
        /// The permissions to watch.
        #[property(get, set = Self::set_permissions, explicit_notify, nullable)]
        permissions: BoundObject<Permissions>,
        /// The selected power level.
        #[property(get, set = Self::set_selected_power_level, explicit_notify)]
        selected_power_level: Cell<i64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PowerLevelSelectionPopover {
        const NAME: &'static str = "PowerLevelSelectionPopover";
        type Type = super::PowerLevelSelectionPopover;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PowerLevelSelectionPopover {}

    impl WidgetImpl for PowerLevelSelectionPopover {}
    impl PopoverImpl for PowerLevelSelectionPopover {}

    #[gtk::template_callbacks]
    impl PowerLevelSelectionPopover {
        /// Set the permissions to watch.
        fn set_permissions(&self, permissions: Option<Permissions>) {
            if self.permissions.obj() == permissions {
                return;
            }

            self.permissions.disconnect_signals();

            if let Some(permissions) = permissions {
                let own_pl_handler = permissions.connect_own_power_level_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update();
                    }
                ));
                let default_pl_handler = permissions.connect_default_power_level_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_default();
                        imp.update_muted();
                        imp.update_selection();
                    }
                ));
                let muted_pl_handler = permissions.connect_mute_power_level_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_muted();
                        imp.update_selection();
                    }
                ));

                self.permissions.set(
                    permissions,
                    vec![own_pl_handler, default_pl_handler, muted_pl_handler],
                );
            }

            self.update();
            self.obj().notify_permissions();
        }

        /// Set the selected power level.
        fn set_selected_power_level(&self, power_level: i64) {
            if self.selected_power_level.get() == power_level {
                return;
            }

            self.selected_power_level.set(power_level);

            self.update_selection();
            self.update_custom();
            self.obj().notify_selected_power_level();
        }

        /// Update the rows.
        fn update(&self) {
            self.update_admin();
            self.update_mod();
            self.update_default();
            self.update_muted();
            self.update_custom();
            self.update_selection();
        }

        /// Update the admin row.
        fn update_admin(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let can_change_to_admin = permissions.can_set_user_power_level_to(POWER_LEVEL_ADMIN);

            self.admin_row.set_sensitive(can_change_to_admin);
            self.admin_row.set_activatable(can_change_to_admin);
        }

        /// Update the moderator row.
        fn update_mod(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let can_change_to_mod = permissions.can_set_user_power_level_to(POWER_LEVEL_MOD);

            self.mod_row.set_sensitive(can_change_to_mod);
            self.mod_row.set_activatable(can_change_to_mod);
        }

        /// Update the default row.
        fn update_default(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let default = permissions.default_power_level();
            self.default_pl_label.set_label(&default.to_string());

            let can_change_to_default = permissions.can_set_user_power_level_to(default);

            self.default_row.set_sensitive(can_change_to_default);
            self.default_row.set_activatable(can_change_to_default);
        }

        /// Update the muted row.
        fn update_muted(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let mute = permissions.mute_power_level();
            let default = permissions.default_power_level();

            if mute >= default {
                // There is no point in having the muted row since all users are muted by
                // default.
                self.muted_row.set_visible(false);
                return;
            }

            self.muted_pl_label.set_label(&mute.to_string());

            let can_change_to_muted = permissions.can_set_user_power_level_to(mute);

            self.muted_row.set_sensitive(can_change_to_muted);
            self.muted_row.set_activatable(can_change_to_muted);

            self.muted_row.set_visible(true);
        }

        /// Update the custom row.
        fn update_custom(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let max = if let UserPowerLevel::Int(value) = permissions.own_power_level() {
                i64::from(value)
            } else {
                POWER_LEVEL_MAX
            };
            self.custom_adjustment.set_upper(max as f64);

            self.custom_adjustment
                .set_value(self.selected_power_level.get() as f64);
        }

        /// Update the selected row.
        fn update_selection(&self) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let power_level = self.selected_power_level.get();

            self.admin_selected
                .set_opacity((power_level == POWER_LEVEL_ADMIN).into());
            self.mod_selected
                .set_opacity((power_level == POWER_LEVEL_MOD).into());
            self.default_selected
                .set_opacity((power_level == permissions.default_power_level()).into());
            self.muted_selected
                .set_opacity((power_level == permissions.mute_power_level()).into());
        }

        /// The custom value changed.
        #[template_callback]
        fn custom_value_changed(&self) {
            let power_level = self.custom_adjustment.value() as i64;
            let can_confirm = power_level != self.selected_power_level.get();

            self.custom_confirm.set_sensitive(can_confirm);
        }

        /// The custom value was confirmed.
        #[template_callback]
        fn custom_value_confirmed(&self) {
            let power_level = self.custom_adjustment.value() as i64;

            self.obj().popdown();
            self.set_selected_power_level(power_level);
        }

        /// A row was activated.
        #[template_callback]
        fn row_activated(&self, row: &gtk::ListBoxRow) {
            let Some(permissions) = self.permissions.obj() else {
                return;
            };

            let power_level = match row.index() {
                0 => POWER_LEVEL_ADMIN,
                1 => POWER_LEVEL_MOD,
                2 => permissions.default_power_level(),
                3 => permissions.mute_power_level(),
                _ => return,
            };

            self.obj().popdown();
            self.set_selected_power_level(power_level);
        }
    }
}

glib::wrapper! {
    /// A popover to select a room member's power level.
    pub struct PowerLevelSelectionPopover(ObjectSubclass<imp::PowerLevelSelectionPopover>)
        @extends gtk::Widget, gtk::Popover,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native, gtk::ShortcutManager;
}

impl PowerLevelSelectionPopover {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
