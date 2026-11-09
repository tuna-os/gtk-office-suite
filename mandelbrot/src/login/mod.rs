use std::net::{Ipv4Addr, Ipv6Addr};

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};
use matrix_sdk::{
    Client,
    authentication::oauth::{
        ClientRegistrationData,
        registration::{ApplicationType, ClientMetadata, Localized, OAuthGrantType},
    },
    sanitize_server_name,
    utils::local_server::LocalServerRedirectHandle,
};
use ruma::{OwnedServerName, api::client::session::get_login_types::v3::LoginType, serde::Raw};
use tracing::warn;
use url::Url;

mod advanced_dialog;
mod greeter;
mod homeserver_page;
mod in_browser_page;
mod local_server;
mod method_page;
mod qr_code_page;
mod session_setup_view;

use self::{
    advanced_dialog::LoginAdvancedDialog,
    greeter::Greeter,
    homeserver_page::LoginHomeserverPage,
    in_browser_page::{LoginInBrowserData, LoginInBrowserPage},
    local_server::spawn_local_server,
    method_page::LoginMethodPage,
    qr_code_page::LoginQrCodePage,
    session_setup_view::SessionSetupView,
};
use crate::{
    APP_HOMEPAGE_URL, APP_NAME, Application, RUNTIME, SETTINGS_KEY_CURRENT_SESSION, Window,
    components::OfflineBanner, prelude::*, secret::Secret, session::Session, spawn, spawn_tokio,
    toast,
};

/// A page of the login stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginPage {
    /// The greeter page.
    Greeter,
    /// The homeserver page.
    Homeserver,
    /// The page to select a login method.
    Method,
    /// The page to log in with the browser.
    InBrowser,
    /// The page to log in by scanning a QR code.
    QrCode,
    /// The session setup stack.
    SessionSetup,
    /// The login is completed.
    Completed,
}

