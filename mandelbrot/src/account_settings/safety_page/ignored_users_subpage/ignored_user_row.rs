use gettextrs::gettext;
use gtk::{glib, prelude::*, subclass::prelude::*};
use ruma::UserId;

use crate::{components::LoadingButton, session::IgnoredUsers, toast};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/safety_page/ignored_users_subpage/ignored_user_row.ui"
    )]
    #[properties(wrapper_type = super::IgnoredUserRow)]
    pub struct IgnoredUserRow {
        #[template_child]
        stop_ignoring_button: TemplateChild<LoadingButton>,
        /// The item containing the user ID presented by this row.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: RefCell<Option<gtk::StringObject>>,
        /// The current list of ignored users.
        #[property(get, set, nullable)]
        ignored_users: RefCell<Option<IgnoredUsers>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for IgnoredUserRow {
        const NAME: &'static str = "IgnoredUserRow";
        type Type = super::IgnoredUserRow;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for IgnoredUserRow {}

    impl WidgetImpl for IgnoredUserRow {}
    impl BoxImpl for IgnoredUserRow {}

    #[gtk::template_callbacks]
    impl IgnoredUserRow {
        /// Set the item containing the user ID presented by this row.
        fn set_item(&self, item: Option<gtk::StringObject>) {
            if *self.item.borrow() == item {
                return;
            }

            self.item.replace(item);
            self.obj().notify_item();

            // Reset the state of the button.
            self.stop_ignoring_button.set_is_loading(false);
        }

        /// Stop ignoring the user of this row.
        #[template_callback]
        async fn stop_ignoring_user(&self) {
            let Some(user_id) = self
                .item
                .borrow()
                .as_ref()
                .and_then(|string_object| UserId::parse(string_object.string()).ok())
            else {
                return;
            };
            let Some(ignored_users) = self.ignored_users.borrow().clone() else {
                return;
            };

            self.stop_ignoring_button.set_is_loading(true);

            if ignored_users.remove(&user_id).await.is_err() {
                toast!(self.obj(), gettext("Could not stop ignoring user"));
                self.stop_ignoring_button.set_is_loading(false);
            }
        }
    }
}

glib::wrapper! {
    /// A row presenting an ignored user.
    pub struct IgnoredUserRow(ObjectSubclass<imp::IgnoredUserRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl IgnoredUserRow {
    pub fn new(ignored_users: &IgnoredUsers) -> Self {
        glib::Object::builder()
            .property("ignored-users", ignored_users)
            .build()
    }
}
