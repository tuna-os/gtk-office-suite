use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    components::LoadingButton, gettext_f, prelude::*, session::IdentityVerification, toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/no_supported_methods_page.ui"
    )]
    #[properties(wrapper_type = super::NoSupportedMethodsPage)]
    pub struct NoSupportedMethodsPage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, explicit_notify, nullable)]
        pub verification: BoundObjectWeakRef<IdentityVerification>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub message: TemplateChild<gtk::Label>,
        #[template_child]
        pub instructions: TemplateChild<gtk::Label>,
        #[template_child]
        pub cancel_btn: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NoSupportedMethodsPage {
        const NAME: &'static str = "IdentityVerificationNoSupportedMethodsPage";
        type Type = super::NoSupportedMethodsPage;
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
    impl ObjectImpl for NoSupportedMethodsPage {
        fn dispose(&self) {
            if let Some(verification) = self.verification.obj()
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for NoSupportedMethodsPage {
        fn grab_focus(&self) -> bool {
            self.cancel_btn.grab_focus()
        }
    }

    impl BinImpl for NoSupportedMethodsPage {}

    impl NoSupportedMethodsPage {
        /// Set the current identity verification.
        fn set_verification(&self, verification: Option<&IdentityVerification>) {
            let prev_verification = self.verification.obj();

            if prev_verification.as_ref() == verification {
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
                        obj.update_page();
                    }
                ));
                self.display_name_handler
                    .replace(Some(display_name_handler));

                let was_accepted_handler = verification.connect_was_accepted_notify(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.update_page();
                    }
                ));

                self.verification
                    .set(verification, vec![was_accepted_handler]);
            }

            obj.update_page();
            obj.notify_verification();
        }
    }
}

glib::wrapper! {
    /// A page to show when a verification request was received with no methods that Fractal supports.
    pub struct NoSupportedMethodsPage(ObjectSubclass<imp::NoSupportedMethodsPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl NoSupportedMethodsPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update the page for the current verification.
    fn update_page(&self) {
        let Some(verification) = self.verification() else {
            return;
        };
        let imp = self.imp();
        let was_accepted = verification.was_accepted();

        let message = if verification.is_self_verification() {
            if was_accepted {
                gettext(
                    "None of the methods offered by the other client are supported by Mandelbrot.",
                )
            } else {
                gettext(
                    "A login request was received, but none of the methods offered by the other client are supported by Mandelbrot.",
                )
            }
        } else {
            let name = verification.user().display_name();
            if was_accepted {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "None of the methods offered by {user}’s client are supported by Mandelbrot.",
                    &[("user", &format!("<b>{name}</b>"))],
                )
            } else {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "{user} sent a verification request, but none of the methods offered by the other client are supported by Mandelbrot.",
                    &[("user", &format!("<b>{name}</b>"))],
                )
            }
        };
        imp.message.set_markup(&message);

        imp.instructions.set_visible(!was_accepted);

        let cancel_label = if was_accepted {
            gettext("Cancel Verification")
        } else {
            gettext("Decline Verification")
        };
        imp.cancel_btn.set_content_label(cancel_label);
    }

    /// Reset the UI to its initial state.
    pub fn reset(&self) {
        self.imp().cancel_btn.set_is_loading(false);
        self.set_sensitive(true);
    }

    /// Decline the verification.
    #[template_callback]
    async fn cancel(&self) {
        let Some(verification) = self.verification() else {
            return;
        };

        self.imp().cancel_btn.set_is_loading(true);
        self.set_sensitive(false);

        if verification.cancel().await.is_err() {
            toast!(self, gettext("Could not decline the verification"));
            self.reset();
        }
    }
}
