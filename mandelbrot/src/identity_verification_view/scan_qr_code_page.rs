use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk::encryption::verification::QrVerificationData;
use tracing::error;

use crate::{
    components::{LoadingButton, QrCodeScanner, ScannedQrCode},
    gettext_f,
    prelude::*,
    session::{IdentityVerification, VerificationSupportedMethods},
    spawn, toast,
    utils::BoundConstructOnlyObject,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/scan_qr_code_page.ui"
    )]
    #[properties(wrapper_type = super::ScanQrCodePage)]
    pub struct ScanQrCodePage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, construct_only)]
        pub verification: BoundConstructOnlyObject<IdentityVerification>,
        /// The QR code scanner to use.
        #[property(get)]
        pub qrcode_scanner: BoundConstructOnlyObject<QrCodeScanner>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub instructions: TemplateChild<gtk::Label>,
        #[template_child]
        qrcode_scanner_bin: TemplateChild<adw::Bin>,
        #[template_child]
        pub show_qr_code_btn: TemplateChild<gtk::Button>,
        #[template_child]
        pub start_sas_btn: TemplateChild<LoadingButton>,
        #[template_child]
        pub cancel_btn: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ScanQrCodePage {
        const NAME: &'static str = "IdentityVerificationScanQrCodePage";
        type Type = super::ScanQrCodePage;
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
    impl ObjectImpl for ScanQrCodePage {
        fn dispose(&self) {
            if let Some(handler) = self.display_name_handler.take() {
                self.verification.obj().user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for ScanQrCodePage {}
    impl BinImpl for ScanQrCodePage {}

    impl ScanQrCodePage {
        /// Set the current identity verification.
        fn set_verification(&self, verification: IdentityVerification) {
            let obj = self.obj();

            let display_name_handler = verification.user().connect_display_name_notify(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.update_labels();
                }
            ));
            self.display_name_handler
                .replace(Some(display_name_handler));

            let supported_methods_handler = verification.connect_supported_methods_notify(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.update_page();
                }
            ));

            self.verification
                .set(verification, vec![supported_methods_handler]);

            self.init_qrcode_scanner();
            obj.update_labels();
            obj.update_page();
            obj.notify_verification();
        }

        /// Initialize the QR code scanner.
        fn init_qrcode_scanner(&self) {
            let Some(qrcode_scanner) = self.verification.obj().qrcode_scanner() else {
                // This is a programmer error.
                error!("Could not show QR code scanner: not found");
                return;
            };

            self.qrcode_scanner_bin.set_child(Some(&qrcode_scanner));

            let obj = self.obj();
            let qrcode_detected_handler = qrcode_scanner.connect_qrcode_detected(clone!(
                #[weak]
                obj,
                move |_, data| {
                    obj.qrcode_detected(data);
                }
            ));

            self.qrcode_scanner
                .set(qrcode_scanner, vec![qrcode_detected_handler]);
        }
    }
}

glib::wrapper! {
    /// A page to scan a QR code.
    pub struct ScanQrCodePage(ObjectSubclass<imp::ScanQrCodePage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl ScanQrCodePage {
    pub fn new(verification: IdentityVerification) -> Self {
        glib::Object::builder()
            .property("verification", verification)
            .build()
    }

    /// Update the labels for the current verification.
    fn update_labels(&self) {
        let imp = self.imp();
        let verification = self.verification();

        if verification.is_self_verification() {
            imp.title.set_label(&gettext("Verify Session"));
            imp.instructions
                .set_label(&gettext("Scan the QR code displayed by the other session."));
        } else {
            let name = verification.user().display_name();
            imp.title.set_markup(&gettext("Verification Request"));
            imp.instructions.set_markup(&gettext_f(
                // Translators: Do NOT translate the content between '{' and '}', this is a
                // variable name.
                "Scan the QR code shown on the device of {user}.",
                &[("user", &format!("<b>{name}</b>"))],
            ));
        }
    }

    /// Update the UI for the available verification methods.
    fn update_page(&self) {
        let verification = self.verification();
        let supported_methods = verification.supported_methods();

        let show_qr_code_visible = supported_methods
            .contains(VerificationSupportedMethods::QR_SHOW)
            && verification.has_qr_code();
        let sas_visible = supported_methods.contains(VerificationSupportedMethods::SAS);

        let imp = self.imp();
        imp.show_qr_code_btn.set_visible(show_qr_code_visible);
        imp.start_sas_btn.set_visible(sas_visible);
    }

    /// Reset the UI to its initial state.
    pub fn reset(&self) {
        let imp = self.imp();

        imp.start_sas_btn.set_is_loading(false);
        imp.cancel_btn.set_is_loading(false);

        self.set_sensitive(true);
    }

    /// Handle a detected QR Code.
    fn qrcode_detected(&self, scanned: ScannedQrCode) {
        let ScannedQrCode::Verification(data) = scanned else {
            toast!(
                self,
                gettext("The scanned QR code is not a verification QR code")
            );
            return;
        };

        self.qr_verification_detected(*data);
    }

    /// Handle a detected identity verification QR Code.
    fn qr_verification_detected(&self, data: QrVerificationData) {
        spawn!(clone!(
            #[weak(rename_to = obj)]
            self,
            async move {
                if obj.verification().qr_code_scanned(data).await.is_err() {
                    toast!(obj, gettext("Could not validate scanned QR Code"));
                }
            }
        ));
    }

    /// Switch to the screen to scan a QR Code.
    #[template_callback]
    fn show_qrcode(&self) {
        self.verification().choose_method();
    }

    /// Start a SAS verification.
    #[template_callback]
    async fn start_sas(&self) {
        let imp = self.imp();

        imp.start_sas_btn.set_is_loading(true);
        self.set_sensitive(false);

        if self.verification().start_sas().await.is_err() {
            toast!(self, gettext("Could not start emoji verification"));
            self.reset();
        }
    }

    /// Cancel the verification.
    #[template_callback]
    async fn cancel(&self) {
        self.imp().cancel_btn.set_is_loading(true);
        self.set_sensitive(false);

        if self.verification().cancel().await.is_err() {
            toast!(self, gettext("Could not cancel the verification"));
            self.reset();
        }
    }
}
