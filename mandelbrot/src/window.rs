use std::cell::Cell;

use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, gio, glib, glib::clone};
use tracing::{error, warn};

use crate::{
    APP_ID, Application, PROFILE, SETTINGS_KEY_CURRENT_SESSION,
    account_chooser_dialog::AccountChooserDialog,
    account_settings::AccountSettings,
    account_switcher::{AccountSwitcherButton, AccountSwitcherPopover},
    components::OfflineBanner,
    error_page::ErrorPage,
    intent::SessionIntent,
    login::Login,
    prelude::*,
    secret::SESSION_ID_LENGTH,
    session::{Session, SessionState},
    session_list::{FailedSession, SessionInfo},
    session_view::SessionView,
    toast,
    utils::{FixedSelection, LoadingState},
};

/// A page of the main window stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowPage {
    /// The loading page.
    Loading,
    /// The login view.
    Login,
    /// The session view.
    Session,
    /// The error page.
    Error,
}

impl WindowPage {
    /// Get the name of this page.
    const fn name(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Login => "login",
            Self::Session => "session",
            Self::Error => "error",
        }
    }

    /// Get the page matching the given name.
    ///
    /// Panics if the name does not match any of the variants.
    fn from_name(name: &str) -> Self {
        match name {
            "loading" => Self::Loading,
            "login" => Self::Login,
            "session" => Self::Session,
            "error" => Self::Error,
            _ => panic!("Unknown WindowPage: {name}"),
        }
    }
}

mod imp {
    use std::{cell::RefCell, rc::Rc};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/window.ui")]
    #[properties(wrapper_type = super::Window)]
    pub struct Window {
        #[template_child]
        main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        loading: TemplateChild<gtk::WindowHandle>,
        #[template_child]
        login: TemplateChild<Login>,
        #[template_child]
        error_page: TemplateChild<ErrorPage>,
        #[template_child]
        pub(super) session_view: TemplateChild<SessionView>,
        #[template_child]
        toast_overlay: TemplateChild<adw::ToastOverlay>,
        /// Whether the window should be in compact view.
        ///
        /// It means that the horizontal size is not large enough to hold all
        /// the content.
        #[property(get, set = Self::set_compact, explicit_notify)]
        compact: Cell<bool>,
        /// The selection of the logged-in sessions.
        ///
        /// The one that is selected being the one that is visible.
        #[property(get)]
        session_selection: FixedSelection,
        /// The account switcher popover.
        pub(super) account_switcher: AccountSwitcherPopover,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "Window";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            AccountSwitcherButton::ensure_type();
            OfflineBanner::ensure_type();

            Self::bind_template(klass);

            klass.add_binding_action(gdk::Key::v, gdk::ModifierType::CONTROL_MASK, "win.paste");
            klass.add_binding_action(gdk::Key::Insert, gdk::ModifierType::SHIFT_MASK, "win.paste");
            klass.install_action("win.paste", None, |obj, _, _| {
                obj.imp().session_view.handle_paste_action();
            });

            klass.install_action(
                "win.open-account-settings",
                Some(&String::static_variant_type()),
                |obj, _, variant| {
                    if let Some(session_id) = variant.and_then(glib::Variant::get::<String>) {
                        obj.imp().open_account_settings(&session_id);
                    }
                },
            );

            klass.install_action("win.new-session", None, |obj, _, _| {
                obj.imp().set_visible_page(WindowPage::Login);
            });
            klass.install_action("win.show-session", None, |obj, _, _| {
                obj.imp().show_session();
            });

            klass.install_action("win.toggle-fullscreen", None, |obj, _, _| {
                if obj.is_fullscreen() {
                    obj.unfullscreen();
                } else {
                    obj.fullscreen();
                }
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();

            // Development Profile
            if PROFILE.should_use_devel_class() {
                self.obj().add_css_class("devel");
            }

            self.load_window_size();

            self.main_stack.connect_transition_running_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |stack| if !stack.is_transition_running() {
                    // Focus the default widget when the transition has ended.
                    imp.grab_focus();
                }
            ));

            self.account_switcher
                .set_session_selection(Some(self.session_selection.clone()));

