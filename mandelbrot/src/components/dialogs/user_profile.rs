use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};
use ruma::OwnedUserId;

use super::ToastableDialog;
use crate::{
    components::UserPage,
    prelude::*,
    session::{Member, Session, User},
    utils::LoadingState,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/dialogs/user_profile.ui")]
    pub struct UserProfileDialog {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        user_page: TemplateChild<UserPage>,
        user_loading_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserProfileDialog {
        const NAME: &'static str = "UserProfileDialog";
        type Type = super::UserProfileDialog;
        type ParentType = ToastableDialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for UserProfileDialog {
        fn dispose(&self) {
            self.reset();
        }
    }

    impl WidgetImpl for UserProfileDialog {}
    impl AdwDialogImpl for UserProfileDialog {}
    impl ToastableDialogImpl for UserProfileDialog {}

    impl UserProfileDialog {
        /// Show the details page.
        fn show_details(&self) {
            self.stack.set_visible_child_name("details");
        }

        /// Load the user with the given session and user ID.
        pub(super) fn load_user(&self, session: &Session, user_id: OwnedUserId) {
            self.reset();

            let user = session.remote_cache().user(user_id);
            self.user_page.set_user(Some(user.clone()));

            if matches!(
                user.loading_state(),
                LoadingState::Initial | LoadingState::Loading
            ) {
                let user_loading_handler = user.connect_loading_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |user| {
                        if !matches!(
                            user.loading_state(),
                            LoadingState::Initial | LoadingState::Loading
                        ) && let Some(handler) = imp.user_loading_handler.take()
                        {
                            user.disconnect(handler);
                            imp.show_details();
                        }
                    }
                ));
                self.user_loading_handler
                    .replace(Some(user_loading_handler));
            } else {
                self.show_details();
            }
        }

        /// Set the member to present.
        pub(super) fn set_room_member(&self, member: Member) {
            self.reset();

            self.user_page.set_user(Some(member.upcast::<User>()));
            self.show_details();
        }

        /// Reset this dialog.
        fn reset(&self) {
            if let Some(handler) = self.user_loading_handler.take()
                && let Some(user) = self.user_page.user()
            {
                user.disconnect(handler);
            }
        }
    }
}

glib::wrapper! {
    /// Dialog to view a user's profile.
    pub struct UserProfileDialog(ObjectSubclass<imp::UserProfileDialog>)
        @extends gtk::Widget, adw::Dialog, ToastableDialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl UserProfileDialog {
    /// Create a new `UserProfileDialog`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Load the user with the given session and user ID.
    pub(crate) fn load_user(&self, session: &Session, user_id: OwnedUserId) {
        self.imp().load_user(session, user_id);
    }

    /// Set the member to present.
    pub(crate) fn set_room_member(&self, member: Member) {
        self.imp().set_room_member(member);
    }
}
