use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::api::client::discovery::get_authorization_server_metadata::v1::{
    AccountManagementActionData, AuthorizationServerMetadata,
};
use tracing::error;

use super::AccountSettings;
use crate::{
    components::{AuthDialog, LoadingButtonRow},
    prelude::*,
    session::Session,
    toast,
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/general_page/deactivate_account_subpage.ui"
    )]
    #[properties(wrapper_type = super::DeactivateAccountSubpage)]
    pub struct DeactivateAccountSubpage {
        #[template_child]
        confirmation: TemplateChild<adw::EntryRow>,
        #[template_child]
        loading_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        open_url_button: TemplateChild<adw::ButtonRow>,
        /// The current session.
        #[property(get, set = Self::set_session, construct_only)]
        session: glib::WeakRef<Session>,
        /// The ancestor [`AccountSettings`].
        #[property(get, set = Self::set_account_settings, construct_only)]
        account_settings: glib::WeakRef<AccountSettings>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DeactivateAccountSubpage {
        const NAME: &'static str = "DeactivateAccountSubpage";
        type Type = super::DeactivateAccountSubpage;
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
    impl ObjectImpl for DeactivateAccountSubpage {}

    impl WidgetImpl for DeactivateAccountSubpage {}
    impl NavigationPageImpl for DeactivateAccountSubpage {}

    #[gtk::template_callbacks]
    impl DeactivateAccountSubpage {
        /// Set the current session.
        fn set_session(&self, session: &Session) {
            self.session.set(Some(session));
            self.confirmation.set_title(session.user_id().as_str());
        }

        /// Set the ancestor [`AccountSettings`].
        fn set_account_settings(&self, account_settings: &AccountSettings) {
            self.account_settings.set(Some(account_settings));

            account_settings.connect_oauth_server_metadata_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_visible_button();
                }
            ));
            self.update_visible_button();
        }

        /// The OAuth 2.0 authorization server metadata, if any.
        fn oauth_server_metadata(&self) -> Option<AuthorizationServerMetadata> {
            self.account_settings
                .upgrade()
                .and_then(|s| s.oauth_server_metadata())
        }

        /// Update the visible button for the current state.
        fn update_visible_button(&self) {
            let should_open_url = self
                .oauth_server_metadata()
                .is_some_and(|metadata| metadata.account_management_uri.is_some());
            self.loading_button.set_visible(!should_open_url);
            self.open_url_button.set_visible(should_open_url);
        }

        /// Update the state of the buttons.
        #[template_callback]
        fn update_buttons_state(&self) {
            let sensitive = self.can_deactivate_account();
            self.loading_button.set_sensitive(sensitive);
            self.open_url_button.set_sensitive(sensitive);
        }

        /// Whether the account can be deactivated with the current state.
        fn can_deactivate_account(&self) -> bool {
            self.confirmation.text() == self.confirmation.title()
        }

        /// Deactivate the account with the proper method.
        #[template_callback]
        async fn deactivate_account(&self) {
            if self
                .oauth_server_metadata()
                .is_some_and(|metadata| metadata.account_management_uri.is_some())
            {
                self.open_deactivate_account_url().await;
            } else {
                self.deactivate_account_with_request().await;
            }
        }

        /// Deactivate the account of the current session by making a request to
        /// the homeserver.
        #[template_callback]
        async fn deactivate_account_with_request(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            if !self.can_deactivate_account() {
                return;
            }

            self.loading_button.set_is_loading(true);
            self.confirmation.set_sensitive(false);

            let dialog = AuthDialog::new(&session);
            let obj = self.obj();

            let result = dialog
                .authenticate(&*obj, move |client, auth| async move {
                    client.account().deactivate(None, auth, false).await
                })
                .await;

            match result {
                Ok(_) => {
                    if let Some(session) = self.session.upgrade() {
                        if let Some(window) = obj.root().and_downcast_ref::<gtk::Window>() {
                            toast!(window, gettext("Account successfully deactivated"));
                        }
                        session.clean_up().await;
                    }
                    let _ = obj.activate_action("account-settings.close", None);
                }
                Err(error) => {
                    error!("Could not deactivate account: {error:?}");
                    toast!(obj, gettext("Could not deactivate account"));
                }
            }
            self.loading_button.set_is_loading(false);
            self.confirmation.set_sensitive(true);
        }

        // Open the account management URL to deactivate the account.
        #[template_callback]
        async fn open_deactivate_account_url(&self) {
            let Some(metadata) = self.oauth_server_metadata() else {
                error!("Could not find OAuth 2.0 authorization server metadata");
                return;
            };

            let Some(url) = metadata
                .account_management_url_with_action(AccountManagementActionData::AccountDeactivate)
            else {
                error!("Could not build OAuth 2.0 account management URL");
                return;
            };

            if let Err(error) = gtk::UriLauncher::new(url.as_str())
                .launch_future(self.obj().root().and_downcast_ref::<gtk::Window>())
                .await
            {
                error!("Could not launch OAuth 2.0 account management URL: {error}");
            }
        }
    }
}

glib::wrapper! {
    /// Subpage allowing the user to deactivate their account.
    pub struct DeactivateAccountSubpage(ObjectSubclass<imp::DeactivateAccountSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DeactivateAccountSubpage {
    pub fn new(session: &Session, account_settings: &AccountSettings) -> Self {
        glib::Object::builder()
            .property("session", session)
            .property("account-settings", account_settings)
            .build()
    }
}
