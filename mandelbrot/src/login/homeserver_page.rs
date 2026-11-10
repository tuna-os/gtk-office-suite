use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk::{
    Client, ClientBuildError, ClientBuilder, config::RequestConfig, sanitize_server_name,
};
use tracing::warn;
use url::Url;

use super::Login;
use crate::{
    components::{LoadingButton, OfflineBanner},
    gettext_f,
    prelude::*,
    spawn_tokio, toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/login/homeserver_page.ui")]
    #[properties(wrapper_type = super::LoginHomeserverPage)]
    pub struct LoginHomeserverPage {
        #[template_child]
        homeserver_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        homeserver_help: TemplateChild<gtk::Label>,
        #[template_child]
        next_button: TemplateChild<LoadingButton>,
        /// The parent `Login` object.
        #[property(get, set = Self::set_login, explicit_notify, nullable)]
        login: BoundObjectWeakRef<Login>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoginHomeserverPage {
        const NAME: &'static str = "LoginHomeserverPage";
        type Type = super::LoginHomeserverPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LoginHomeserverPage {}

    impl WidgetImpl for LoginHomeserverPage {
        fn grab_focus(&self) -> bool {
            self.homeserver_entry.grab_focus()
        }
    }

    impl NavigationPageImpl for LoginHomeserverPage {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl LoginHomeserverPage {
        /// Set the parent `Login` object.
        fn set_login(&self, login: Option<&Login>) {
            self.login.disconnect_signals();

            if let Some(login) = login {
                let handler = login.connect_autodiscovery_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_next_state();
                        imp.update_text();
                    }
                ));

                self.login.set(login, vec![handler]);
            }

            self.update_next_state();
            self.update_text();
        }

        /// Update the text of this page according to the current settings.
        fn update_text(&self) {
            let Some(login) = self.login.obj() else {
                return;
            };

            if login.autodiscovery() {
                self.homeserver_entry.set_title(&gettext("Domain Name"));
                self.homeserver_help.set_markup(&gettext(
                    "The domain of your Matrix homeserver, for example gnome.org",
                ));
            } else {
                self.homeserver_entry.set_title(&gettext("Homeserver URL"));
                self.homeserver_help.set_markup(&gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "The URL of your Matrix homeserver, for example {address}",
                    &[(
                        "address",
                        "<span segment=\"word\">https://gnome.modular.im</span>",
                    )],
                ));
            }
        }

        /// Reset this page.
        pub(super) fn clean(&self) {
            self.homeserver_entry.set_text("");
            self.next_button.set_is_loading(false);
            self.update_next_state();
        }

        /// The current text from the homeserver entry.
        pub(super) fn homeserver(&self) -> glib::GString {
            self.homeserver_entry.text()
        }

        /// Whether the current state allows to go to the next step.
        fn can_go_next(&self) -> bool {
            let Some(login) = self.login.obj() else {
                return false;
            };
            let homeserver = self.homeserver();

            if login.autodiscovery() {
                sanitize_server_name(homeserver.as_str()).is_ok()
            } else {
                Url::parse(homeserver.as_str()).is_ok()
            }
        }

        /// Update the state of the "Next" button.
        #[template_callback]
        fn update_next_state(&self) {
            self.next_button.set_sensitive(self.can_go_next());
        }

        /// Check if the homeserver that was entered is valid.
        #[template_callback]
        async fn check_homeserver(&self) {
            if !self.can_go_next() {
                return;
            }

            let Some(login) = self.login.obj() else {
                return;
            };

            self.next_button.set_is_loading(true);
            login.freeze();

            let autodiscovery = login.autodiscovery();
            let res = self.build_client(autodiscovery).await;

            match res {
                Ok(client) => {
                    login.set_client(Some(client.clone()));
                    self.discover_login_api(client).await;
                }
                Err(error) => {
                    self.abort_on_error(&error.to_user_facing());
                }
            }

            self.next_button.set_is_loading(false);
            login.unfreeze();
        }

        /// Try to build a client with the current homeserver.
        pub(super) async fn build_client(
            &self,
            autodiscovery: bool,
        ) -> Result<Client, ClientBuildError> {
            if autodiscovery {
                self.build_client_with_autodiscovery().await
            } else {
                self.build_client_with_url().await
            }
        }

        /// Try to build a client by using homeserver autodiscovery.
        async fn build_client_with_autodiscovery(&self) -> Result<Client, ClientBuildError> {
            let homeserver = self.homeserver();
            let handle = spawn_tokio!(async move {
                Self::client_builder()
                    .server_name_or_homeserver_url(homeserver)
                    .build()
                    .await
            });

            match handle.await.expect("task was not aborted") {
                Ok(client) => Ok(client),
                Err(error) => {
                    warn!("Could not discover homeserver: {error}");
                    Err(error)
                }
            }
        }

        /// Try to build a client by using the homeserver's URL.
        async fn build_client_with_url(&self) -> Result<Client, ClientBuildError> {
            let homeserver = self.homeserver();
            spawn_tokio!(async move {
                let client = Self::client_builder()
                    .respect_login_well_known(false)
                    .homeserver_url(homeserver)
                    .build()
                    .await?;

                // Call the `GET /versions` endpoint to make sure that the URL belongs to a
                // Matrix homeserver.
                client.server_versions().await?;

                Ok(client)
            })
            .await
            .expect("task was not aborted")
        }

        /// Discover the login API supported by the homeserver.
        async fn discover_login_api(&self, client: Client) {
            let Some(login) = self.login.obj() else {
                return;
            };

            // Check if the server supports the OAuth 2.0 API.
            let oauth = client.oauth();
            let handle = spawn_tokio!(async move { oauth.server_metadata().await });

            match handle.await.expect("task was not aborted") {
                Ok(_) => {
                    login.init_oauth_login().await;
                }
                Err(error) => {
                    if error.is_not_supported() {
                        // Fallback to the Matrix native API.
                        login.init_matrix_login().await;
                    } else {
                        warn!("Could not get authorization server metadata: {error}");
                        self.abort_on_error(&gettext("Could not set up login"));
                    }
                }
            }
        }

        /// Construct a [`ClientBuilder`] with the proper configuration.
        fn client_builder() -> ClientBuilder {
            Client::builder().request_config(RequestConfig::new().retry_limit(2))
        }

        /// Show the given error and abort the current login.
        fn abort_on_error(&self, error: &str) {
            toast!(self.obj(), error);

            // Drop the client because it is bound to the homeserver.
            if let Some(login) = self.login.obj() {
                login.drop_client();
            }
        }
    }
}

glib::wrapper! {
    /// The login page to provide the homeserver and login settings.
    pub struct LoginHomeserverPage(ObjectSubclass<imp::LoginHomeserverPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoginHomeserverPage {
    /// The tag for this page.
    pub(super) const TAG: &str = "homeserver";

    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The current text from the homeserver entry.
    pub(super) fn homeserver(&self) -> glib::GString {
        self.imp().homeserver()
    }

    /// Reset this page.
    pub(super) fn clean(&self) {
        self.imp().clean();
    }

    /// Try to build a client with the current homeserver.
    pub(super) async fn build_client(
        &self,
        autodiscovery: bool,
    ) -> Result<Client, ClientBuildError> {
        self.imp().build_client(autodiscovery).await
    }
}