            self.session_selection.set_item_equivalence_fn(|lhs, rhs| {
                let lhs = lhs
                    .downcast_ref::<SessionInfo>()
                    .expect("session selection item should be a SessionInfo");
                let rhs = rhs
                    .downcast_ref::<SessionInfo>()
                    .expect("session selection item should be a SessionInfo");

                lhs.session_id() == rhs.session_id()
            });
            self.session_selection.connect_selected_item_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_selected_session();
                }
            ));
            self.session_selection.connect_is_empty_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |session_selection| {
                    imp.obj()
                        .action_set_enabled("win.show-session", !session_selection.is_empty());
                }
            ));

            let app = Application::default();
            let session_list = app.session_list();

            self.session_selection.set_model(Some(session_list.clone()));

            if session_list.state() == LoadingState::Ready {
                self.finish_session_selection_init();
            } else {
                session_list.connect_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |session_list| {
                        if session_list.state() == LoadingState::Ready {
                            imp.finish_session_selection_init();
                        }
                    }
                ));
            }
        }
    }

    impl WindowImpl for Window {
        fn close_request(&self) -> glib::Propagation {
            if let Err(error) = self.save_window_size() {
                warn!("Could not save window state: {error}");
            }
            if let Err(error) = self.save_current_visible_session() {
                warn!("Could not save current session: {error}");
            }

            glib::Propagation::Proceed
        }
    }

    impl WidgetImpl for Window {
        fn grab_focus(&self) -> bool {
            match self.visible_page() {
                WindowPage::Loading => false,
                WindowPage::Login => self.login.grab_focus(),
                WindowPage::Session => self.session_view.grab_focus(),
                WindowPage::Error => self.error_page.grab_focus(),
            }
        }
    }

    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}

    impl Window {
        /// Set whether the window should be in compact view.
        fn set_compact(&self, compact: bool) {
            if compact == self.compact.get() {
                return;
            }

            self.compact.set(compact);
            self.obj().notify_compact();
        }

        /// Finish the initialization of the session selection, when the session
        /// list is ready.
        fn finish_session_selection_init(&self) {
            for item in self.session_selection.iter::<glib::Object>() {
                if let Some(failed) = item.ok().and_downcast_ref::<FailedSession>() {
                    toast!(self.obj(), failed.error().to_user_facing());
                }
            }

            self.restore_current_visible_session();

            self.session_selection.connect_selected_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |session_selection| {
                    if session_selection.selected() == gtk::INVALID_LIST_POSITION {
                        imp.select_first_session();
                    }
                }
            ));

            if self.session_selection.selected() == gtk::INVALID_LIST_POSITION {
                self.select_first_session();
            }
        }

        /// Select the first session in the session list.
        ///
        /// To be used when there is no current selection.
        fn select_first_session(&self) {
            // Select the first session in the list.
            let selected_session = self.session_selection.item(0);

            if selected_session.is_none() {
                // There are no more sessions.
                self.set_visible_page(WindowPage::Login);
            }

            self.session_selection.set_selected_item(selected_session);
        }

        /// Load the window size from the settings.
        fn load_window_size(&self) {
            let obj = self.obj();
            let settings = Application::default().settings();

            let width = settings.int("window-width");
            let height = settings.int("window-height");
            let is_maximized = settings.boolean("is-maximized");

            obj.set_default_size(width, height);
            obj.set_maximized(is_maximized);
        }

        /// Save the current window size to the settings.
        fn save_window_size(&self) -> Result<(), glib::BoolError> {
            let obj = self.obj();
            let settings = Application::default().settings();

            let size = obj.default_size();
            settings.set_int("window-width", size.0)?;
            settings.set_int("window-height", size.1)?;

            settings.set_boolean("is-maximized", obj.is_maximized())?;

            Ok(())
        }

        /// Restore the currently visible session from the settings.
        fn restore_current_visible_session(&self) {
            let settings = Application::default().settings();
            let mut current_session_setting =
                settings.string(SETTINGS_KEY_CURRENT_SESSION).to_string();

            // Session IDs have been truncated in version 6 of StoredSession.
            if current_session_setting.len() > SESSION_ID_LENGTH {
                current_session_setting.truncate(SESSION_ID_LENGTH);

                if let Err(error) =
                    settings.set_string(SETTINGS_KEY_CURRENT_SESSION, &current_session_setting)
                {
                    warn!("Could not save current session: {error}");
                }
            }

            if let Some(session) = Application::default()
                .session_list()
                .get(&current_session_setting)
            {
                self.session_selection.set_selected_item(Some(session));
            }
        }

        /// Save the currently visible session to the settings.
        fn save_current_visible_session(&self) -> Result<(), glib::BoolError> {
            let settings = Application::default().settings();

            settings.set_string(
                SETTINGS_KEY_CURRENT_SESSION,
                self.current_session_id().unwrap_or_default().as_str(),
            )?;

            Ok(())
        }

        /// The visible page of the window.
        pub(super) fn visible_page(&self) -> WindowPage {
            WindowPage::from_name(
                &self
                    .main_stack
                    .visible_child_name()
                    .expect("stack should always have a visible child name"),
            )
        }

        /// The ID of the currently visible session, if any.
        pub(super) fn current_session_id(&self) -> Option<String> {
            self.session_selection
                .selected_item()
                .and_downcast::<SessionInfo>()
                .map(|s| s.session_id())
        }

        /// Set the current session by its ID.
        ///
        /// Returns `true` if the session was set as the current session.
        pub(super) fn set_current_session_by_id(&self, session_id: &str) -> bool {
            let Some(index) = Application::default().session_list().index(session_id) else {
                return false;
            };

            let index = index as u32;
            let prev_selected = self.session_selection.selected();

            if index == prev_selected {
                // Make sure the session is displayed;
                self.show_session();
            } else {
                self.session_selection.set_selected(index);
            }

            true
        }

        /// Update the selected session in the session view.
        fn update_selected_session(&self) {
            let Some(selected_session) = self
                .session_selection
                .selected_item()
                .and_downcast::<SessionInfo>()
            else {
                return;
            };

            let session = selected_session.downcast_ref::<Session>();
            self.session_view.set_session(session);

            // Show the selected session automatically only if we are not showing a more
            // important view.
            if matches!(
                self.visible_page(),
                WindowPage::Session | WindowPage::Loading
            ) {
                self.show_session();
            }
        }

        /// Show the selected session.
        ///
        /// The displayed view will change according to the current session.
        pub(super) fn show_session(&self) {
            let Some(selected_session) = self
                .session_selection
                .selected_item()
                .and_downcast::<SessionInfo>()
            else {
                return;
            };

            if let Some(session) = selected_session.downcast_ref::<Session>() {
                if session.state() == SessionState::Ready {
                    self.set_visible_page(WindowPage::Session);
                } else {
                    let ready_handler_cell: Rc<RefCell<Option<glib::SignalHandlerId>>> =
                        Rc::default();
                    let ready_handler = session.connect_ready(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        #[strong]
                        ready_handler_cell,
                        move |session| {
                            if let Some(handler) = ready_handler_cell.take() {
                                session.disconnect(handler);
                            }

                            imp.update_selected_session();
                        }
                    ));
                    ready_handler_cell.replace(Some(ready_handler));

                    self.set_visible_page(WindowPage::Loading);
                }

                // We need to grab the focus so that keyboard shortcuts work.
                self.session_view.grab_focus();
            } else if let Some(failed) = selected_session.downcast_ref::<FailedSession>() {
                self.error_page
                    .display_session_error(&failed.error().to_user_facing());
                self.set_visible_page(WindowPage::Error);
            } else {
                self.set_visible_page(WindowPage::Loading);
            }
        }

        /// Set the visible page of the window.
        fn set_visible_page(&self, page: WindowPage) {
            self.main_stack.set_visible_child_name(page.name());
        }

        /// Open the error page and display the given secret error message.
        pub(super) fn show_secret_error(&self, message: &str) {
            self.error_page.display_secret_error(message);
            self.set_visible_page(WindowPage::Error);
        }

        /// Add the given toast to the queue.
        pub(super) fn add_toast(&self, toast: adw::Toast) {
            self.toast_overlay.add_toast(toast);
        }

        /// Open the account settings for the session with the given ID.
        fn open_account_settings(&self, session_id: &str) {
            let Some(session) = Application::default()
                .session_list()
                .get(session_id)
                .and_downcast::<Session>()
            else {
                error!("Tried to open account settings of unknown session with ID '{session_id}'");
                return;
            };

            let dialog = AccountSettings::new(&session);
            dialog.present(Some(&*self.obj()));
        }
    }
}

