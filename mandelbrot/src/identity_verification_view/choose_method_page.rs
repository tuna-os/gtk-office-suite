use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    components::LoadingButton,
    contrib::QRCode,
    gettext_f,
    prelude::*,
    session::{IdentityVerification, VerificationSupportedMethods},
    toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/choose_method_page.ui"
    )]
    #[properties(wrapper_type = super::ChooseMethodPage)]
    pub struct ChooseMethodPage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, explicit_notify, nullable)]
        pub verification: BoundObjectWeakRef<IdentityVerification>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub instructions: TemplateChild<gtk::Label>,
        #[template_child]
        pub qrcode: TemplateChild<QRCode>,
        #[template_child]
        pub cannot_scan_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub scan_qr_code_btn: TemplateChild<LoadingButton>,
        #[template_child]
        pub start_sas_btn: TemplateChild<LoadingButton>,
        #[template_child]
        pub cancel_btn: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ChooseMethodPage {
        const NAME: &'static str = "IdentityVerificationChooseMethodPage";
        type Type = super::ChooseMethodPage;
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
    impl ObjectImpl for ChooseMethodPage {
        fn dispose(&self) {
            if let Some(verification) = self.verification.obj()
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for ChooseMethodPage {
        fn grab_focus(&self) -> bool {
            if self.scan_qr_code_btn.is_visible() {
                self.scan_qr_code_btn.grab_focus()
            } else if self.start_sas_btn.is_visible() {
                self.start_sas_btn.grab_focus()
            } else {
                self.cancel_btn.grab_focus()
            }
        }
    }

    impl BinImpl for ChooseMethodPage {}

    impl ChooseMethodPage {
        /// Set the current identity verification.
        fn set_verification(&self, verification: Option<&IdentityVerification>) {
            let prev_verification = self.verification.obj();

            if prev_verification.as_ref() == verification {
                return;
            }
            let obj = self.obj();

            obj.reset();

            if let Some(verification) = prev_verification
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
            self.verification.disconnect_signals();

            if let Some(verification) = verification {
                let display_name_handler = verification.user().connect_display_name_notify(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.update_page();
                    }
                ));
                self.display_name_handler
                    .replace(Some(display_name_handler));

                let supported_methods_handler =
                    verification.connect_supported_methods_notify(clone!(
                        #[weak]
                        obj,
                        move |_| {
                            obj.update_page();
                        }
                    ));

                self.verification
                    .set(verification, vec![supported_methods_handler]);
            }

            obj.update_page();
            obj.notify_verification();
        }
    }
}

glib::wrapper! {
    /// A page that shows a QR code or allows to choose between different flows.
    pub struct ChooseMethodPage(ObjectSubclass<imp::ChooseMethodPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl ChooseMethodPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update the UI for the current verification.
    fn update_page(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let imp = self.imp();
        let supported_methods = verification.supported_methods();

        let qr_code_visible = if supported_methods.contains(VerificationSupportedMethods::QR_SHOW) {
            if let Some(qr_code) = verification.qr_code() {
                imp.qrcode.set_qrcode(qr_code);
                true
            } else {
                false
            }
        } else {
            false
        };
        let scan_qr_code_visible =
            supported_methods.contains(VerificationSupportedMethods::QR_SCAN);
        let sas_visible = supported_methods.contains(VerificationSupportedMethods::SAS);

        let options_nb =
            u8::from(qr_code_visible) + u8::from(scan_qr_code_visible) + u8::from(sas_visible);
        let has_several_options = options_nb > 1;

        if verification.is_self_verification() {
            imp.title.set_label(&gettext("Verify Session"));

            if qr_code_visible {
                imp.instructions
                    .set_label(&gettext("Scan this QR code from the other session."));
            }
        } else {
            let name = verification.user().display_name();
            imp.title.set_label(&gettext("Verification Request"));

            if qr_code_visible {
                imp.instructions.set_markup(&gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Ask {user} to scan this QR code from their session.",
                    &[("user", &format!("<b>{name}</b>"))],
                ));
            }
        }

        if !qr_code_visible {
            if has_several_options {
                imp.instructions
                    .set_label(&gettext("Select a verification method to proceed."));
            } else {
                imp.instructions
                    .set_label(&gettext("Click on the verification method to proceed."));
            }
        }

        imp.qrcode.set_visible(qr_code_visible);
        imp.scan_qr_code_btn.set_visible(scan_qr_code_visible);
        imp.start_sas_btn.set_visible(sas_visible);
    }

    /// Reset the UI to its initial state.
    pub fn reset(&self) {
        let imp = self.imp();

        imp.scan_qr_code_btn.set_is_loading(false);
        imp.start_sas_btn.set_is_loading(false);
        imp.cancel_btn.set_is_loading(false);

        self.set_sensitive(true);
    }

    /// Switch to the screen to scan a QR Code.
    #[template_callback]
    async fn start_qr_code_scan(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let imp = self.imp();

        imp.scan_qr_code_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.start_qr_code_scan().await.is_err() {
            toast!(self, gettext("Could not access camera"));
            self.reset();
        }
    }

    /// Start a SAS verification.
    #[template_callback]
    async fn start_sas(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let imp = self.imp();

        imp.start_sas_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.start_sas().await.is_err() {
            toast!(self, gettext("Could not start emoji verification"));
            self.reset();
        }
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
