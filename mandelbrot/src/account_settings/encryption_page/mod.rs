use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

mod import_export_keys_subpage;

pub(super) use self::import_export_keys_subpage::{
    ImportExportKeysSubpage, ImportExportKeysSubpageMode,
};
use crate::session::{CryptoIdentityState, RecoveryState, Session, SessionVerificationState};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_settings/encryption_page/mod.ui")]
    #[properties(wrapper_type = super::EncryptionPage)]
    pub struct EncryptionPage {
        #[template_child]
        crypto_identity_row: TemplateChild<adw::PreferencesRow>,
        #[template_child]
        crypto_identity_icon: TemplateChild<gtk::Image>,
        #[template_child]
        crypto_identity_description: TemplateChild<gtk::Label>,
        #[template_child]
        crypto_identity_btn: TemplateChild<gtk::Button>,
        #[template_child]
        recovery_row: TemplateChild<adw::PreferencesRow>,
        #[template_child]
        recovery_icon: TemplateChild<gtk::Image>,
        #[template_child]
        recovery_description: TemplateChild<gtk::Label>,
        #[template_child]
        recovery_btn: TemplateChild<gtk::Button>,
        /// The current session.
        #[property(get, set = Self::set_session, nullable)]
        session: glib::WeakRef<Session>,
        security_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EncryptionPage {
        const NAME: &'static str = "EncryptionPage";
        type Type = super::EncryptionPage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for EncryptionPage {
        fn dispose(&self) {
            if let Some(session) = self.session.upgrade() {
                let security = session.security();
                for handler in self.security_handlers.take() {
                    security.disconnect(handler);
                }
            }
        }
    }

    impl WidgetImpl for EncryptionPage {}
    impl PreferencesPageImpl for EncryptionPage {}

    impl EncryptionPage {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            let prev_session = self.session.upgrade();

            if prev_session.as_ref() == session {
                return;
            }

            if let Some(session) = prev_session {
                let security = session.security();
                for handler in self.security_handlers.take() {
                    security.disconnect(handler);
                }
            }

            if let Some(session) = session {
                let security = session.security();
                let crypto_identity_state_handler =
                    security.connect_crypto_identity_state_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_crypto_identity();
                        }
                    ));
                let verification_state_handler =
                    security.connect_verification_state_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_crypto_identity();
                        }
                    ));
                let recovery_state_handler = security.connect_recovery_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_recovery();
                    }
                ));

                self.security_handlers.replace(vec![
                    crypto_identity_state_handler,
                    verification_state_handler,
                    recovery_state_handler,
                ]);
            }

            self.session.set(session);

            self.update_crypto_identity();
            self.update_recovery();

            self.obj().notify_session();
        }

        /// Update the crypto identity section.
        fn update_crypto_identity(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let security = session.security();

            let crypto_identity_state = security.crypto_identity_state();
            if matches!(
                crypto_identity_state,
                CryptoIdentityState::Unknown | CryptoIdentityState::Missing
            ) {
                self.crypto_identity_icon
                    .set_icon_name(Some("verified-danger-symbolic"));
                self.crypto_identity_icon.remove_css_class("success");
                self.crypto_identity_icon.remove_css_class("warning");
                self.crypto_identity_icon.add_css_class("error");

                self.crypto_identity_row
                    .set_title(&gettext("No Crypto Identity"));
                self.crypto_identity_description.set_label(&gettext(
                    "Verifying your own devices or other users is not possible",
                ));

                self.crypto_identity_btn.set_label(&gettext("Enable…"));
                self.crypto_identity_btn
                    .update_property(&[gtk::accessible::Property::Label(&gettext(
                        "Enable Crypto Identity",
                    ))]);
                self.crypto_identity_btn.add_css_class("suggested-action");

                return;
            }

            let verification_state = security.verification_state();
            if verification_state == SessionVerificationState::Verified {
                self.crypto_identity_icon
                    .set_icon_name(Some("verified-symbolic"));
                self.crypto_identity_icon.add_css_class("success");
                self.crypto_identity_icon.remove_css_class("warning");
                self.crypto_identity_icon.remove_css_class("error");

                self.crypto_identity_row
                    .set_title(&gettext("Crypto Identity Enabled"));
                self.crypto_identity_description.set_label(&gettext(
                    "The crypto identity exists and this device is verified",
                ));

                self.crypto_identity_btn.set_label(&gettext("Reset…"));
                self.crypto_identity_btn
                    .update_property(&[gtk::accessible::Property::Label(&gettext(
                        "Reset Crypto Identity",
                    ))]);
                self.crypto_identity_btn
                    .remove_css_class("suggested-action");
            } else {
                self.crypto_identity_icon
                    .set_icon_name(Some("verified-warning-symbolic"));
                self.crypto_identity_icon.remove_css_class("success");
                self.crypto_identity_icon.add_css_class("warning");
                self.crypto_identity_icon.remove_css_class("error");

                self.crypto_identity_row
                    .set_title(&gettext("Crypto Identity Incomplete"));
                self.crypto_identity_description.set_label(&gettext(
                    "The crypto identity exists but this device is not verified",
                ));

                self.crypto_identity_btn.set_label(&gettext("Verify…"));
                self.crypto_identity_btn
                    .update_property(&[gtk::accessible::Property::Label(&gettext(
                        "Verify This Session",
                    ))]);
                self.crypto_identity_btn.add_css_class("suggested-action");
            }
        }

        /// Update the recovery section.
        fn update_recovery(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let recovery_state = session.security().recovery_state();
            match recovery_state {
                RecoveryState::Unknown | RecoveryState::Disabled => {
                    self.recovery_icon.set_icon_name(Some("sync-off-symbolic"));
                    self.recovery_icon.remove_css_class("success");
                    self.recovery_icon.remove_css_class("warning");
                    self.recovery_icon.add_css_class("error");

                    self.recovery_row
                        .set_title(&gettext("Account Recovery Disabled"));
                    self.recovery_description.set_label(&gettext(
                        "Enable recovery to be able to restore your account without another device",
                    ));

                    self.recovery_btn.set_label(&gettext("Enable…"));
                    self.recovery_btn
                        .update_property(&[gtk::accessible::Property::Label(&gettext(
                            "Enable Account Recovery",
                        ))]);
                    self.recovery_btn.add_css_class("suggested-action");
                }
                RecoveryState::Enabled => {
                    self.recovery_icon.set_icon_name(Some("sync-on-symbolic"));
                    self.recovery_icon.add_css_class("success");
                    self.recovery_icon.remove_css_class("warning");
                    self.recovery_icon.remove_css_class("error");

                    self.recovery_row
                        .set_title(&gettext("Account Recovery Enabled"));
                    self.recovery_description.set_label(&gettext(
                        "Your signing keys and encryption keys are synchronized",
                    ));

                    self.recovery_btn.set_label(&gettext("Reset…"));
                    self.recovery_btn
                        .update_property(&[gtk::accessible::Property::Label(&gettext(
                            "Reset Account Recovery Key",
                        ))]);
                    self.recovery_btn.remove_css_class("suggested-action");
                }
                RecoveryState::Incomplete => {
                    self.recovery_icon
                        .set_icon_name(Some("sync-partial-symbolic"));
                    self.recovery_icon.remove_css_class("success");
                    self.recovery_icon.add_css_class("warning");
                    self.recovery_icon.remove_css_class("error");

                    self.recovery_row
                        .set_title(&gettext("Account Recovery Incomplete"));
                    self.recovery_description.set_label(&gettext(
                        "Recover to synchronize your signing keys and encryption keys",
                    ));

                    self.recovery_btn.set_label(&gettext("Recover…"));
                    self.recovery_btn
                        .update_property(&[gtk::accessible::Property::Label(&gettext(
                            "Recover Account Data",
                        ))]);
                    self.recovery_btn.add_css_class("suggested-action");
                }
            }
        }
    }
}

glib::wrapper! {
    /// Encryption settings page.
    pub struct EncryptionPage(ObjectSubclass<imp::EncryptionPage>)
        @extends gtk::Widget, adw::PreferencesPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl EncryptionPage {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
