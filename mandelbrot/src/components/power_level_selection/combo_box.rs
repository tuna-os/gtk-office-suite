use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, glib};
use ruma::{Int, events::room::power_levels::UserPowerLevel};

use super::PowerLevelSelectionPopover;
use crate::{components::RoleBadge, session::Permissions};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/components/power_level_selection/combo_box.ui"
    )]
    #[properties(wrapper_type = super::PowerLevelSelectionComboBox)]
    pub struct PowerLevelSelectionComboBox {
        #[template_child]
        selected_level_label: TemplateChild<gtk::Label>,
        #[template_child]
        selected_role_badge: TemplateChild<RoleBadge>,
        #[template_child]
        popover: TemplateChild<PowerLevelSelectionPopover>,
        /// The permissions to watch.
        #[property(get, set = Self::set_permissions, explicit_notify, nullable)]
        permissions: RefCell<Option<Permissions>>,
        /// The selected power level.
        #[property(get, set = Self::set_selected_power_level, explicit_notify)]
        selected_power_level: Cell<i64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PowerLevelSelectionComboBox {
        const NAME: &'static str = "PowerLevelSelectionComboBox";
        type Type = super::PowerLevelSelectionComboBox;
        type ParentType = gtk::ToggleButton;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PowerLevelSelectionComboBox {}

    impl WidgetImpl for PowerLevelSelectionComboBox {}
    impl ButtonImpl for PowerLevelSelectionComboBox {}
    impl ToggleButtonImpl for PowerLevelSelectionComboBox {}

    #[gtk::template_callbacks]
    impl PowerLevelSelectionComboBox {
        /// Set the permissions to watch.
        fn set_permissions(&self, permissions: Option<Permissions>) {
            if *self.permissions.borrow() == permissions {
                return;
            }

            self.permissions.replace(permissions);
            self.update_selected_label();
            self.obj().notify_permissions();
        }

        /// Update the label of the selected power level.
        fn update_selected_label(&self) {
            let Some(permissions) = self.permissions.borrow().clone() else {
                return;
            };

            let power_level = self.selected_power_level.get();
            let role = permissions.role(UserPowerLevel::Int(Int::new_saturating(power_level)));

            self.selected_role_badge.set_role(role);
            self.selected_level_label
                .set_label(&power_level.to_string());

            let role_string = format!("{power_level} {role}");
            self.obj()
                .update_property(&[gtk::accessible::Property::Description(&role_string)]);
        }

        /// Set the selected power level.
        fn set_selected_power_level(&self, power_level: i64) {
            if self.selected_power_level.get() == power_level {
                return;
            }

            self.selected_power_level.set(power_level);

            self.update_selected_label();
            self.obj().notify_selected_power_level();
        }

        /// The toggle button's changed.
        #[template_callback]
        fn active_changed(&self) {
            let obj = self.obj();

            if obj.is_active() {
                self.popover
                    .set_pointing_to(Some(&gdk::Rectangle::new(0, 0, 0, obj.height())));
                self.popover.popup();
            } else {
                self.popover.popdown();
            }
        }
    }
}

glib::wrapper! {
    /// A combo box to select a room member's power level.
    pub struct PowerLevelSelectionComboBox(ObjectSubclass<imp::PowerLevelSelectionComboBox>)
        @extends gtk::Widget, gtk::Button, gtk::ToggleButton,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl PowerLevelSelectionComboBox {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
