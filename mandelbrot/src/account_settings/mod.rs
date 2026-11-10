use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    glib,
    glib::{clone, closure_local},
};
use matrix_sdk::authentication::oauth::error::OAuthDiscoveryError;
use ruma::api::client::discovery::get_authorization_server_metadata::v1::AuthorizationServerMetadata;
use tracing::{error, warn};

mod encryption_page;
mod general_page;
mod notifications_page;
mod safety_page;
mod user_session;

use self::{
    encryption_page::{EncryptionPage, ImportExportKeysSubpage, ImportExportKeysSubpageMode},
    general_page::{ChangePasswordSubpage, DeactivateAccountSubpage, GeneralPage, LogOutSubpage},
    notifications_page::NotificationsPage,
    safety_page::{IgnoredUsersSubpage, SafetyPage},
    user_session::{LinkDeviceSubpage, UserSessionListSubpage, UserSessionSubpage},
};
use crate::{
    components::crypto::{CryptoIdentitySetupView, CryptoRecoverySetupView},
    session::Session,
    spawn, spawn_tokio,
    utils::BoundObjectWeakRef,
};

/// A subpage of the account settings.
#[derive(Debug, Clone, Copy, Eq, PartialEq, glib::Variant)]
pub(crate) enum AccountSettingsSubpage {
    /// A form to change the account's password.
    ChangePassword,
    /// A page to view the list of account's sessions.
    UserSessionList,
    /// A page to link a new device by displaying a QR code.
    LinkNewDevice,
    /// A page to confirm the logout.
    LogOut,
    /// A page to confirm the deactivation of the password.
    DeactivateAccount,
    /// The list of ignored users.
    IgnoredUsers,
    /// A form to import encryption keys.
    ImportKeys,
    /// A form to export encryption keys.
    ExportKeys,
    /// The crypto identity setup view.
    CryptoIdentitySetup,
    /// The recovery setup view.
    RecoverySetup,
}

mod imp {
    use std::{cell::RefCell, sync::LazyLock};

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_settings/mod.ui")]
    #[properties(wrapper_type = super::AccountSettings)]
    pub struct AccountSettings {
        /// The current session.
        #[property(get, set = Self::set_session, nullable)]
        session: BoundObjectWeakRef<Session>,
        /// The OAuth 2.0 authorization server metadata, if any.
        oauth_server_metadata: RefCell<Option<AuthorizationServerMetadata>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AccountSettings {
        const NAME: &'static str = "AccountSettings";
        type Type = super::AccountSettings;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            GeneralPage::ensure_type();
            NotificationsPage::ensure_type();
            SafetyPage::ensure_type();
            EncryptionPage::ensure_type();

            Self::bind_template(klass);

            klass.install_action(
                "account-settings.show-subpage",
                Some(&AccountSettingsSubpage::static_variant_type()),
                |obj, _, param| {
                    let subpage = param
                        .and_then(glib::Variant::get::<AccountSettingsSubpage>)
                        .expect("The parameter should be a valid subpage name");

                    obj.show_subpage(subpage);
                },
            );

            klass.install_action(
                "account-settings.show-session-subpage",
                Some(&String::static_variant_type()),
                |obj, _, param| {
                    obj.show_session_subpage(
                        &param
                            .and_then(glib::Variant::get::<String>)
                            .expect("The parameter should be a string"),
                    );
                },
            );

            klass.install_action_async(
                "account-settings.reload-user-sessions",
                None,
                |obj, _, _| async move {
                    obj.imp().reload_user_sessions().await;
                },
            );

            klass.install_action("account-settings.close", None, |obj, _, _| {
                obj.close();
            });

            klass.install_action("account-settings.close-subpage", None, |obj, _, _| {
                obj.pop_subpage();
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AccountSettings {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("oauth-server-metadata-changed").build()]);
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for AccountSettings {}
    impl AdwDialogImpl for AccountSettings {}
    impl PreferencesDialogImpl for AccountSettings {}

    impl AccountSettings {
        /// Set the current session.
        fn set_session(&self, session: Option<Session>) {
            if self.session.obj() == session {
                return;
            }
            let obj = self.obj();

            self.session.disconnect_signals();
            self.set_oauth_server_metadata(None);

            if let Some(session) = session {
                let logged_out_handler = session.connect_logged_out(clone!(
                    #[weak]
                    obj,
                    move |_| {
                        obj.close();
                    }
                ));
                self.session.set(&session, vec![logged_out_handler]);

                // Refresh the list of sessions.
                spawn!(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.reload_user_sessions().await;
                    }
                ));

                // Load the account management URL.
                spawn!(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    async move {
                        imp.load_oauth_server_metadata().await;
                    }
                ));
            }

            obj.notify_session();
        }

