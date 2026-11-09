use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use matrix_sdk::{
    Error,
    authentication::oauth::OAuthAuthorizationData,
    utils::{
        UrlOrQuery,
        local_server::{LocalServerRedirectHandle, QueryString},
    },
};
use tokio::task::AbortHandle;
use tracing::{error, warn};
use url::Url;

use super::Login;
use crate::{APP_NAME, prelude::*, spawn_tokio, toast};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/login/in_browser_page.ui")]
    #[properties(wrapper_type = super::LoginInBrowserPage)]
    pub struct LoginInBrowserPage {
        #[template_child]
        continue_btn: TemplateChild<gtk::Button>,
        /// The ancestor `Login` object.
        #[property(get, set, nullable)]
        login: glib::WeakRef<Login>,
        /// A handle to the local server to wait for the redirect.
        local_server_handle: RefCell<Option<LocalServerRedirectHandle>>,
        /// The login data to use.
        data: RefCell<Option<LoginInBrowserData>>,
        /// The abort handle for the ongoing task.
        abort_handle: RefCell<Option<AbortHandle>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoginInBrowserPage {
        const NAME: &'static str = "LoginInBrowserPage";
        type Type = super::LoginInBrowserPage;
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
    impl ObjectImpl for LoginInBrowserPage {}

    impl WidgetImpl for LoginInBrowserPage {
        fn grab_focus(&self) -> bool {
            self.continue_btn.grab_focus()
        }
    }

    impl NavigationPageImpl for LoginInBrowserPage {
        fn shown(&self) {
            self.grab_focus();
        }

        fn hidden(&self) {
            self.clean();
        }
    }

    #[gtk::template_callbacks]
    impl LoginInBrowserPage {
        /// Set up this page with the given local server and data.
        pub(super) fn set_up(
            &self,
            local_server_handle: LocalServerRedirectHandle,
            data: LoginInBrowserData,
        ) {
            self.clean();
            self.local_server_handle.replace(Some(local_server_handle));
            self.data.replace(Some(data));
        }

        /// Open the URL for the current state.
        #[template_callback]
        async fn launch_url(&self) {
            let Some(data) = self.data.borrow().clone() else {
                return;
            };

            if let Err(error) = gtk::UriLauncher::new(data.url().as_str())
                .launch_future(self.obj().root().and_downcast_ref::<gtk::Window>())
                .await
            {
                error!("Could not launch URI: {error}");
                toast!(self.obj(), gettext("Could not open URL"));
                return;
            }

            let Some(local_server_handle) = self.local_server_handle.take() else {
                // If we don't have the server handle, we are already waiting for the redirect.
                return;
            };

            let handle = spawn_tokio!(async move { local_server_handle.await });

            self.abort_handle.replace(Some(handle.abort_handle()));

            let Ok(result) = handle.await else {
                // The task was aborted.
                self.abort_handle.take();
                return;
            };

            self.abort_handle.take();

            if let Some(window) = self.obj().root().and_downcast::<gtk::Window>() {
                window.present();
            }

            let Some(query_string) = result else {
                warn!("Could not log in: missing query string in redirect URI");
                self.abort_on_error(&gettext("An unexpected error occurred."));
                return;
            };

            match data {
                LoginInBrowserData::Oauth(_) => self.finish_oauth_login(query_string).await,
                LoginInBrowserData::Matrix(_) => {
                    self.finish_matrix_login(query_string).await;
                }
            }
        }

        /// Finish the OAuth 2.0 login process.
        async fn finish_oauth_login(&self, query_string: QueryString) {
            let Some(login) = self.login.upgrade() else {
                return;
            };

            let client = login
                .client()
                .await
                .expect("login client should be constructed");
            let oauth = client.oauth();
            let handle =
                spawn_tokio!(
                    async move { oauth.finish_login(UrlOrQuery::Query(query_string.0)).await }
                );

            self.abort_handle.replace(Some(handle.abort_handle()));

            let Ok(result) = handle.await else {
                // The task was aborted.
                self.abort_handle.take();
                return;
            };

            self.abort_handle.take();

            match result {
                Ok(()) => {
                    login.create_session().await;
                }
                Err(error) => {
                    warn!("Could not log in via OAuth 2.0: {error}");
                    self.abort_on_error(&error.to_user_facing());
                }
            }
        }

        /// Finish the Matrix SSO login process.
        async fn finish_matrix_login(&self, query_string: QueryString) {
            let Some(login) = self.login.upgrade() else {
                return;
            };

            let client = login
                .client()
                .await
                .expect("login client should be constructed");
            let matrix_auth = client.matrix_auth();

            let handle = spawn_tokio!(async move {
                matrix_auth
                    .login_with_sso_callback(query_string.into())
                    .map_err(|error| Error::UnknownError(error.into()))?
                    .initial_device_display_name(APP_NAME)
                    .await
            });

            self.abort_handle.replace(Some(handle.abort_handle()));

            let Ok(result) = handle.await else {
                // The task was aborted.
                self.abort_handle.take();
                return;
            };

            self.abort_handle.take();

            match result {
                Ok(_) => {
                    login.create_session().await;
                }
                Err(error) => {
                    warn!("Could not log in via SSO: {error}");
                    self.abort_on_error(&error.to_user_facing());
                }
            }
        }

        /// Show the given error and abort the current login.
        fn abort_on_error(&self, error: &str) {
            let obj = self.obj();
            toast!(obj, error);

            // We need to restart the server if the user wants to try again, so let's go
            // back to the previous screen.
            let _ = obj.activate_action("navigation.pop", None);
        }

        /// Reset this page.
        fn clean(&self) {
            if let Some(handle) = self.abort_handle.take() {
                handle.abort();
            }

            self.data.take();
            self.local_server_handle.take();
        }
    }
}

glib::wrapper! {
    /// A page to log the user in via the browser.
    pub struct LoginInBrowserPage(ObjectSubclass<imp::LoginInBrowserPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoginInBrowserPage {
    /// The tag for this page.
    pub(super) const TAG: &str = "in-browser";

    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set up this page with the given local server and data.
    pub(super) fn set_up(
        &self,
        local_server_handle: LocalServerRedirectHandle,
        data: LoginInBrowserData,
    ) {
        self.imp().set_up(local_server_handle, data);
    }
}

/// Data for logging in via the browser.
#[derive(Debug, Clone)]
pub(super) enum LoginInBrowserData {
    /// Log in via the OAuth 2.0 API with the given authorization data.
    Oauth(OAuthAuthorizationData),

    /// Log in via the Matrix native SSO API with the given URL.
    Matrix(Url),
}

impl LoginInBrowserData {
    /// Get the URL to open in the browser.
    fn url(&self) -> &Url {
        match self {
            Self::Oauth(authorization_data) => &authorization_data.url,
            Self::Matrix(url) => url,
        }
    }
}