glib::wrapper! {
    /// The main window.
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Root, gtk::Native,
                    gtk::ShortcutManager, gio::ActionMap, gio::ActionGroup;
}

impl Window {
    pub fn new(app: &Application) -> Self {
        glib::Object::builder()
            .property("application", Some(app))
            .property("icon-name", Some(APP_ID))
            .build()
    }

    /// Add the given session to the session list and select it.
    pub(crate) fn add_session(&self, session: Session) {
        let index = Application::default().session_list().insert(session);
        self.session_selection().set_selected(index as u32);
        self.imp().show_session();
    }

    /// The ID of the currently visible session, if any.
    pub(crate) fn current_session_id(&self) -> Option<String> {
        self.imp().current_session_id()
    }

    /// Add the given toast to the queue.
    pub(crate) fn add_toast(&self, toast: adw::Toast) {
        self.imp().add_toast(toast);
    }

    /// The account switcher popover.
    pub(crate) fn account_switcher(&self) -> &AccountSwitcherPopover {
        &self.imp().account_switcher
    }

    /// The `SessionView` of this window.
    pub(crate) fn session_view(&self) -> &SessionView {
        &self.imp().session_view
    }

    /// Open the error page and display the given secret error message.
    pub(crate) fn show_secret_error(&self, message: &str) {
        self.imp().show_secret_error(message);
    }

    /// Ask the user to choose a session.
    ///
    /// The session list must be ready.
    ///
    /// Returns the ID of the selected session, if any.
    pub(crate) async fn ask_session(&self) -> Option<String> {
        let dialog = AccountChooserDialog::new(Application::default().session_list());
        dialog.choose_account(self).await
    }

    /// Process the given session intent.
    ///
    /// The session must be ready.
    pub(crate) fn process_session_intent(&self, session_id: &str, intent: SessionIntent) {
        if !self.imp().set_current_session_by_id(session_id) {
            error!("Cannot switch to unknown session with ID `{session_id}`");
            return;
        }

        self.session_view().process_intent(intent);
    }
}
