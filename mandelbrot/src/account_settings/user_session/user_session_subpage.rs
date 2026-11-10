use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::api::client::discovery::get_authorization_server_metadata::v1::{
    AccountManagementActionData, AuthorizationServerMetadata, DeviceDeleteData,
};
use tracing::error;

use crate::{
    account_settings::AccountSettings,
    components::{ActionButton, ActionState, AuthError, LoadingButtonRow},
    gettext_f,
    prelude::*,
    session::UserSession,
    toast,
    utils::{BoundConstructOnlyObject, BoundObject, TemplateCallbacks},
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/user_session/user_session_subpage.ui"
    )]
    #[properties(wrapper_type = super::UserSessionSubpage)]
    pub struct UserSessionSubpage {
        #[template_child]
        display_name: TemplateChild<adw::EntryRow>,
        #[template_child]
        display_name_button: TemplateChild<ActionButton>,
        #[template_child]
        verified_status: TemplateChild<adw::ActionRow>,
        #[template_child]
        log_out_button: TemplateChild<adw::ButtonRow>,
        #[template_child]
        loading_disconnect_button: TemplateChild<LoadingButtonRow>,
        #[template_child]
        open_url_disconnect_button: TemplateChild<adw::ButtonRow>,
        /// The user session displayed by this subpage.
        #[property(get, set = Self::set_user_session, construct_only)]
        user_session: BoundObject<UserSession>,
        /// The ancestor [`AccountSettings`].
        #[property(get, set = Self::set_account_settings, construct_only)]
        account_settings: BoundConstructOnlyObject<AccountSettings>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserSessionSubpage {
        const NAME: &'static str = "UserSessionSubpage";
        type Type = super::UserSessionSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for UserSessionSubpage {
        fn constructed(&self) {
            self.parent_constructed();

            self.update_disconnect_button();
        }
    }

    impl WidgetImpl for UserSessionSubpage {}
    impl NavigationPageImpl for UserSessionSubpage {}

    #[gtk::template_callbacks]
    impl UserSessionSubpage {
        /// Set the user session displayed by this subpage.
        fn set_user_session(&self, user_session: UserSession) {
            let obj = self.obj();

            let verified_handler = user_session.connect_verified_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_verified();
                }
            ));
            let disconnected_handler = user_session.connect_disconnected(clone!(
                #[weak]
                obj,
                move |_| {
                    let _ = obj.activate_action("account-settings.close-subpage", None);
                }
            ));

            self.user_session
                .set(user_session, vec![verified_handler, disconnected_handler]);

            self.update_verified();

            obj.notify_user_session();
        }

        /// Update the verified status.
        fn update_verified(&self) {
            let Some(user_session) = self.user_session.obj() else {
                return;
            };

            self.verified_status.remove_css_class("success");
            self.verified_status.remove_css_class("error");
            if user_session.verified() {
                // Translators: As in 'A verified session'.
                self.verified_status.set_title(&gettext("Verified"));
                self.verified_status.add_css_class("success");
            } else {
                // Translators: As in 'A verified session'.
                self.verified_status.set_title(&gettext("Not verified"));
                self.verified_status.add_css_class("error");
            }
        }

        /// Set the ancestor [`AccountSettings`].
        fn set_account_settings(&self, account_settings: AccountSettings) {
            let handler = account_settings.connect_oauth_server_metadata_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_disconnect_button();
                }
            ));
            self.account_settings.set(account_settings, vec![handler]);
        }

        /// The OAuth 2.0 authorization server metadata, if any.
        fn oauth_server_metadata(&self) -> Option<AuthorizationServerMetadata> {
            self.account_settings.obj().oauth_server_metadata()
        }

        /// Update the visible disconnect button.
        fn update_disconnect_button(&self) {
            let Some(user_session) = self.user_session.obj() else {
                return;
            };

            if user_session.is_current() {
                self.log_out_button.set_visible(true);
                self.loading_disconnect_button.set_visible(false);
                self.open_url_disconnect_button.set_visible(false);
                return;
            }

            let Some(session) = user_session.session() else {
                return;
            };

            let uses_oauth_api = session.uses_oauth_api();
            let has_account_management_url = self
                .oauth_server_metadata()
                .is_some_and(|metadata| metadata.account_management_uri.is_some());

            self.log_out_button.set_visible(false);
            self.loading_disconnect_button
                .set_visible(!uses_oauth_api && !has_account_management_url);
            self.open_url_disconnect_button
                .set_visible(has_account_management_url);
        }

        /// Update the display name button when the display name is changed by
        /// the user.
        #[template_callback]
        fn display_name_changed(&self) {
            self.display_name_button
                .set_visible(self.has_display_name_changed());
            self.display_name_button.set_state(ActionState::Confirm);
        }

        /// Whether the display name in the entry row is different than the user
        /// session's.
        fn has_display_name_changed(&self) -> bool {
            let Some(user_session) = self.user_session.obj() else {
                return false;
            };

            self.display_name.text().trim() != user_session.display_name().trim()
        }

        /// Update the display name of the user session by making a request to
        /// the homeserver.
        #[template_callback]
        async fn rename_user_session(&self) {
            if !self.has_display_name_changed() {
                // Nothing to do.
                return;
            }
            let obj = self.obj();
            let Some(user_session) = self.user_session.obj() else {
                return;
            };

            let new_display_name = self.display_name.text().trim().to_owned();

            self.display_name.set_editable(false);
            self.display_name.add_css_class("dimmed");
            self.display_name_button.set_state(ActionState::Loading);

            if let Ok(()) = user_session.rename(new_display_name).await {
                self.display_name_button.set_visible(false);
                self.display_name_button.set_state(ActionState::Confirm);
                let confirmation_message = gettext("Session renamed");
                toast!(obj, confirmation_message);
            } else {
                self.display_name_button.set_state(ActionState::Retry);
                let error_message = gettext("Could not rename session");
                toast!(obj, error_message);
            }

            self.display_name.set_editable(true);
            self.display_name.remove_css_class("dimmed");
        }

        /// Disconnect the user session by making a request to the homeserver.
        #[template_callback]
        async fn disconnect_with_request(&self) {
            let obj = self.obj();
            let Some(user_session) = self.user_session.obj() else {
                return;
            };

            self.loading_disconnect_button.set_is_loading(true);

            match user_session.delete(&*obj).await {
                Ok(()) => {
                    let _ = obj.activate_action("account-settings.reload-user-sessions", None);
                }
                Err(AuthError::UserCancelled) => {
                    self.loading_disconnect_button.set_is_loading(false);
                }
                Err(_) => {
                    let device_name = user_session.display_name_or_device_id();
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    let error_message = gettext_f(
                        "Could not disconnect session “{device_name}”",
                        &[("device_name", &device_name)],
                    );
                    toast!(obj, error_message);
                    self.loading_disconnect_button.set_is_loading(false);
                }
            }
        }

        // Open the account management URL to disconnect the session.
        #[template_callback]
        async fn open_disconnect_url(&self) {
            let Some(user_session) = self.user_session.obj() else {
                return;
            };

            let device_id = user_session.device_id();
            let Some(metadata) = self.oauth_server_metadata() else {
                error!("Could not find OAuth 2.0 authorization server metadata");
                return;
            };

            let Some(url) = metadata.account_management_url_with_action(
                AccountManagementActionData::DeviceDelete(DeviceDeleteData::new(device_id)),
            ) else {
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
    /// Account settings subpage about a user session.
    pub struct UserSessionSubpage(ObjectSubclass<imp::UserSessionSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl UserSessionSubpage {
    pub fn new(user_session: &UserSession, account_settings: &AccountSettings) -> Self {
        glib::Object::builder()
            .property("user-session", user_session)
            .property("account-settings", account_settings)
            .build()
    }
}
