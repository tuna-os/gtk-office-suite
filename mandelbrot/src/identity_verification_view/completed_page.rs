use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{gettext_f, prelude::*, session::IdentityVerification};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/identity_verification_view/completed_page.ui")]
    #[properties(wrapper_type = super::CompletedPage)]
    pub struct CompletedPage {
        /// The current identity verification.
        #[property(get, set = Self::set_verification, explicit_notify, nullable)]
        pub verification: glib::WeakRef<IdentityVerification>,
        pub display_name_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub message: TemplateChild<gtk::Label>,
        #[template_child]
        pub dismiss_btn: TemplateChild<gtk::Button>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CompletedPage {
        const NAME: &'static str = "IdentityVerificationCompletedPage";
        type Type = super::CompletedPage;
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
    impl ObjectImpl for CompletedPage {
        fn dispose(&self) {
            if let Some(verification) = self.verification.upgrade()
                && let Some(handler) = self.display_name_handler.take()
            {
                verification.user().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for CompletedPage {
        fn grab_focus(&self) -> bool {
            self.dismiss_btn.grab_focus()
        }
    }

    impl BinImpl for CompletedPage {}

    impl CompletedPage {
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
    /// A page to show when the verification was completed.
    pub struct CompletedPage(ObjectSubclass<imp::CompletedPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl CompletedPage {
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
            imp.title.set_label(&gettext("Request Complete"));
            imp.message.set_label(&gettext(
                "The new session is now ready to send and receive secure messages.",
            ));
        } else {
            let name = verification.user().display_name();
            imp.title.set_markup(&gettext("Verification Complete"));
            // Translators: Do NOT translate the content between '{' and '}', this is a
            // variable name.
            imp.message.set_markup(&gettext_f("{user} is verified and you can now be sure that your communication will be private.", &[("user", &format!("<b>{name}</b>"))]));
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
