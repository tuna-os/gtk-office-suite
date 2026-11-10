use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;

use crate::{
    account_settings::AccountSettings,
    components::LoadingButtonRow,
    session::{CryptoIdentityState, RecoveryState, Session, SessionVerificationState},
    toast,
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/general_page/log_out_subpage.ui"
    )]
    #[properties(wrapper_type = super::LogOutSubpage)]
    pub struct LogOutSubpage {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        warning_box: TemplateChild<gtk::Box>,
        #[template_child]
        warning_description: TemplateChild<gtk::Label>,
        #[template_child]
        warning_button: TemplateChild<adw::ButtonRow>,
        #[template_child]
        logout_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        try_again_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        remove_button: TemplateChild<LoadingButtonRow>,
        /// The current session.
        #[property(get, set = Self::set_session, nullable)]
        session: glib::WeakRef<Session>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LogOutSubpage {
        const NAME: &'static str = "LogOutSubpage";
        type Type = super::LogOutSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LogOutSubpage {}

    impl WidgetImpl for LogOutSubpage {}
    impl NavigationPageImpl for LogOutSubpage {}

    #[gtk::template_callbacks]
    impl LogOutSubpage {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            self.session.set(session);
            self.update_warning();
        }

        /// Update the warning message.
        fn update_warning(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let security = session.security();
            let verification_state = security.verification_state();
            let recovery_state = security.recovery_state();

            if verification_state != SessionVerificationState::Verified
                || recovery_state != RecoveryState::Enabled
            {
                self.warning_description.set_label(&gettext("The crypto identity and account recovery are not set up properly. If this is your last connected session and you have no recent local backup of your encryption keys, you will not be able to restore your account."));
                self.warning_box.set_visible(true);
                return;
            }

            let crypto_identity_state = security.crypto_identity_state();

            if crypto_identity_state == CryptoIdentityState::LastManStanding {
                self.warning_description.set_label(&gettext("This is your last connected session. Make sure that you can still access your recovery key or passphrase, or to backup your encryption keys before logging out."));
                self.warning_box.set_visible(true);
                return;
            }

            // No particular problem, do not show the warning.
            self.warning_box.set_visible(false);
        }

        /// Show the security tab of the settings.
        #[template_callback]
        fn view_security(&self) {
            let Some(dialog) = self
                .obj()
                .ancestor(AccountSettings::static_type())
                .and_downcast::<AccountSettings>()
            else {
                return;
            };

            dialog.pop_subpage();
            dialog.show_encryption_tab();
        }

        /// Log out the current session.
        #[template_callback]
        async fn log_out(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let is_logout_page = self
                .stack
                .visible_child_name()
                .is_some_and(|name| name == "logout");

            if is_logout_page {
                self.logout_button.set_is_loading(true);
                self.warning_button.set_sensitive(false);
            } else {
                self.try_again_button.set_is_loading(true);
            }

            if let Err(error) = session.log_out().await {
                if is_logout_page {
                    self.stack.set_visible_child_name("failed");
                } else {
                    toast!(self.obj(), error);
                }
            }

            if is_logout_page {
                self.logout_button.set_is_loading(false);
                self.warning_button.set_sensitive(true);
            } else {
                self.try_again_button.set_is_loading(false);
            }
        }

        /// Remove the current session.
        #[template_callback]
        async fn remove(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            self.remove_button.set_is_loading(true);

            session.clean_up().await;

            self.remove_button.set_is_loading(false);
        }
    }
}

glib::wrapper! {
    /// Subpage allowing a user to log out from their account.
    pub struct LogOutSubpage(ObjectSubclass<imp::LogOutSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LogOutSubpage {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
