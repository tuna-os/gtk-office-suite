use std::fmt::Debug;

use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::components::LoadingButton;

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/dialogs/auth/password_page.ui")]
    pub struct AuthDialogPasswordPage {
        #[template_child]
        pub(super) password: TemplateChild<gtk::PasswordEntry>,
        #[template_child]
        pub(super) confirm_button: TemplateChild<LoadingButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AuthDialogPasswordPage {
        const NAME: &'static str = "AuthDialogPasswordPage";
        type Type = super::AuthDialogPasswordPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AuthDialogPasswordPage {}
    impl WidgetImpl for AuthDialogPasswordPage {}
    impl BinImpl for AuthDialogPasswordPage {}

    #[gtk::template_callbacks]
    impl AuthDialogPasswordPage {
        /// Whether the user can proceed given the current state.
        fn can_proceed(&self) -> bool {
            !self.password.text().is_empty()
        }

        /// Update the confirm button for the current state.
        #[template_callback]
        fn update_confirm(&self) {
            self.confirm_button.set_sensitive(self.can_proceed());
        }

        /// Proceed to authentication with the current password.
        #[template_callback]
        fn proceed(&self) {
            if !self.can_proceed() {
                return;
            }

            self.confirm_button.set_is_loading(true);
            let _ = self.obj().activate_action("auth-dialog.continue", None);
        }

        /// Retry this stage.
        pub(super) fn retry(&self) {
            self.confirm_button.set_is_loading(false);
            self.update_confirm();
        }
    }
}

glib::wrapper! {
    /// Page to pass the password stage for the [`AuthDialog`].
    pub struct AuthDialogPasswordPage(ObjectSubclass<imp::AuthDialogPasswordPage>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AuthDialogPasswordPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Get the default widget of this page.
    pub fn default_widget(&self) -> &gtk::Widget {
        self.imp().confirm_button.upcast_ref()
    }

    /// Get the current password in the entry.
    pub fn password(&self) -> String {
        self.imp().password.text().into()
    }

    /// Retry this stage.
    pub fn retry(&self) {
        self.imp().retry();
    }
}
