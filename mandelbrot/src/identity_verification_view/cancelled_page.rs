use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk::encryption::verification::CancelInfo;
use ruma::events::key::verification::cancel::CancelCode;

use crate::{
    components::LoadingButton, gettext_f, prelude::*, session::IdentityVerification, toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/cancelled_page.ui")]
    #[properties(wrapper_type = super::CancelledPage)]
    pub struct CancelledPage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, explicit_notify, nullable)]
        pub verification: BoundObjectWeakRef<IdentityVerification>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub message: TemplateChild<gtk::Label>,
        #[template_child]
        pub try_again_btn: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CancelledPage {
        const NAME: &'static str = "IdentityVerificationCancelledPage";
        type Type = super::CancelledPage;
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
    impl ObjectImpl for CancelledPage {
        fn dispose(&self) {
            if let Some(verification) = self.verification.obj()
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for CancelledPage {
        fn grab_focus(&self) -> bool {
            self.try_again_btn.grab_focus()
        }
    }

    impl BinImpl for CancelledPage {}

    impl CancelledPage {
        /// Set the current identity verification.
        fn set_verification(&self, verification: Option<IdentityVerification>) {
            let prev_verification = self.verification.obj();

            if prev_verification == verification {
                return;
            }
            let obj = self.obj();

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
                        obj.update_message();
                    }
                ));
                self.display_name_handler
                    .replace(Some(display_name_handler));

                let cancel_info_changed_handler = verification.connect_cancel_info_changed(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.update_message();
                    }
                ));

                self.verification
                    .set(&verification, vec![cancel_info_changed_handler]);
            }

            obj.update_message();
            obj.notify_verification();
        }
    }
}

glib::wrapper! {
    /// A page to show when the verification was cancelled.
    pub struct CancelledPage(ObjectSubclass<imp::CancelledPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl CancelledPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update the labels for the current verification.
    fn update_message(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let cancel_info = verification.cancel_info();
        let imp = self.imp();

        let message = match cancel_info.as_ref().map(CancelInfo::cancel_code) {
            Some(CancelCode::User) => {
                if verification.is_self_verification() {
                    gettext("The verification was cancelled from the other session.")
                } else {
                    let name = verification.user().display_name();
                    gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', this is a
                        // variable name.
                        "The verification was cancelled by {user}.",
                        &[("user", &format!("<b>{name}</b>"))],
                    )
                }
            }
            Some(CancelCode::Timeout) => {
                gettext("The verification process failed because it reached a timeout.")
            }
            Some(CancelCode::Accepted) => gettext("You accepted the request from another session."),
            Some(CancelCode::MismatchedSas) => {
                if verification.sas_supports_emoji() {
                    gettext("The emoji did not match.")
                } else {
                    gettext("The numbers did not match.")
                }
            }
            _ => gettext("An unexpected error happened during the verification process."),
        };
        imp.message.set_markup(&message);

        let title = if cancel_info.is_some() {
            gettext("Verification Cancelled")
        } else {
            gettext("Verification Error")
        };
        imp.title.set_text(&title);

        // If the verification was started by one of our other devices, let it offer to
        // try again.
        let offer_to_retry = !verification.is_self_verification() || verification.started_by_us();
        imp.try_again_btn.set_visible(offer_to_retry);
    }

    /// Reset the UI to its initial state.
    pub fn reset(&self) {
        self.imp().try_again_btn.set_is_loading(false);
        self.set_sensitive(true);
    }

    /// Send a new request to replace the verification.
    #[template_callback]
    async fn try_again(&self) {
        let Some(verification) = self.verification() else {
            return;
        };

        self.imp().try_again_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.restart().await.is_err() {
            toast!(self, gettext("Could not send a new verification request"));
            self.reset();
        }
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
