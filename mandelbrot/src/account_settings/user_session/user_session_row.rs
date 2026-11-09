use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::{session::UserSession, utils::TemplateCallbacks};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/user_session/user_session_row.ui"
    )]
    #[properties(wrapper_type = super::UserSessionRow)]
    pub struct UserSessionRow {
        /// The user session displayed by this row.
        #[property(get, set = Self::set_user_session, construct_only)]
        user_session: RefCell<Option<UserSession>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserSessionRow {
        const NAME: &'static str = "UserSessionRow";
        type Type = super::UserSessionRow;
        type ParentType = gtk::ListBoxRow;

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
    impl ObjectImpl for UserSessionRow {}

    impl WidgetImpl for UserSessionRow {}
    impl ListBoxRowImpl for UserSessionRow {}

    #[gtk::template_callbacks]
    impl UserSessionRow {
        /// Set the user session displayed by this row.
        fn set_user_session(&self, user_session: UserSession) {
            let obj = self.obj();

            self.user_session.replace(Some(user_session));

            obj.notify_user_session();
        }
    }
}

glib::wrapper! {
    /// A row presenting a user session.
    pub struct UserSessionRow(ObjectSubclass<imp::UserSessionRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl UserSessionRow {
    pub fn new(user_session: &UserSession) -> Self {
        glib::Object::builder()
            .property("user-session", user_session)
            .build()
    }
}
