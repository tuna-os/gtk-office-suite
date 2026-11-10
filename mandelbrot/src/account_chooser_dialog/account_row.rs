use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::{
    components::{Avatar, AvatarData},
    prelude::*,
    session::Session,
    session_list::{FailedSession, SessionInfo},
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/account_chooser_dialog/account_row.ui")]
    #[properties(wrapper_type = super::AccountRow)]
    pub struct AccountRow {
        #[template_child]
        pub avatar: TemplateChild<Avatar>,
        #[template_child]
        pub display_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub user_id: TemplateChild<gtk::Label>,
        #[template_child]
        pub state_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub error_image: TemplateChild<gtk::Image>,
        /// The session this item represents.
        #[property(get, set = Self::set_session, explicit_notify)]
        pub session: glib::WeakRef<SessionInfo>,
        pub user_bindings: RefCell<Vec<glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AccountRow {
        const NAME: &'static str = "AccountChooserDialogRow";
        type Type = super::AccountRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AccountRow {
        fn dispose(&self) {
            for binding in self.user_bindings.take() {
                binding.unbind();
            }
        }
    }

    impl WidgetImpl for AccountRow {}
    impl ListBoxRowImpl for AccountRow {}

    impl AccountRow {
        /// Set the session this item represents.
        fn set_session(&self, session: Option<&SessionInfo>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            for binding in self.user_bindings.take() {
                binding.unbind();
            }

            if let Some(session) = session {
                if let Some(session) = session.downcast_ref::<Session>() {
                    let user = session.user();

                    let avatar_data_handler = user
                        .bind_property("avatar-data", &*self.avatar, "data")
                        .sync_create()
                        .build();
                    let display_name_handler = user
                        .bind_property("display-name", &*self.display_name, "label")
                        .sync_create()
                        .build();
                    self.user_bindings
                        .borrow_mut()
                        .extend([avatar_data_handler, display_name_handler]);

                    self.user_id.set_label(session.user_id().as_str());
                    self.user_id.set_visible(true);

                    self.state_stack.set_visible(false);
                } else {
                    let user_id = session.user_id().to_string();

                    let avatar_data = AvatarData::new();
                    avatar_data.set_display_name(user_id.clone());
                    self.avatar.set_data(Some(avatar_data));

                    self.display_name.set_label(&user_id);
                    self.user_id.set_visible(false);

                    if let Some(failed) = session.downcast_ref::<FailedSession>() {
                        self.error_image
                            .set_tooltip_text(Some(&failed.error().to_user_facing()));
                        self.state_stack.set_visible_child_name("error");
                    } else {
                        self.state_stack.set_visible_child_name("loading");
                    }
                    self.state_stack.set_visible(true);
                }
            }

            self.session.set(session);
            self.obj().notify_session();
        }
    }
}

glib::wrapper! {
    /// A `GtkListBoxRow` representing a logged-in session in the `AccountChooserDialog`.
    pub struct AccountRow(ObjectSubclass<imp::AccountRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl AccountRow {
    pub fn new(session: &SessionInfo) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
