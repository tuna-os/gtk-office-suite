use adw::subclass::prelude::*;
use gtk::{glib, glib::clone, prelude::*};

use crate::{components::LoadingBin, utils::bool_to_accessible_tristate};

mod imp {
    use std::marker::PhantomData;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/check_loading_row.ui")]
    #[properties(wrapper_type = super::CheckLoadingRow)]
    pub struct CheckLoadingRow {
        #[template_child]
        bin: TemplateChild<LoadingBin>,
        #[template_child]
        check: TemplateChild<gtk::CheckButton>,
        /// The action activated by the button.
        #[property(get = Self::action_name, set = Self::set_action_name, override_interface = gtk::Actionable)]
        action_name: PhantomData<Option<glib::GString>>,
        /// The target value of the action of the button.
        #[property(get = Self::action_target_value, set = Self::set_action_target, override_interface = gtk::Actionable)]
        action_target: PhantomData<Option<glib::Variant>>,
        /// Whether the row is loading.
        #[property(get = Self::is_loading, set = Self::set_is_loading)]
        is_loading: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CheckLoadingRow {
        const NAME: &'static str = "CheckLoadingRow";
        type Type = super::CheckLoadingRow;
        type ParentType = adw::ActionRow;
        type Interfaces = (gtk::Actionable,);

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CheckLoadingRow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.check.connect_active_notify(clone!(
                #[weak]
                obj,
                move |check| {
                    obj.update_state(&[gtk::accessible::State::Checked(
                        bool_to_accessible_tristate(check.is_active()),
                    )]);
                }
            ));
            obj.update_state(&[gtk::accessible::State::Checked(
                bool_to_accessible_tristate(self.check.is_active()),
            )]);
        }
    }

    impl WidgetImpl for CheckLoadingRow {}
    impl ListBoxRowImpl for CheckLoadingRow {}
    impl PreferencesRowImpl for CheckLoadingRow {}
    impl ActionRowImpl for CheckLoadingRow {}

    impl ActionableImpl for CheckLoadingRow {
        fn action_name(&self) -> Option<glib::GString> {
            self.check.action_name()
        }

        fn action_target_value(&self) -> Option<glib::Variant> {
            self.check.action_target_value()
        }

        fn set_action_name(&self, name: Option<&str>) {
            self.check.set_action_name(name);
        }

        fn set_action_target_value(&self, value: Option<&glib::Variant>) {
            self.check.set_action_target(value);
        }
    }

    impl CheckLoadingRow {
        /// Set the target value of the action of the button.
        #[allow(clippy::needless_pass_by_value)] // glib::Properties macro does not work with ref.
        fn set_action_target(&self, value: Option<glib::Variant>) {
            self.set_action_target_value(value.as_ref());
        }

        /// Whether the row is loading.
        fn is_loading(&self) -> bool {
            self.bin.is_loading()
        }

        /// Set whether the row is loading.
        fn set_is_loading(&self, loading: bool) {
            if self.is_loading() == loading {
                return;
            }

            self.bin.set_is_loading(loading);
            self.obj().notify_is_loading();
        }
    }
}

glib::wrapper! {
    /// An `AdwActionRow` with a check button and a loading state.
    pub struct CheckLoadingRow(ObjectSubclass<imp::CheckLoadingRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow, adw::ActionRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl CheckLoadingRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
