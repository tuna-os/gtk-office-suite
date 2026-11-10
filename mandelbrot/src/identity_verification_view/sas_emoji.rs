use adw::subclass::prelude::*;
use gtk::glib;

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/sas_emoji.ui")]
    pub struct SasEmoji {
        #[template_child]
        pub emoji: TemplateChild<gtk::Label>,
        #[template_child]
        pub emoji_name: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SasEmoji {
        const NAME: &'static str = "IdentityVerificationSasEmoji";
        type Type = super::SasEmoji;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SasEmoji {}
    impl WidgetImpl for SasEmoji {}
    impl BinImpl for SasEmoji {}
}

glib::wrapper! {
    /// An emoji for SAS verification.
    pub struct SasEmoji(ObjectSubclass<imp::SasEmoji>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SasEmoji {
    pub fn new(symbol: &str, name: &str) -> Self {
        let obj: Self = glib::Object::new();

        obj.set_emoji(symbol, name);
        obj
    }

    /// Set the emoji.
    pub fn set_emoji(&self, symbol: &str, name: &str) {
        let imp = self.imp();

        imp.emoji.set_text(symbol);
        imp.emoji_name.set_text(name);
    }
}
