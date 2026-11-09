use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, glib::clone};
use tracing::error;

use super::UserSessionRow;
use crate::{
    prelude::*,
    session::{Session, UserSession, UserSessionsList},
    utils::{BoundObject, LoadingState},
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/account_settings/user_session/user_session_list_subpage.ui"
    )]
    #[properties(wrapper_type = super::UserSessionListSubpage)]
    pub struct UserSessionListSubpage {
        #[template_child]
        pub(super) link_device_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        current_session_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        current_session: TemplateChild<gtk::ListBox>,
        #[template_child]
        other_sessions_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        other_sessions: TemplateChild<gtk::ListBox>,
        /// The list of user sessions.
        #[property(get, set = Self::set_user_sessions, explicit_notify, nullable)]
        user_sessions: BoundObject<UserSessionsList>,
        other_sessions_sorted_model: gtk::SortListModel,
        other_sessions_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserSessionListSubpage {
        const NAME: &'static str = "UserSessionListSubpage";
        type Type = super::UserSessionListSubpage;
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
    impl ObjectImpl for UserSessionListSubpage {
        fn constructed(&self) {
            self.parent_constructed();

            self.init_other_sessions();
        }

        fn dispose(&self) {
            if let Some(user_sessions) = self.user_sessions.obj()
                && let Some(handler) = self.other_sessions_handler.take()
            {
                user_sessions.other_sessions().disconnect(handler);
            }

            // AdwPreferencesPage doesn't handle children other than AdwPreferencesGroup.
            self.stack.unparent();
        }
    }

    impl WidgetImpl for UserSessionListSubpage {}
    impl NavigationPageImpl for UserSessionListSubpage {}

    #[gtk::template_callbacks]
    impl UserSessionListSubpage {
        /// Set the list of user sessions.
        fn set_user_sessions(&self, user_sessions: Option<UserSessionsList>) {
            let prev_user_sessions = self.user_sessions.obj();

            if prev_user_sessions == user_sessions {
                return;
            }

            if let Some(user_sessions) = prev_user_sessions
                && let Some(handler) = self.other_sessions_handler.take()
            {
                user_sessions.other_sessions().disconnect(handler);
            }
            self.user_sessions.disconnect_signals();

            if let Some(user_sessions) = user_sessions {
                let other_sessions = user_sessions.other_sessions();

                let other_sessions_handler = other_sessions.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |other_sessions, _, _, _| {
                        imp.other_sessions_group
                            .set_visible(other_sessions.n_items() > 0);
                    }
                ));
                self.other_sessions_handler
                    .replace(Some(other_sessions_handler));
                self.other_sessions_group
                    .set_visible(other_sessions.n_items() > 0);
                self.other_sessions_sorted_model
                    .set_model(Some(&other_sessions));

                let loading_state_handler = user_sessions.connect_loading_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_other_sessions_state();
                    }
                ));
                let is_empty_handler = user_sessions.connect_is_empty_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_other_sessions_state();
                    }
                ));
                let current_session_handler = user_sessions.connect_current_session_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_current_session();
                    }
                ));

                self.user_sessions.set(
                    user_sessions,
                    vec![
                        loading_state_handler,
                        is_empty_handler,
                        current_session_handler,
                    ],
                );
            } else {
                self.other_sessions_sorted_model
                    .set_model(None::<&gio::ListModel>);
            }

            self.obj().notify_user_sessions();

            self.update_current_session();
            self.update_other_sessions_state();
        }

        /// Initialize the list of other sessions.
        fn init_other_sessions(&self) {
            let last_seen_ts_sorter = gtk::NumericSorter::builder()
                .expression(UserSession::this_expression("last-seen-ts"))
                .sort_order(gtk::SortType::Descending)
                .build();
            let device_id_sorter =
                gtk::StringSorter::new(Some(UserSession::this_expression("device-id-string")));
            let multi_sorter = gtk::MultiSorter::new();
            multi_sorter.append(last_seen_ts_sorter);
            multi_sorter.append(device_id_sorter);
            self.other_sessions_sorted_model
                .set_sorter(Some(&multi_sorter));

            self.other_sessions
                .bind_model(Some(&self.other_sessions_sorted_model), move |item| {
                    let Some(user_session) = item.downcast_ref::<UserSession>() else {
                        error!("Did not get a user session as an item of user session list");
                        return adw::Bin::new().upcast();
                    };

                    UserSessionRow::new(user_session).upcast()
                });
        }

        /// The current page of the other sessions stack according to the
        /// current state.
        fn current_other_sessions_page(&self) -> &str {
            let Some(user_sessions) = self.user_sessions.obj() else {
                return "loading";
            };

            if user_sessions.is_empty() {
                match user_sessions.loading_state() {
                    LoadingState::Error | LoadingState::Ready => "error",
                    _ => "loading",
                }
            } else {
                "list"
            }
        }

        /// Update the state of the UI according to the current state.
        fn update_other_sessions_state(&self) {
            self.stack
                .set_visible_child_name(self.current_other_sessions_page());
        }

        /// Update the section about the current session.
        fn update_current_session(&self) {
            if let Some(child) = self.current_session.first_child() {
                self.current_session.remove(&child);
            }

            let current_session = self.user_sessions.obj().and_then(|s| s.current_session());
            let Some(current_session) = current_session else {
                self.current_session_group.set_visible(false);
                return;
            };

            self.current_session
                .append(&UserSessionRow::new(&current_session));
            self.current_session_group.set_visible(true);
        }

        /// Show the session subpage.
        #[template_callback]
        fn show_session_subpage(&self, row: &UserSessionRow) {
            let obj = self.obj();

            let _ = obj.activate_action(
                "account-settings.show-session-subpage",
                Some(&row.user_session().unwrap().device_id_string().to_variant()),
            );
        }
    }
}

glib::wrapper! {
    /// Subpage to present the sessions of a user.
    pub struct UserSessionListSubpage(ObjectSubclass<imp::UserSessionListSubpage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl UserSessionListSubpage {
    /// Construct a new `UserSessionListSubpage` for the given session.
    pub fn new(session: &Session) -> Self {
        let obj: Self = glib::Object::builder()
            .property("user-sessions", session.user_sessions())
            .build();

        // Linking a new device with a QR code is only possible with the OAuth 2.0 API.
        obj.imp()
            .link_device_group
            .set_visible(session.uses_oauth_api());

        obj
    }
}
