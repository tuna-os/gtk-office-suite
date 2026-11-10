use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use ruma::OwnedServerName;
use tracing::warn;
use url::Url;

use super::Login;
use crate::{components::LoadingButton, gettext_f, prelude::*, spawn_tokio, toast};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/login/method_page.ui")]
    #[properties(wrapper_type = super::LoginMethodPage)]
    pub struct LoginMethodPage {
        #[template_child]
        title: TemplateChild<gtk::Label>,
        #[template_child]
        homeserver_url: TemplateChild<gtk::Label>,
        #[template_child]
        username_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        password_entry: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        sso_button: TemplateChild<gtk::Button>,
        #[template_child]
        next_button: TemplateChild<LoadingButton>,
        /// The parent `Login` object.
        #[property(get, set, nullable)]
        login: glib::WeakRef<Login>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoginMethodPage {
        const NAME: &'static str = "LoginMethodPage";
        type Type = super::LoginMethodPage;
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
    impl ObjectImpl for LoginMethodPage {}

    impl WidgetImpl for LoginMethodPage {
        fn grab_focus(&self) -> bool {
            self.username_entry.grab_focus()
        }
    }

    impl NavigationPageImpl for LoginMethodPage {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl LoginMethodPage {
        /// The username entered by the user.
        fn username(&self) -> glib::GString {
            self.username_entry.text()
        }

        /// The password entered by the user.
        fn password(&self) -> glib::GString {
            self.password_entry.text()
        }

        /// Update the domain name and URL displayed in the title.
        pub(super) fn update_title(
            &self,
            homeserver_url: &Url,
            server_name: Option<&OwnedServerName>,
        ) {
            let title = if let Some(server_name) = server_name {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Log in to {domain_name}",
                    &[(
                        "domain_name",
                        &format!("<span segment=\"word\">{server_name}</span>"),
                    )],
                )
            } else {
                gettext("Log in")
            };
            self.title.set_markup(&title);

            let homeserver_url = homeserver_url.as_str().trim_end_matches('/');
            self.homeserver_url.set_label(homeserver_url);
        }

        /// Update the SSO group.
        pub(super) fn update_sso(&self, supports_sso: bool) {
            self.sso_button.set_visible(supports_sso);
        }

        /// Whether the current state allows to login with a password.
        fn can_login_with_password(&self) -> bool {
            let username_length = self.username().len();
            let password_length = self.password().len();
            username_length != 0 && password_length != 0
        }

        /// Update the state of the "Next" button.
        #[template_callback]
        pub(super) fn update_next_state(&self) {
            self.next_button
                .set_sensitive(self.can_login_with_password());
        }

        /// Login with the password login type.
        #[template_callback]
        async fn login_with_password(&self) {
            if !self.can_login_with_password() {
                return;
            }

            let Some(login) = self.login.upgrade() else {
                return;
            };

            self.next_button.set_is_loading(true);
            login.freeze();

            let username = self.username();
            let password = self.password();

            let client = login.client().await.unwrap();
            let handle = spawn_tokio!(async move {
                client
                    .matrix_auth()
                    .login_username(&username, &password)
                    .initial_device_display_name("Mandelbrot")
                    .send()
                    .await
            });

            match handle.await.expect("task was not aborted") {
                Ok(_) => {
                    login.create_session().await;
                }
                Err(error) => {
                    warn!("Could not log in: {error}");
                    toast!(self.obj(), error.to_user_facing());
                }
            }

            self.next_button.set_is_loading(false);
            login.unfreeze();
        }

        /// Reset this page.
        pub(super) fn clean(&self) {
            self.username_entry.set_text("");
            self.password_entry.set_text("");
            self.next_button.set_is_loading(false);
            self.update_next_state();
        }
    }
}

glib::wrapper! {
    /// The login page allowing to login via password or to choose a SSO provider.
    pub struct LoginMethodPage(ObjectSubclass<imp::LoginMethodPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoginMethodPage {
    /// The tag for this page.
    pub(super) const TAG: &str = "method";

    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update this page with the given data.
    pub(crate) fn update(
        &self,
        homeserver_url: &Url,
        domain_name: Option<&OwnedServerName>,
        supports_sso: bool,
    ) {
        let imp = self.imp();
        imp.update_title(homeserver_url, domain_name);
        imp.update_sso(supports_sso);
        imp.update_next_state();
    }

    /// Reset this page.
    pub(crate) fn clean(&self) {
        self.imp().clean();
    }
}
