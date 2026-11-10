use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{glib, glib::clone, prelude::*};

use crate::{
    components::LoadingButton, gettext_f, prelude::*, session::IdentityVerification, toast,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/accept_request_page.ui"
    )]
    #[properties(wrapper_type = super::AcceptRequestPage)]
    pub struct AcceptRequestPage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, explicit_notify, nullable)]
        pub verification: glib::WeakRef<IdentityVerification>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub instructions: TemplateChild<gtk::Label>,
        #[template_child]
        pub decline_btn: TemplateChild<LoadingButton>,
        #[template_child]
        pub accept_btn: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AcceptRequestPage {
        const NAME: &'static str = "IdentityVerificationAcceptRequestPage";
        type Type = super::AcceptRequestPage;
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
    impl ObjectImpl for AcceptRequestPage {
        fn dispose(&self) {
            if let Some(verification) = self.verification.upgrade()
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for AcceptRequestPage {
        fn grab_focus(&self) -> bool {
            self.accept_btn.grab_focus()
        }
    }

    impl BinImpl for AcceptRequestPage {}

    impl AcceptRequestPage {
        /// Set the current identity verification.
        fn set_verification(&self, verification: Option<&IdentityVerification>) {
            let prev_verification = self.verification.upgrade();

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
    /// A page to accept or decline a new identity verification request.
    pub struct AcceptRequestPage(ObjectSubclass<imp::AcceptRequestPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl AcceptRequestPage {
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
            imp.title
                .set_label(&gettext("Login Request From Another Session"));
            imp.instructions
                .set_label(&gettext("Verify the new session from the current session."));
        } else {
            let name = verification.user().display_name();
            imp.title.set_markup(&gettext("Verification Request"));
            imp.instructions
                // Translators: Do NOT translate the content between '{' and '}', this is a
                // variable name.
                .set_markup(&gettext_f("{user} asked to be verified. Verifying a user increases the security of the conversation.", &[("user", &format!("<b>{name}</b>"))]));
        }
    }

    /// Reset the UI to its initial state.
    pub fn reset(&self) {
        let imp = self.imp();
        imp.accept_btn.set_is_loading(false);
        imp.decline_btn.set_is_loading(false);
        self.set_sensitive(true);
    }

    /// Decline the verification request.
    #[template_callback]
    async fn decline(&self) {
        let Some(verification) = self.verification() else {
            return;
        };

        self.imp().decline_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.cancel().await.is_err() {
            toast!(self, gettext("Could not decline verification request"));
            self.reset();
        }
    }

    /// Accept the verification request.
    #[template_callback]
    async fn accept(&self) {
        let Some(verification) = self.verification() else {
            return;
        };

        self.imp().accept_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.accept().await.is_err() {
            toast!(self, gettext("Could not accept verification request"));
            self.reset();
        }
    }
}

impl Default for AcceptRequestPage {
    fn default() -> Self {
        Self::new()
    }
}
