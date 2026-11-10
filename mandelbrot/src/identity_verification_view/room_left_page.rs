use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::session::IdentityVerification;

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/room_left_page.ui")]
    #[properties(wrapper_type = super::RoomLeftPage)]
    pub struct RoomLeftPage {
        /// The current identity verification.
        #[property(get, set, nullable)]
        pub verification: glib::WeakRef<IdentityVerification>,
        #[template_child]
        pub dismiss_btn: TemplateChild<gtk::Button>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomLeftPage {
        const NAME: &'static str = "IdentityVerificationRoomLeftPage";
        type Type = super::RoomLeftPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::Type::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoomLeftPage {}

    impl WidgetImpl for RoomLeftPage {
        fn grab_focus(&self) -> bool {
            self.dismiss_btn.grab_focus()
        }
    }

    impl BinImpl for RoomLeftPage {}
}

glib::wrapper! {
    /// A page to show when a verification request was cancelled because the room where it happened was left.
    pub struct RoomLeftPage(ObjectSubclass<imp::RoomLeftPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl RoomLeftPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Dismiss the verification.
    #[template_callback]
    fn dismiss(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        verification.dismiss();
    }
}
