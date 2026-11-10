use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

mod imp {
    use std::marker::PhantomData;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/rows/button_count_row.ui")]
    #[properties(wrapper_type = super::ButtonCountRow)]
    pub struct ButtonCountRow {
        #[template_child]
        count_label: TemplateChild<gtk::Label>,
        /// The count that is displayed.
        #[property(get = Self::count, set = Self::set_count, explicit_notify)]
        count: PhantomData<glib::GString>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ButtonCountRow {
        const NAME: &'static str = "ButtonCountRow";
        type Type = super::ButtonCountRow;
        type ParentType = adw::ActionRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ButtonCountRow {}

    impl WidgetImpl for ButtonCountRow {}
    impl ListBoxRowImpl for ButtonCountRow {}
    impl PreferencesRowImpl for ButtonCountRow {}
    impl ActionRowImpl for ButtonCountRow {}

    impl ButtonCountRow {
        /// The count to display.
        fn count(&self) -> glib::GString {
            self.count_label.label()
        }

        /// Set the count to display.
        fn set_count(&self, count: &str) {
            if self.count() == count {
                return;
            }

            self.count_label.set_label(count);
            self.obj().notify_count();
        }
    }
}

glib::wrapper! {
    /// An `AdwPreferencesRow` usable as a button, that optionally displays a count.
    pub struct ButtonCountRow(ObjectSubclass<imp::ButtonCountRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::PreferencesRow, adw::ActionRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl ButtonCountRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
