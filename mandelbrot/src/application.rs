use std::{borrow::Cow, cell::RefCell, fmt, rc::Rc};

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};
use tracing::{debug, error, info, warn};

use crate::{
    GETTEXT_PACKAGE, Window, config,
    intent::SessionIntent,
    prelude::*,
    session::{Session, SessionState},
    session_list::{FailedSession, SessionInfo, SessionList},
    spawn,
    system_settings::SystemSettings,
    toast,
    utils::{BoundObjectWeakRef, LoadingState, matrix::MatrixIdUri},
};

/// The key for the current session setting.
pub(crate) const SETTINGS_KEY_CURRENT_SESSION: &str = "current-session";
/// The name of the application.
pub(crate) const APP_NAME: &str = "Mandelbrot";
/// The URL of the homepage of the application.
pub(crate) const APP_HOMEPAGE_URL: &str = "https://gitlab.gnome.org/World/fractal/";

mod imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Debug)]
    pub struct Application {
        /// The application settings.
        pub(super) settings: gio::Settings,
        /// The system settings.
        pub(super) system_settings: SystemSettings,
        /// The list of logged-in sessions.
        pub(super) session_list: SessionList,
        intent_handler: BoundObjectWeakRef<glib::Object>,
        last_network_state: Cell<NetworkState>,
    }

    impl Default for Application {
        fn default() -> Self {
            Self {
                settings: gio::Settings::new(config::APP_ID),
                system_settings: Default::default(),
                session_list: Default::default(),
                intent_handler: Default::default(),
                last_network_state: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "Application";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {
        fn constructed(&self) {
            self.parent_constructed();

            // Initialize actions and accelerators.
            self.set_up_gactions();
            self.set_up_accels();

            // Listen to errors in the session list.
            self.session_list.connect_error_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |session_list| {
                    if let Some(message) = session_list.error() {
                        let window = imp.present_main_window();
                        window.show_secret_error(&message);
                    }
                }
            ));

            // Restore the sessions.
            spawn!(clone!(
                #[weak(rename_to = session_list)]
                self.session_list,
                async move {
                    session_list.restore_sessions().await;
                }
            ));

            // Watch the network to log its state.
            let network_monitor = gio::NetworkMonitor::default();
            network_monitor.connect_network_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |network_monitor, _| {
                    let network_state = NetworkState::with_monitor(network_monitor);

                    if imp.last_network_state.get() == network_state {
                        return;
                    }

                    network_state.log();
                    imp.last_network_state.set(network_state);
                }
            ));
        }
    }

    impl ApplicationImpl for Application {
        fn activate(&self) {
            self.parent_activate();

            debug!("Application::activate");

            self.present_main_window();
        }

        fn startup(&self) {
            self.parent_startup();

            // Set icons for shell
            gtk::Window::set_default_icon_name(crate::APP_ID);
        }

        fn open(&self, files: &[gio::File], _hint: &str) {
            debug!("Application::open");

            self.present_main_window();

            if files.len() > 1 {
                warn!("Trying to open several URIs, only the first one will be processed");
            }

            if let Some(uri) = files.first().map(FileExt::uri) {
                self.process_uri(&uri);
            } else {
                debug!("No URI to open");
            }
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}

    impl Application {
        /// Get or create the main window and make sure it is visible.
        ///
        /// Returns the main window.
        fn present_main_window(&self) -> Window {
            let window = if let Some(window) = self.obj().active_window().and_downcast() {
                window
            } else {
                Window::new(&self.obj())
            };

            window.present();
            window
        }

        /// Set up the application actions.
        fn set_up_gactions(&self) {
            self.obj().add_action_entries([
                // Quit
                gio::ActionEntry::builder("quit")
                    .activate(|obj: &super::Application, _, _| {
                        if let Some(window) = obj.active_window() {
                            // This is needed to trigger the close request and save the window
                            // state.
                            window.close();
                        }

                        obj.quit();
                    })
                    .build(),
                // About
                gio::ActionEntry::builder("about")
                    .activate(|obj: &super::Application, _, _| {
                        obj.imp().show_about_dialog();
                    })
                    .build(),
                // Show a room. This is the action triggered when clicking a notification about a
                // message.
                gio::ActionEntry::builder(SessionIntent::SHOW_MATRIX_ID_ACTION_NAME)
                    .parameter_type(Some(&SessionIntent::static_variant_type()))
                    .activate(|obj: &super::Application, _, variant| {
                        debug!(
                            "`{}` action activated",
                            SessionIntent::SHOW_MATRIX_ID_APP_ACTION_NAME
                        );

                        let Some((session_id, intent)) =
                            variant.and_then(SessionIntent::show_matrix_id_from_variant)
                        else {
                            error!(
                                "Activated `{}` action without the proper payload",
                                SessionIntent::SHOW_MATRIX_ID_APP_ACTION_NAME
                            );
                            return;
                        };

                        obj.imp().process_session_intent(session_id, intent);
                    })
                    .build(),
                // Show the call view of a room. This is the action triggered by the accept
                // button of a notification about an incoming call.
                gio::ActionEntry::builder(SessionIntent::SHOW_ROOM_CALL_ACTION_NAME)
                    .parameter_type(Some(&SessionIntent::static_variant_type()))
                    .activate(|obj: &super::Application, _, variant| {
                        debug!(
                            "`{}` action activated",
                            SessionIntent::SHOW_ROOM_CALL_APP_ACTION_NAME
                        );

                        let Some((session_id, intent)) =
                            variant.and_then(SessionIntent::show_room_call_from_variant)
                        else {
                            error!(
                                "Activated `{}` action without the proper payload",
                                SessionIntent::SHOW_ROOM_CALL_APP_ACTION_NAME
                            );
                            return;
                        };

                        obj.imp().process_session_intent(session_id, intent);
                    })
                    .build(),
                // Withdraw a notification. This is the action triggered by the decline button
                // of a notification about an incoming call.
                gio::ActionEntry::builder("withdraw-notification")
                    .parameter_type(Some(glib::VariantTy::STRING))
                    .activate(|obj: &super::Application, _, variant| {
                        if let Some(id) = variant.and_then(glib::Variant::str) {
                            obj.withdraw_notification(id);
                        }
                    })
                    .build(),
                // Show an identity verification. This is the action triggered when clicking a
                // notification about a new verification.
                gio::ActionEntry::builder(SessionIntent::SHOW_IDENTITY_VERIFICATION_ACTION_NAME)
                    .parameter_type(Some(&SessionIntent::static_variant_type()))
                    .activate(|obj: &super::Application, _, variant| {
                        debug!(
                            "`{}` action activated",
                            SessionIntent::SHOW_IDENTITY_VERIFICATION_APP_ACTION_NAME
                        );

                        let Some((session_id, intent)) = variant
                            .and_then(SessionIntent::show_identity_verification_from_variant)
                        else {
                            error!(
                                "Activated `{}` action without the proper payload",
                                SessionIntent::SHOW_IDENTITY_VERIFICATION_APP_ACTION_NAME
                            );
                            return;
                        };

                        obj.imp().process_session_intent(session_id, intent);
                    })
                    .build(),
            ]);
        }

        /// Sets up keyboard shortcuts for application and window actions.
        fn set_up_accels(&self) {
            let obj = self.obj();
            obj.set_accels_for_action("app.quit", &["<Control>q"]);
            obj.set_accels_for_action("window.close", &["<Control>w"]);
        }

        /// Show the dialog with information about the application.
        fn show_about_dialog(&self) {
            let dialog = adw::AboutDialog::builder()
                .application_name(APP_NAME)
                .application_icon(config::APP_ID)
                .developer_name(gettext("TunaOS"))
                .license_type(gtk::License::Gpl30)
                .website(APP_HOMEPAGE_URL)
                .issue_url("https://gitlab.gnome.org/World/fractal/-/issues")
                .support_url("https://matrix.to/#/#fractal:gnome.org")
                .version(config::VERSION)
                .copyright(gettext("© The Fractal Team and Mandelbrot contributors"))
                .developers([
                    "Alejandro Domínguez",
                    "Alexandre Franke",
                    "Bilal Elmoussaoui",
                    "Christopher Davis",
                    "Daniel García Moreno",
                    "Eisha Chen-yen-su",
                    "Jordan Petridis",
                    "Julian Sparber",
                    "Kévin Commaille",
                    "Saurav Sachidanand",
                ])
                .designers(["Tobias Bernard"])
                .translator_credits(gettext("translator-credits"))
                .build();

            // This can't be added via the builder
            dialog.add_credit_section(Some(&gettext("Name by")), &["Regina Bíró"]);

            // If the user wants our support room, try to open it ourselves.
            dialog.connect_activate_link(clone!(
                #[weak(rename_to = imp)]
                self,
                #[weak]
                dialog,
                #[upgrade_or]
                false,
                move |_, uri| {
                    if uri == "https://matrix.to/#/#fractal:gnome.org"
                        && imp.session_list.has_session_ready()
                    {
                        imp.process_uri(uri);
                        dialog.close();
                        return true;
                    }

                    false
                }
            ));

            dialog.present(Some(&self.present_main_window()));
        }

        /// Process the given URI.
        fn process_uri(&self, uri: &str) {
            debug!(uri, "Processing URI…");
            match MatrixIdUri::parse(uri) {
                Ok(matrix_id) => {
                    self.select_session_for_intent(SessionIntent::ShowMatrixId(matrix_id));
                }
                Err(error) => warn!("Invalid Matrix URI: {error}"),
            }
        }

        /// Select a session to handle the given intent as soon as possible.
        fn select_session_for_intent(&self, intent: SessionIntent) {
            debug!(?intent, "Selecting session for intent…");

            // We only handle a single intent at time, the latest one.
            self.intent_handler.disconnect_signals();

            if self.session_list.state() == LoadingState::Ready {
                match self.session_list.n_items() {
                    0 => {
                        warn!("Cannot process intent with no logged in session");
                    }
                    1 => {
                        let session = self
                            .session_list
                            .first()
                            .expect("there should be one session");
                        self.process_session_intent(session.session_id(), intent);
                    }
                    _ => {
                        spawn!(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            async move {
                                imp.ask_session_for_intent(intent).await;
                            }
                        ));
                    }
                }
            } else {
                debug!(?intent, "Session list is not ready, queuing intent…");
                // Wait for the list to be ready.
                let cell = Rc::new(RefCell::new(Some(intent)));
                let handler = self.session_list.connect_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[strong]
                    cell,
                    move |session_list| {
                        if session_list.state() == LoadingState::Ready {
                            imp.intent_handler.disconnect_signals();

                            if let Some(intent) = cell.take() {
                                imp.select_session_for_intent(intent);
                            }
                        }
                    }
                ));
                self.intent_handler
                    .set(self.session_list.upcast_ref(), vec![handler]);
            }
        }

        /// Ask the user to choose a session to process the given Matrix ID URI.
        ///
        /// The session list needs to be ready.
        async fn ask_session_for_intent(&self, intent: SessionIntent) {
            debug!(?intent, "Asking to select a session to process intent…");
            let main_window = self.present_main_window();

            let Some(session_id) = main_window.ask_session().await else {
                warn!("No session selected to show intent");
                return;
            };

            self.process_session_intent(session_id, intent);
        }

        /// Process the given intent for the given session, as soon as the
        /// session is ready.
        fn process_session_intent(&self, session_id: String, intent: SessionIntent) {
            let Some(session_info) = self.session_list.get(&session_id) else {
                warn!(
                    session = session_id,
                    ?intent,
                    "Could not find session to process intent"
                );
                toast!(self.present_main_window(), gettext("Session not found"));
                return;
            };

            debug!(session = session_id, ?intent, "Processing session intent…");

            if session_info.is::<FailedSession>() {
                // We can't do anything, it should show an error screen.
                warn!(
                    session = session_id,
                    ?intent,
                    "Could not process intent for failed session"
                );
            } else if let Some(session) = session_info.downcast_ref::<Session>() {
                if session.state() == SessionState::Ready {
                    self.present_main_window()
                        .process_session_intent(session.session_id(), intent);
                } else {
                    debug!(
                        session = session_id,
                        ?intent,
                        "Session is not ready, queuing intent…"
                    );
                    // Wait for the session to be ready.
                    let cell = Rc::new(RefCell::new(Some((session_id, intent))));
                    let handler = session.connect_ready(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        #[strong]
                        cell,
                        move |_| {
                            imp.intent_handler.disconnect_signals();

                            if let Some((session_id, intent)) = cell.take() {
                                imp.present_main_window()
                                    .process_session_intent(&session_id, intent);
                            }
                        }
                    ));
                    self.intent_handler.set(session.upcast_ref(), vec![handler]);
                }
            } else {
                debug!(
                    session = session_id,
                    ?intent,
                    "Session is still loading, queuing intent…"
                );
                // Wait for the session to be a `Session`.
                let cell = Rc::new(RefCell::new(Some((session_id, intent))));
                let handler = self.session_list.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[strong]
                    cell,
                    move |session_list, pos, _, added| {
                        if added == 0 {
                            return;
                        }
                        let Some(session_id) = cell
                            .borrow()
                            .as_ref()
                            .map(|(session_id, _)| session_id.clone())
                        else {
                            return;
                        };

                        for i in pos..pos + added {
                            let Some(session_info) =
                                session_list.item(i).and_downcast::<SessionInfo>()
                            else {
                                break;
                            };

                            if session_info.session_id() == session_id {
                                imp.intent_handler.disconnect_signals();

                                if let Some((session_id, intent)) = cell.take() {
                                    imp.process_session_intent(session_id, intent);
                                }
                                break;
                            }
                        }
                    }
                ));
                self.intent_handler
                    .set(self.session_list.upcast_ref(), vec![handler]);
            }
        }
    }
}

