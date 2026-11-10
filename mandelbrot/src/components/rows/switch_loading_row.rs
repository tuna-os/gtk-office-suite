use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use crate::utils::bool_to_accessible_tristate;

mod imp {
    use std::{cell::Cell, marker::PhantomData};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/switch_loading_row.ui")]
    #[properties(wrapper_type = super::SwitchLoadingRow)]
    pub struct SwitchLoadingRow {
        #[template_child]
        spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        switch: TemplateChild<gtk::Switch>,
        /// Whether the switch is active.
        #[property(get = Self::is_active, set = Self::set_is_active)]
        is_active: PhantomData<bool>,
        /// Whether the row is loading.
        #[property(get = Self::is_loading, set = Self::set_is_loading)]
        is_loading: PhantomData<bool>,
        /// Whether the row is read-only.
        #[property(get, set = Self::set_read_only, explicit_notify)]
        read_only: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SwitchLoadingRow {
        const NAME: &'static str = "SwitchLoadingRow";
        type Type = super::SwitchLoadingRow;
        type ParentType = adw::ActionRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Switch);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SwitchLoadingRow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.switch.connect_active_notify(clone!(
                #[weak]
                obj,
                move |switch| {
                    obj.update_state(&[gtk::accessible::State::Checked(
                        bool_to_accessible_tristate(switch.is_active()),
                    )]);
                    obj.notify_is_active();
                }
            ));
            obj.update_state(&[gtk::accessible::State::Checked(
                bool_to_accessible_tristate(self.switch.is_active()),
            )]);
        }
    }

    impl WidgetImpl for SwitchLoadingRow {}
    impl ListBoxRowImpl for SwitchLoadingRow {}
    impl PreferencesRowImpl for SwitchLoadingRow {}
    impl ActionRowImpl for SwitchLoadingRow {}

    impl SwitchLoadingRow {
        /// Whether the switch is active.
        fn is_active(&self) -> bool {
            self.switch.is_active()
        }

        /// Set whether the switch is active.
        fn set_is_active(&self, active: bool) {
            if self.is_active() == active {
                return;
            }

            self.switch.set_active(active);
            self.obj().notify_is_active();
        }

        /// Whether the row is loading.
        fn is_loading(&self) -> bool {
            self.spinner.is_visible()
        }

        /// Set whether the row is loading.
        fn set_is_loading(&self, loading: bool) {
            if self.is_loading() == loading {
                return;
            }

            self.spinner.set_visible(loading);
            self.obj().notify_is_loading();
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
    }
}

glib::wrapper! {
    /// An `AdwActionRow` with a switch and a loading state.
    pub struct SwitchLoadingRow(ObjectSubclass<imp::SwitchLoadingRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow, adw::ActionRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl SwitchLoadingRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
