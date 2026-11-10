use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use ruma::api::error::ErrorKind;
use tracing::error;

use crate::{
    components::{AuthDialog, AuthError, LoadingButtonRow},
    session::Session,
    toast,
    utils::matrix::validate_password,
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/general_page/change_password_subpage.ui"
    )]
    #[properties(wrapper_type = super::ChangePasswordSubpage)]
    pub struct ChangePasswordSubpage {
        #[template_child]
        password: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        password_progress: TemplateChild<gtk::LevelBar>,
        #[template_child]
        password_error_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        password_error: TemplateChild<gtk::Label>,
        #[template_child]
        confirm_password: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        confirm_password_error_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        confirm_password_error: TemplateChild<gtk::Label>,
        #[template_child]
        button: TemplateChild<LoadingButtonRow>,
        /// The current session.
        #[property(get, set, nullable)]
        session: glib::WeakRef<Session>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ChangePasswordSubpage {
        const NAME: &'static str = "ChangePasswordSubpage";
        type Type = super::ChangePasswordSubpage;
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
    impl ObjectImpl for ChangePasswordSubpage {}

    impl WidgetImpl for ChangePasswordSubpage {}
    impl NavigationPageImpl for ChangePasswordSubpage {}

    #[gtk::template_callbacks]
    impl ChangePasswordSubpage {
        #[template_callback]
        fn validate_password(&self) {
            let entry = &self.password;
            let progress = &self.password_progress;
            let revealer = &self.password_error_revealer;
            let label = &self.password_error;
            let password = entry.text();

            if password.is_empty() {
                revealer.set_reveal_child(false);
                entry.remove_css_class("success");
                entry.remove_css_class("warning");
                progress.set_value(0.0);
                progress.remove_css_class("success");
                progress.remove_css_class("warning");
                self.update_button();
                return;
            }

            let validity = validate_password(&password);

            progress.set_value(f64::from(validity.progress) / 20.0);
            if validity.progress == 100 {
                revealer.set_reveal_child(false);
                entry.add_css_class("success");
                entry.remove_css_class("warning");
                progress.add_css_class("success");
                progress.remove_css_class("warning");
            } else {
                entry.remove_css_class("success");
                entry.add_css_class("warning");
                progress.remove_css_class("success");
                progress.add_css_class("warning");
                if !validity.has_length {
                    label.set_label(&gettext("Password must be at least 8 characters long"));
                } else if !validity.has_lowercase {
                    label.set_label(&gettext(
                        "Password must have at least one lower-case letter",
                    ));
                } else if !validity.has_uppercase {
                    label.set_label(&gettext(
                        "Password must have at least one upper-case letter",
                    ));
                } else if !validity.has_number {
                    label.set_label(&gettext("Password must have at least one digit"));
                } else if !validity.has_symbol {
                    label.set_label(&gettext("Password must have at least one symbol"));
                }
                revealer.set_reveal_child(true);
            }

            self.validate_password_confirmation();
        }

        #[template_callback]
        fn validate_password_confirmation(&self) {
            let entry = &self.confirm_password;
            let revealer = &self.confirm_password_error_revealer;
            let label = &self.confirm_password_error;
            let password = self.password.text();
            let confirmation = entry.text();

            if confirmation.is_empty() {
                revealer.set_reveal_child(false);
                entry.remove_css_class("success");
                entry.remove_css_class("warning");
                return;
            }

            if password == confirmation {
                revealer.set_reveal_child(false);
                entry.add_css_class("success");
                entry.remove_css_class("warning");
            } else {
                entry.remove_css_class("success");
                entry.add_css_class("warning");
                label.set_label(&gettext("Passwords do not match"));
                revealer.set_reveal_child(true);
            }

            self.update_button();
        }

        fn update_button(&self) {
            self.button.set_sensitive(self.can_change_password());
        }

        fn can_change_password(&self) -> bool {
            let password = self.password.text();
            let confirmation = self.confirm_password.text();

            validate_password(&password).progress == 100 && password == confirmation
        }

        #[template_callback]
        async fn change_password(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            if !self.can_change_password() {
                return;
            }

            let password = self.password.text();

            self.button.set_is_loading(true);
            self.password.set_sensitive(false);
            self.confirm_password.set_sensitive(false);

            let obj = self.obj();
            let dialog = AuthDialog::new(&session);

            let result = dialog
                .authenticate(&*obj, move |client, auth| {
                    let password = password.clone();
                    async move { client.account().change_password(&password, auth).await }
                })
                .await;

            match result {
                Ok(_) => {
                    toast!(obj, gettext("Password changed successfully"));
                    self.password.set_text("");
                    self.confirm_password.set_text("");
                    let _ = obj.activate_action("account-settings.close-subpage", None);
                }
                Err(error) => match error {
                    AuthError::UserCancelled => {}
                    AuthError::ServerResponse(error)
                        if matches!(
                            error.client_api_error_kind(),
                            Some(ErrorKind::WeakPassword)
                        ) =>
                    {
                        error!("Weak password: {error}");
                        toast!(obj, gettext("Password rejected for being too weak"));
                    }
                    _ => {
                        error!("Could not change the password: {error}");
                        toast!(obj, gettext("Could not change password"));
                    }
                },
            }

            self.button.set_is_loading(false);
            self.password.set_sensitive(true);
            self.confirm_password.set_sensitive(true);
        }
    }
}

glib::wrapper! {
    /// Subpage allowing the user to change the account's password.
    pub struct ChangePasswordSubpage(ObjectSubclass<imp::ChangePasswordSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ChangePasswordSubpage {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
