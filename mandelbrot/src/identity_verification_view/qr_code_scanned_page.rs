use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    components::LoadingButton, gettext_f, prelude::*, session::IdentityVerification, toast,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/qr_code_scanned_page.ui"
    )]
    #[properties(wrapper_type = super::QrCodeScannedPage)]
    pub struct QrCodeScannedPage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, explicit_notify, nullable)]
        pub verification: glib::WeakRef<IdentityVerification>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub message: TemplateChild<gtk::Label>,
        #[template_child]
        pub cancel_btn: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QrCodeScannedPage {
        const NAME: &'static str = "IdentityVerificationQrCodeScannedPage";
        type Type = super::QrCodeScannedPage;
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
    impl ObjectImpl for QrCodeScannedPage {
        fn dispose(&self) {
            if let Some(verification) = self.verification.upgrade()
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for QrCodeScannedPage {}
    impl BinImpl for QrCodeScannedPage {}

    impl QrCodeScannedPage {
        /// Set the current identity verification.
        fn set_verification(&self, verification: Option<&IdentityVerification>) {
            let prev_verification = self.verification.upgrade();

            if prev_verification.as_ref() == verification {
                return;
            }
            let obj = self.obj();

            if let Some(verification) = prev_verification
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }

            if let Some(verification) = verification {
                let display_name_handler = verification.user().connect_display_name_notify(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.update_labels();
                    }
                ));
                self.display_name_handler
                    .replace(Some(display_name_handler));
            }

            self.verification.set(verification);

            obj.update_labels();
            obj.notify_verification();
        }
    }
}

glib::wrapper! {
    /// A page to show when a QR code was scanned.
    pub struct QrCodeScannedPage(ObjectSubclass<imp::QrCodeScannedPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl QrCodeScannedPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update the labels for the current verification.
    fn update_labels(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let imp = self.imp();

        if verification.is_self_verification() {
            imp.message.set_label(&gettext("You scanned the QR code successfully. You may need to confirm the verification from the other session."));
        } else {
            let name = verification.user().display_name();
            // Translators: Do NOT translate the content between '{' and '}', this is a
            // variable name.
            imp.message.set_markup(&gettext_f("You scanned the QR code successfully. {user} may need to confirm the verification.", &[("user", &format!("<b>{name}</b>"))]));
        }
    }

    /// Reset the UI to its initial state.
    pub fn reset(&self) {
        self.imp().cancel_btn.set_is_loading(false);
        self.set_sensitive(true);
    }

    /// Cancel the verification.
    #[template_callback]
    async fn cancel(&self) {
        let Some(verification) = self.verification() else {
            return;
        };

        self.imp().cancel_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.cancel().await.is_err() {
            toast!(self, gettext("Could not cancel the verification"));
            self.reset();
        }
    }
}
