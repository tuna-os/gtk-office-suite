use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};
use tracing::error;

mod ignored_user_row;

use self::ignored_user_row::IgnoredUserRow;
use crate::session::Session;

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/safety_page/ignored_users_subpage/mod.ui"
    )]
    #[properties(wrapper_type = super::IgnoredUsersSubpage)]
    pub struct IgnoredUsersSubpage {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        list_view: TemplateChild<gtk::ListView>,
        filtered_model: gtk::FilterListModel,
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
        items_changed_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for IgnoredUsersSubpage {
        const NAME: &'static str = "IgnoredUsersSubpage";
        type Type = super::IgnoredUsersSubpage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for IgnoredUsersSubpage {
        fn constructed(&self) {
            self.parent_constructed();

            // Needed because the GtkSearchEntry is not the direct child of the
            // GtkSearchBar.
            self.search_bar.connect_entry(&*self.search_entry);

            let search_filter = gtk::StringFilter::builder()
                .match_mode(gtk::StringFilterMatchMode::Substring)
                .expression(gtk::StringObject::this_expression("string"))
                .ignore_case(true)
                .build();

            self.search_entry
                .bind_property("text", &search_filter, "search")
                .sync_create()
                .build();

            self.filtered_model.set_filter(Some(&search_filter));

            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, item| {
                    let Some(session) = imp.session.upgrade() else {
                        return;
                    };
                    let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                        error!("List item factory did not receive a list item: {item:?}");
                        return;
                    };

                    let row = IgnoredUserRow::new(&session.ignored_users());
                    item.set_child(Some(&row));
                    item.bind_property("item", &row, "item").build();
                    item.set_activatable(false);
                    item.set_selectable(false);
                }
            ));
            self.list_view.set_factory(Some(&factory));

            self.list_view.set_model(Some(&gtk::NoSelection::new(Some(
                self.filtered_model.clone(),
            ))));
        }

        fn dispose(&self) {
            if let Some(session) = self.session.upgrade()
                && let Some(handler) = self.items_changed_handler.take()
            {
                session.ignored_users().disconnect(handler);
            }
        }
    }

    impl WidgetImpl for IgnoredUsersSubpage {}
    impl NavigationPageImpl for IgnoredUsersSubpage {}

    impl IgnoredUsersSubpage {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            let prev_session = self.session.upgrade();

            if prev_session.as_ref() == session {
                return;
            }

            if let Some(session) = prev_session
                && let Some(handler) = self.items_changed_handler.take()
            {
                session.ignored_users().disconnect(handler);
            }

            let ignored_users = session.map(Session::ignored_users);
            if let Some(ignored_users) = &ignored_users {
                let items_changed_handler = ignored_users.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _, _| {
                        imp.update_visible_page();
                    }
                ));
                self.items_changed_handler
                    .replace(Some(items_changed_handler));
            }

            self.filtered_model.set_model(ignored_users.as_ref());
            self.session.set(session);

            self.obj().notify_session();
            self.update_visible_page();
        }

        /// Update the visible page according to the current state.
        fn update_visible_page(&self) {
            let has_users = self
                .session
                .upgrade()
                .is_some_and(|s| s.ignored_users().n_items() > 0);

            let page = if has_users { "list" } else { "empty" };
            self.stack.set_visible_child_name(page);
        }
    }
}

glib::wrapper! {
    /// A subpage with the list of ignored users.
    pub struct IgnoredUsersSubpage(ObjectSubclass<imp::IgnoredUsersSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl IgnoredUsersSubpage {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }
}
