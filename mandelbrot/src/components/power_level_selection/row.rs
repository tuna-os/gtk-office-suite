use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::closure_local};
use ruma::{Int, events::room::power_levels::UserPowerLevel, int};

use super::PowerLevelSelectionPopover;
use crate::{
    components::{LoadingBin, RoleBadge},
    session::Permissions,
};

mod imp {
    use std::{
        cell::{Cell, RefCell},
        marker::PhantomData,
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/power_level_selection/row.ui")]
    #[properties(wrapper_type = super::PowerLevelSelectionRow)]
    pub struct PowerLevelSelectionRow {
        #[template_child]
        subtitle_bin: TemplateChild<adw::Bin>,
        #[template_child]
        combo_selection_bin: TemplateChild<adw::Bin>,
        #[template_child]
        arrow_box: TemplateChild<gtk::Box>,
        #[template_child]
        loading_bin: TemplateChild<LoadingBin>,
        #[template_child]
        popover: TemplateChild<PowerLevelSelectionPopover>,
        #[template_child]
        selected_box: TemplateChild<gtk::Box>,
        #[template_child]
        selected_level_label: TemplateChild<gtk::Label>,
        #[template_child]
        creator_info_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        selected_role_badge: TemplateChild<RoleBadge>,
        /// The permissions to watch.
        #[property(get, set = Self::set_permissions, explicit_notify, nullable)]
        permissions: RefCell<Option<Permissions>>,
        /// The selected power level.
        pub(super) selected_power_level: Cell<UserPowerLevel>,
        /// Whether the selected power level should be displayed in the
        /// subtitle, rather than next to the combo arrow.
        #[property(get, set = Self::set_use_subtitle, explicit_notify)]
        use_subtitle: Cell<bool>,
        /// Whether the row is loading.
        #[property(get = Self::is_loading, set = Self::set_is_loading)]
        is_loading: PhantomData<bool>,
        /// Whether the row is read-only.
        #[property(get, set = Self::set_read_only, explicit_notify)]
        read_only: Cell<bool>,
    }

    impl Default for PowerLevelSelectionRow {
        fn default() -> Self {
            Self {
                subtitle_bin: Default::default(),
                combo_selection_bin: Default::default(),
                arrow_box: Default::default(),
                loading_bin: Default::default(),
                popover: Default::default(),
                selected_box: Default::default(),
                selected_level_label: Default::default(),
                creator_info_button: Default::default(),
                selected_role_badge: Default::default(),
                permissions: Default::default(),
                selected_power_level: Cell::new(UserPowerLevel::Int(int!(0))),
                use_subtitle: Default::default(),
                is_loading: PhantomData,
                read_only: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PowerLevelSelectionRow {
        const NAME: &'static str = "PowerLevelSelectionRow";
        type Type = super::PowerLevelSelectionRow;
        type ParentType = adw::PreferencesRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_accessible_role(gtk::AccessibleRole::ComboBox);

            klass.install_action("power-level-selection-row.popup", None, |obj, _, _| {
                if !obj.read_only() && !obj.is_loading() {
                    obj.imp().popover.popup();
                }
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PowerLevelSelectionRow {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("selected-power-level-changed").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.update_selected_position();
        }
    }

    impl WidgetImpl for PowerLevelSelectionRow {}
    impl ListBoxRowImpl for PowerLevelSelectionRow {}
    impl PreferencesRowImpl for PowerLevelSelectionRow {}

    #[gtk::template_callbacks]
    impl PowerLevelSelectionRow {
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
            let role = permissions.role(power_level);

            self.selected_role_badge.set_role(role);

            let (creator_info_visible, accessible_desc) =
                if let UserPowerLevel::Int(value) = power_level {
                    self.selected_level_label.set_label(&value.to_string());
                    self.popover.set_selected_power_level(i64::from(value));
                    (false, format!("{value} {role}"))
                } else {
                    (true, role.to_string())
                };

            self.creator_info_button.set_visible(creator_info_visible);
            self.selected_level_label.set_visible(!creator_info_visible);

            self.obj()
                .update_property(&[gtk::accessible::Property::Description(&accessible_desc)]);
        }

        /// Set the selected power level.
        pub(super) fn set_selected_power_level(&self, power_level: UserPowerLevel) {
            if self.selected_power_level.get() == power_level {
                return;
            }

            self.selected_power_level.set(power_level);

            self.update_selected_label();
            self.obj()
                .emit_by_name::<()>("selected-power-level-changed", &[]);
        }

        /// Set whether the selected power level should be displayed in the
        /// subtitle, rather than next to the combo arrow.
        fn set_use_subtitle(&self, use_subtitle: bool) {
            if self.use_subtitle.get() == use_subtitle {
                return;
            }

            self.use_subtitle.set(use_subtitle);

            self.update_selected_position();
            self.obj().notify_use_subtitle();
        }

        /// Whether the row is loading.
        fn is_loading(&self) -> bool {
            self.loading_bin.is_loading()
        }

        /// Set whether the row is loading.
        fn set_is_loading(&self, loading: bool) {
            if self.is_loading() == loading {
                return;
            }

            self.loading_bin.set_is_loading(loading);
            self.obj().notify_is_loading();
        }

        /// Update the position of the selected label.
        fn update_selected_position(&self) {
            if self.use_subtitle.get() {
                if self
                    .selected_box
                    .parent()
                    .is_none_or(|p| p != *self.subtitle_bin)
                {
                    if self.selected_box.parent().is_some() {
                        self.combo_selection_bin.set_child(None::<&gtk::Widget>);
                    }

                    self.subtitle_bin.set_child(Some(&*self.selected_box));
                }
            } else if self
                .selected_box
                .parent()
                .is_none_or(|p| p != *self.combo_selection_bin)
            {
                if self.selected_box.parent().is_some() {
                    self.subtitle_bin.set_child(None::<&gtk::Widget>);
                }

                self.combo_selection_bin
                    .set_child(Some(&*self.selected_box));
            }
        }

        /// Set whether the row is read-only.
        fn set_read_only(&self, read_only: bool) {
            if self.read_only.get() == read_only {
                return;
            }
            let obj = self.obj();

            self.read_only.set(read_only);

            obj.update_property(&[gtk::accessible::Property::ReadOnly(read_only)]);
            obj.notify_read_only();
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

        /// The selected power level changed.
        #[template_callback]
        fn power_level_changed(&self) {
            self.set_selected_power_level(UserPowerLevel::Int(Int::new_saturating(
                self.popover.selected_power_level(),
            )));
        }
    }
}

glib::wrapper! {
    /// An `AdwPreferencesRow` behaving like a combo box to select a room member's power level.
    pub struct PowerLevelSelectionRow(ObjectSubclass<imp::PowerLevelSelectionRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl PowerLevelSelectionRow {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The selected power level.
    pub(crate) fn selected_power_level(&self) -> UserPowerLevel {
        self.imp().selected_power_level.get()
    }

    /// Set the selected power level.
    pub(crate) fn set_selected_power_level(&self, power_level: UserPowerLevel) {
        self.imp().set_selected_power_level(power_level);
    }

    /// Connect to the signal emitted when the selected power level changed.
    pub fn connect_power_level_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "selected-power-level-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