impl LoginPage {
    /// Get the tag for this page.
    const fn tag(self) -> &'static str {
        match self {
            Self::Greeter => Greeter::TAG,
            Self::Homeserver => LoginHomeserverPage::TAG,
            Self::Method => LoginMethodPage::TAG,
            Self::InBrowser => LoginInBrowserPage::TAG,
            Self::QrCode => LoginQrCodePage::TAG,
            Self::SessionSetup => SessionSetupView::TAG,
            Self::Completed => "completed",
        }
    }

    /// Get the page matching the given tag.
    ///
    /// Panics if the tag does not match any of the variants.
    fn from_tag(tag: &str) -> Self {
        match tag {
            Greeter::TAG => Self::Greeter,
            LoginHomeserverPage::TAG => Self::Homeserver,
            LoginMethodPage::TAG => Self::Method,
            LoginInBrowserPage::TAG => Self::InBrowser,
            LoginQrCodePage::TAG => Self::QrCode,
            SessionSetupView::TAG => Self::SessionSetup,
            "completed" => Self::Completed,
            _ => panic!("Unknown LoginPage: {tag}"),
        }
    }
}

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/login/mod.ui")]
    #[properties(wrapper_type = super::Login)]
    pub struct Login {
        #[template_child]
        navigation: TemplateChild<adw::NavigationView>,
        #[template_child]
        greeter: TemplateChild<Greeter>,
        #[template_child]
        homeserver_page: TemplateChild<LoginHomeserverPage>,
        #[template_child]
        method_page: TemplateChild<LoginMethodPage>,
        #[template_child]
        in_browser_page: TemplateChild<LoginInBrowserPage>,
        #[template_child]
        qr_code_page: TemplateChild<LoginQrCodePage>,
        #[template_child]
        done_button: TemplateChild<gtk::Button>,
        /// Whether auto-discovery is enabled.
        #[property(get, set = Self::set_autodiscovery, construct, explicit_notify, default = true)]
        autodiscovery: Cell<bool>,
        /// The Matrix client used to log in.
        client: RefCell<Option<Client>>,
        /// The session that was just logged in.
        session: RefCell<Option<Session>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Login {
        const NAME: &'static str = "Login";
        type Type = super::Login;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("login");
            klass.set_accessible_role(gtk::AccessibleRole::Group);

            klass.install_action_async("login.sso", None, |obj, _, _| async move {
                obj.imp().init_matrix_sso_login().await;
            });

            klass.install_action_async("login.open-advanced", None, |obj, _, _| async move {
                obj.imp().open_advanced_dialog().await;
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Login {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let monitor = gio::NetworkMonitor::default();
            monitor.connect_network_changed(clone!(
                #[weak]
                obj,
                move |_, available| {
                    obj.action_set_enabled("login.sso", available);
                }
            ));
            obj.action_set_enabled("login.sso", monitor.is_network_available());

            self.navigation.connect_visible_page_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.visible_page_changed();
                }
            ));
        }

        fn dispose(&self) {
            self.drop_client();
            self.drop_session();
        }
    }

    impl WidgetImpl for Login {
        fn grab_focus(&self) -> bool {
            match self.visible_page() {
                LoginPage::Greeter => self.greeter.grab_focus(),
                LoginPage::Homeserver => self.homeserver_page.grab_focus(),
                LoginPage::Method => self.method_page.grab_focus(),
                LoginPage::InBrowser => self.in_browser_page.grab_focus(),
                LoginPage::QrCode => self.qr_code_page.grab_focus(),
                LoginPage::SessionSetup => {
                    if let Some(session_setup) = self.session_setup() {
                        session_setup.grab_focus()
                    } else {
                        false
                    }
                }
                LoginPage::Completed => self.done_button.grab_focus(),
            }
        }
    }

    impl BinImpl for Login {}
    impl AccessibleImpl for Login {}

    #[gtk::template_callbacks]
    impl Login {
        /// The visible page of the view.
        pub(super) fn visible_page(&self) -> LoginPage {
            LoginPage::from_tag(
                &self
                    .navigation
                    .visible_page()
                    .expect("Login navigation view should always have a visible page")
                    .tag()
                    .expect("Login navigation page should always have a tag"),
            )
        }

        /// Set whether auto-discovery is enabled.
        pub fn set_autodiscovery(&self, autodiscovery: bool) {
            if self.autodiscovery.get() == autodiscovery {
                return;
            }

            self.autodiscovery.set(autodiscovery);
            self.obj().notify_autodiscovery();
        }

        /// Get the session setup view, if any.
        pub(super) fn session_setup(&self) -> Option<SessionSetupView> {
            self.navigation
                .find_page(LoginPage::SessionSetup.tag())
                .and_downcast()
        }

        /// The visible page changed.
        fn visible_page_changed(&self) {
            match self.visible_page() {
                LoginPage::Greeter => {
                    self.clean();
                }
                LoginPage::Homeserver => {
                    // Drop the client because it is bound to the homeserver.
                    self.drop_client();
                    // Drop the session because it is bound to the homeserver and account.
                    self.drop_session();
                    self.method_page.clean();
                }
                LoginPage::Method => {
                    // Drop the session because it is bound to the account.
                    self.drop_session();
                }
                LoginPage::QrCode => {
                    // Drop the client because it is bound to the homeserver from the QR code.
                    self.drop_client();
                    // Drop the session because it is bound to the account.
                    self.drop_session();
                }
                _ => {}
            }
        }

        /// The Matrix client.
        pub(super) async fn client(&self) -> Option<Client> {
            if let Some(client) = self.client.borrow().clone() {
                return Some(client);
            }

            // If the client was dropped, try to recreate it.
            let autodiscovery = self.autodiscovery.get();
            let client = self.homeserver_page.build_client(autodiscovery).await.ok();
            self.set_client(client.clone());

            client
        }

        /// Set the Matrix client.
        pub(super) fn set_client(&self, client: Option<Client>) {
            self.client.replace(client);
        }

        /// Drop the Matrix client.
        pub(super) fn drop_client(&self) {
            if let Some(client) = self.client.take() {
                // The `Client` needs to access a tokio runtime when it is dropped.
                let _guard = RUNTIME.enter();
                drop(client);
            }
        }

        /// Drop the session and clean up its data from the system.
        fn drop_session(&self) {
            if let Some(session) = self.session.take() {
                spawn!(async move {
                    let _ = session.log_out().await;
                });
            }
        }

        /// Open the login advanced dialog.
        async fn open_advanced_dialog(&self) {
            let obj = self.obj();
            let dialog = LoginAdvancedDialog::new();
            obj.bind_property("autodiscovery", &dialog, "autodiscovery")
                .sync_create()
                .bidirectional()
                .build();
            dialog.run_future(&*obj).await;
        }

        /// Prepare to log in via the OAuth 2.0 API.
        pub(super) async fn init_oauth_login(&self) {
            let Some(client) = self.client.borrow().clone() else {
                return;
            };

            let Ok((redirect_uri, local_server_handle)) = spawn_local_server().await else {
                return;
            };

            let oauth = client.oauth();
            let handle = spawn_tokio!(async move {
                oauth
                    .login(redirect_uri, None, Some(client_registration_data()), None)
                    .build()
                    .await
            });

            let authorization_data = match handle.await.expect("task was not aborted") {
                Ok(authorization_data) => authorization_data,
                Err(error) => {
                    warn!("Could not construct OAuth 2.0 authorization URL: {error}");
                    toast!(self.obj(), gettext("Could not set up login"));
                    return;
                }
            };

            self.show_in_browser_page(
                local_server_handle,
                LoginInBrowserData::Oauth(authorization_data),
            );
        }

        /// Prepare to log in via the Matrix native API.
        pub(super) async fn init_matrix_login(&self) {
            let Some(client) = self.client.borrow().clone() else {
                return;
            };

            let matrix_auth = client.matrix_auth();
            let handle = spawn_tokio!(async move { matrix_auth.get_login_types().await });

            let login_types = match handle.await.expect("task was not aborted") {
                Ok(response) => response.flows,
                Err(error) => {
                    warn!("Could not get available Matrix login types: {error}");
                    toast!(self.obj(), gettext("Could not set up login"));
                    return;
                }
            };

            let supports_password = login_types
                .iter()
                .any(|login_type| matches!(login_type, LoginType::Password(_)));
            let supports_sso = login_types
                .iter()
                .any(|login_type| matches!(login_type, LoginType::Sso(_)));

            if supports_password {
                let server_name = self
                    .autodiscovery
                    .get()
                    .then(|| self.homeserver_page.homeserver())
                    .and_then(|s| sanitize_server_name(&s).ok());

                self.show_method_page(&client.homeserver(), server_name.as_ref(), supports_sso);
            } else {
                self.init_matrix_sso_login().await;
            }
        }

        /// Prepare to log in via the Matrix SSO API.
        pub(super) async fn init_matrix_sso_login(&self) {
            let Some(client) = self.client.borrow().clone() else {
                return;
            };

            let Ok((redirect_uri, local_server_handle)) = spawn_local_server().await else {
                return;
            };

            let matrix_auth = client.matrix_auth();
            let handle = spawn_tokio!(async move {
                matrix_auth
                    .get_sso_login_url(redirect_uri.as_str(), None)
                    .await
            });

            match handle.await.expect("task was not aborted") {
                Ok(url) => {
                    let url = Url::parse(&url).expect("Matrix SSO URL should be a valid URL");
                    self.show_in_browser_page(local_server_handle, LoginInBrowserData::Matrix(url));
                }
                Err(error) => {
                    warn!("Could not build Matrix SSO URL: {error}");
                    toast!(self.obj(), gettext("Could not set up login"));
                }
            }
        }

        /// Show the page to chose a login method with the given data.
        fn show_method_page(
            &self,
            homeserver: &Url,
            server_name: Option<&OwnedServerName>,
            supports_sso: bool,
        ) {
            self.method_page
                .update(homeserver, server_name, supports_sso);
            self.navigation.push_by_tag(LoginPage::Method.tag());
        }

        /// Show the page to log in with the browser with the given data.
        fn show_in_browser_page(
            &self,
            local_server_handle: LocalServerRedirectHandle,
            data: LoginInBrowserData,
        ) {
            self.in_browser_page.set_up(local_server_handle, data);
            self.navigation.push_by_tag(LoginPage::InBrowser.tag());
        }

        /// Create the session after a successful login.
        pub(super) async fn create_session(&self) {
            let client = self.client().await.expect("client should be constructed");

            match Session::create(&client).await {
                Ok(session) => {
                    self.init_session(session).await;
                }
                Err(error) => {
                    warn!("Could not create session: {error}");
                    toast!(self.obj(), error.to_user_facing());

                    self.navigation.pop();
                }
            }
        }

        /// Initialize the given session.
        async fn init_session(&self, session: Session) {
            let setup_view = SessionSetupView::new(&session);
            setup_view.connect_completed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.navigation.push_by_tag(LoginPage::Completed.tag());
                }
            ));
            self.navigation.push(&setup_view);

            self.drop_client();
            self.session.replace(Some(session.clone()));

            // Save ID of logging in session to GSettings
            let settings = Application::default().settings();
            if let Err(err) =
                settings.set_string(SETTINGS_KEY_CURRENT_SESSION, session.session_id())
            {
                warn!("Could not save current session: {err}");
            }

            let session_info = session.info().clone();

            if Secret::store_session(session_info).await.is_err() {
                toast!(self.obj(), gettext("Could not store session"));
            }

            session.prepare().await;
        }

        /// Finish the login process and show the session.
        #[template_callback]
        fn finish_login(&self) {
            let Some(window) = self.obj().root().and_downcast::<Window>() else {
                return;
            };

            if let Some(session) = self.session.take() {
                window.add_session(session);
            }

            self.clean();
        }

        /// Reset the login stack.
        pub(super) fn clean(&self) {
            // Clean pages.
            self.homeserver_page.clean();
            self.method_page.clean();

            // Clean data.
            self.set_autodiscovery(true);
            self.drop_client();
            self.drop_session();

            // Reinitialize UI.
            self.navigation.pop_to_tag(LoginPage::Greeter.tag());
            self.unfreeze();
        }

        /// Freeze the login screen.
        pub(super) fn freeze(&self) {
            self.navigation.set_sensitive(false);
        }

        /// Unfreeze the login screen.
        pub(super) fn unfreeze(&self) {
            self.navigation.set_sensitive(true);
        }
    }
}

