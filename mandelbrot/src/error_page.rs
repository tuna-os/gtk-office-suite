use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;

use crate::{APP_ID, toast};

/// The possible error subpages.
#[derive(Debug, Clone, Copy)]
pub enum ErrorSubpage {
    /// The page to present when there was an error with the secret API.
    Secret,
    /// The page to present when there was an error when initializing a session.
    Session,
}

impl ErrorSubpage {
    /// The name of this page.
    const fn name(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Session => "name",
        }
    }
}

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/error_page.ui")]
    pub struct ErrorPage {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        secret_error_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        linux_secret_instructions: TemplateChild<adw::Clamp>,
        #[template_child]
        secret_service_override_command: TemplateChild<gtk::Label>,
        #[template_child]
        session_error_page: TemplateChild<adw::StatusPage>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ErrorPage {
        const NAME: &'static str = "ErrorPage";
        type Type = super::ErrorPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ErrorPage {}
    impl WidgetImpl for ErrorPage {}
    impl BinImpl for ErrorPage {}

    #[gtk::template_callbacks]
    impl ErrorPage {
        /// Display the given secret error.
        pub(super) fn display_secret_error(&self, message: &str) {
            #[cfg(not(target_os = "linux"))]
            self.linux_secret_instructions.set_visible(false);

            #[cfg(target_os = "linux")]
            {
                self.linux_secret_instructions.set_visible(true);

                self.secret_service_override_command.set_label(&format!(
                    "flatpak --user override --talk-name=org.freedesktop.secrets {APP_ID}",
                ));
            }

            self.secret_error_page.set_description(Some(message));
            self.stack
                .set_visible_child_name(ErrorSubpage::Secret.name());
        }

        /// Display the given session error.
        pub(super) fn display_session_error(&self, message: &str) {
            self.session_error_page.set_description(Some(message));
            self.stack
                .set_visible_child_name(ErrorSubpage::Session.name());
        }

        /// Copy the secret service override command to the clipboard.
        #[template_callback]
        fn copy_secret_service_override_command(&self) {
            let obj = self.obj();
            let command = self.secret_service_override_command.label();
            obj.clipboard().set_text(&command);
            toast!(obj, gettext("Command copied to clipboard"));
        }
    }
}

glib::wrapper! {
    /// A view displaying an error.
    pub struct ErrorPage(ObjectSubclass<imp::ErrorPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ErrorPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Display the given secret error.
    pub(crate) fn display_secret_error(&self, message: &str) {
        self.imp().display_secret_error(message);
    }

    /// Display the given session error.
    pub(crate) fn display_session_error(&self, message: &str) {
        self.imp().display_session_error(message);
    }
}