glib::wrapper! {
    /// The Fractal application.
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionMap, gio::ActionGroup;
}

impl Application {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", Some(config::APP_ID))
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .property("resource-base-path", Some("/org/tunaos/mandelbrot/"))
            .build()
    }

    /// The application settings.
    pub(crate) fn settings(&self) -> gio::Settings {
        self.imp().settings.clone()
    }

    /// The system settings.
    pub(crate) fn system_settings(&self) -> SystemSettings {
        self.imp().system_settings.clone()
    }

    /// The list of logged-in sessions.
    pub(crate) fn session_list(&self) -> &SessionList {
        &self.imp().session_list
    }

    /// Run Fractal.
    pub(crate) fn run(&self) {
        info!("{} ({})", APP_NAME, config::APP_ID);
        info!("Version: {} ({})", config::VERSION, config::PROFILE);
        info!("Datadir: {}", config::PKGDATADIR);

        ApplicationExtManual::run(self);
    }
}

impl Default for Application {
    fn default() -> Self {
        gio::Application::default()
            .and_downcast::<Application>()
            .expect("application should always be available")
    }
}

/// The profile that was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AppProfile {
    /// A stable release.
    Stable,
    /// A beta release.
    Beta,
    /// A development release.
    Devel,
}

impl AppProfile {
    /// The string representation of this `AppProfile`.
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Devel => "devel",
        }
    }

    /// Whether this `AppProfile` should use the `.devel` CSS class on windows.
    pub(crate) fn should_use_devel_class(self) -> bool {
        matches!(self, Self::Devel)
    }

    /// The name of the directory where to put data for this profile.
    pub(crate) fn dir_name(self) -> Cow<'static, str> {
        match self {
            AppProfile::Stable => Cow::Borrowed(GETTEXT_PACKAGE),
            _ => Cow::Owned(format!("{GETTEXT_PACKAGE}-{self}")),
        }
    }
}

impl fmt::Display for AppProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The state of the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkState {
    /// The network is available.
    Unavailable,
    /// The network is available with the given connectivity.
    Available(gio::NetworkConnectivity),
}

impl NetworkState {
    /// Construct the network state with the given network monitor.
    fn with_monitor(monitor: &gio::NetworkMonitor) -> Self {
        if monitor.is_network_available() {
            Self::Available(monitor.connectivity())
        } else {
            Self::Unavailable
        }
    }

    /// Log this network state.
    fn log(self) {
        match self {
            Self::Unavailable => {
                info!("Network is unavailable");
            }
            Self::Available(connectivity) => {
                info!("Network connectivity is {connectivity:?}");
            }
        }
    }
}

impl Default for NetworkState {
    fn default() -> Self {
        Self::Available(gio::NetworkConnectivity::Full)
    }
}