glib::wrapper! {
    /// A widget managing the login flows.
    pub struct Login(ObjectSubclass<imp::Login>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Login {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the Matrix client.
    fn set_client(&self, client: Option<Client>) {
        self.imp().set_client(client);
    }

    /// The Matrix client.
    async fn client(&self) -> Option<Client> {
        self.imp().client().await
    }

    /// Drop the Matrix client.
    fn drop_client(&self) {
        self.imp().drop_client();
    }

    /// Freeze the login screen.
    fn freeze(&self) {
        self.imp().freeze();
    }

    /// Unfreeze the login screen.
    fn unfreeze(&self) {
        self.imp().unfreeze();
    }

    /// Prepare to log in via the OAuth 2.0 API.
    async fn init_oauth_login(&self) {
        self.imp().init_oauth_login().await;
    }

    /// Prepare to log in via the Matrix native API.
    async fn init_matrix_login(&self) {
        self.imp().init_matrix_login().await;
    }

    /// Create the session after a successful login.
    async fn create_session(&self) {
        self.imp().create_session().await;
    }
}

/// Client registration data for the OAuth 2.0 API.
fn client_registration_data() -> ClientRegistrationData {
    // Register the IPv4 and IPv6 localhost APIs as we use a local server for the
    // redirection.
    let ipv4_localhost_uri = Url::parse(&format!("http://{}/", Ipv4Addr::LOCALHOST))
        .expect("IPv4 localhost address should be a valid URL");
    let ipv6_localhost_uri = Url::parse(&format!("http://[{}]/", Ipv6Addr::LOCALHOST))
        .expect("IPv6 localhost address should be a valid URL");

    let client_uri =
        Url::parse(APP_HOMEPAGE_URL).expect("application homepage URL should be a valid URL");

    let mut client_metadata = ClientMetadata::new(
        ApplicationType::Native,
        vec![
            OAuthGrantType::AuthorizationCode {
                redirect_uris: vec![ipv4_localhost_uri, ipv6_localhost_uri],
            },
            // Necessary for signing in with a QR code.
            OAuthGrantType::DeviceCode,
        ],
        Localized::new(client_uri, None),
    );
    client_metadata.client_name = Some(Localized::new(APP_NAME.to_owned(), None));

    Raw::new(&client_metadata)
        .expect("client metadata should serialize to JSON successfully")
        .into()
}