        /// Load the the OAuth 2.0 authorization server metadata.
        async fn load_oauth_server_metadata(&self) {
            let Some(session) = self.session.obj() else {
                return;
            };

            let oauth = session.client().oauth();
            let handle = spawn_tokio!(async move { oauth.cached_server_metadata().await });

            let metadata = match handle.await.expect("task was not aborted") {
                Ok(metadata) => Some(metadata),
                Err(error) => {
                    // Ignore the error that says that OAuth 2.0 is not supported, it can happen.
                    if !matches!(error, OAuthDiscoveryError::NotSupported) {
                        warn!("Could not fetch OAuth 2.0 authorization server metadata: {error}");
                    }
                    None
                }
            };
            self.set_oauth_server_metadata(metadata);
        }

        /// Set the builder for the account management URL of the OAuth 2.0
        /// authorization server.
        fn set_oauth_server_metadata(&self, metadata: Option<AuthorizationServerMetadata>) {
            self.oauth_server_metadata.replace(metadata);
            self.obj()
                .emit_by_name::<()>("oauth-server-metadata-changed", &[]);
        }

        /// The OAuth 2.0 authorization server metadata, if any.
        pub(super) fn oauth_server_metadata(&self) -> Option<AuthorizationServerMetadata> {
            self.oauth_server_metadata.borrow().clone()
        }

        /// Reload the sessions from the server.
        async fn reload_user_sessions(&self) {
            let Some(session) = self.session.obj() else {
                return;
            };

            session.user_sessions().load().await;
        }
    }
}

glib::wrapper! {
    /// Preference window to display and update account settings.
    pub struct AccountSettings(ObjectSubclass<imp::AccountSettings>)
        @extends gtk::Widget, adw::Dialog, adw::PreferencesDialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl AccountSettings {
    /// Construct new `AccountSettings` for the given session.
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// The OAuth 2.0 authorization server metadata, if any.
    fn oauth_server_metadata(&self) -> Option<AuthorizationServerMetadata> {
        self.imp().oauth_server_metadata()
    }

    /// Show the "Encryption" tab.
    pub(crate) fn show_encryption_tab(&self) {
        self.set_visible_page_name("encryption");
    }

    /// Show the given subpage.
    pub(crate) fn show_subpage(&self, subpage: AccountSettingsSubpage) {
        let Some(session) = self.session() else {
            return;
        };

        let page: adw::NavigationPage = match subpage {
            AccountSettingsSubpage::ChangePassword => ChangePasswordSubpage::new(&session).upcast(),
            AccountSettingsSubpage::UserSessionList => {
                UserSessionListSubpage::new(&session).upcast()
            }
            AccountSettingsSubpage::LinkNewDevice => LinkDeviceSubpage::new(&session).upcast(),
            AccountSettingsSubpage::LogOut => LogOutSubpage::new(&session).upcast(),
            AccountSettingsSubpage::DeactivateAccount => {
                DeactivateAccountSubpage::new(&session, self).upcast()
            }
            AccountSettingsSubpage::IgnoredUsers => IgnoredUsersSubpage::new(&session).upcast(),
            AccountSettingsSubpage::ImportKeys => {
                ImportExportKeysSubpage::new(&session, ImportExportKeysSubpageMode::Import).upcast()
            }
            AccountSettingsSubpage::ExportKeys => {
                ImportExportKeysSubpage::new(&session, ImportExportKeysSubpageMode::Export).upcast()
            }
            AccountSettingsSubpage::CryptoIdentitySetup => {
                let view = CryptoIdentitySetupView::new(&session);
                view.connect_completed(clone!(
                    #[weak(rename_to = obj)]
                    self,
                    move |_, _| {
                        obj.pop_subpage();
                    }
                ));

                let page = adw::NavigationPage::builder()
                    .tag("crypto-identity-setup")
                    .child(&view)
                    .build();
                page.connect_shown(clone!(
                    #[weak]
                    view,
                    move |_| {
                        view.grab_focus();
                    }
                ));

                page
            }
            AccountSettingsSubpage::RecoverySetup => {
                let view = CryptoRecoverySetupView::new(&session);
                view.connect_completed(clone!(
                    #[weak(rename_to = obj)]
                    self,
                    move |_| {
                        obj.pop_subpage();
                    }
                ));

                let page = adw::NavigationPage::builder()
                    .tag("crypto-recovery-setup")
                    .child(&view)
                    .build();
                page.connect_shown(clone!(
                    #[weak]
                    view,
                    move |_| {
                        view.grab_focus();
                    }
                ));

                page
            }
        };

        self.push_subpage(&page);
    }

    /// Show a subpage with the session details of the given session ID.
    pub(crate) fn show_session_subpage(&self, device_id: &str) {
        let Some(session) = self.session() else {
            return;
        };

        let user_session = session.user_sessions().get(&device_id.into());

        let Some(user_session) = user_session else {
            error!("ID {device_id} is not associated to any device");
            return;
        };

        let page = UserSessionSubpage::new(&user_session, self);

        self.push_subpage(&page);
    }

    /// Connect to the signal emitted when the OAuth 2.0 authorization server
    /// metadata changed.
    pub fn connect_oauth_server_metadata_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "oauth-server-metadata-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
