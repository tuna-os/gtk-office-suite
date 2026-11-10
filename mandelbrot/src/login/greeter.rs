use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::components::OfflineBanner;

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/login/greeter.ui")]
    pub struct Greeter {}

    #[glib::object_subclass]
    impl ObjectSubclass for Greeter {
        const NAME: &'static str = "Greeter";
        type Type = super::Greeter;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Greeter {}
    impl WidgetImpl for Greeter {}

    impl NavigationPageImpl for Greeter {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    impl AccessibleImpl for Greeter {}
}

glib::wrapper! {
    /// The welcome screen of the app.
    pub struct Greeter(ObjectSubclass<imp::Greeter>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Greeter {
    /// The tag for this page.
    pub(super) const TAG: &str = "greeter";

    pub fn new() -> Self {
        glib::Object::new()
    }
}
