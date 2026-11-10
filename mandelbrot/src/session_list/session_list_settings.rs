use gtk::{glib, prelude::*, subclass::prelude::*};
use indexmap::{IndexMap, IndexSet};
use tracing::error;

use crate::{
    Application,
    secret::SESSION_ID_LENGTH,
    session::{SessionSettings, StoredSessionSettings},
};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default)]
    pub struct SessionListSettings {
        /// The settings of the sessions.
        pub(super) sessions: RefCell<IndexMap<String, SessionSettings>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionListSettings {
        const NAME: &'static str = "SessionListSettings";
        type Type = super::SessionListSettings;
    }

    impl ObjectImpl for SessionListSettings {}
}

glib::wrapper! {
    /// The settings of the list of sessions.
    pub struct SessionListSettings(ObjectSubclass<imp::SessionListSettings>);
}

impl SessionListSettings {
    /// Create a new `SessionListSettings`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Load these settings from the application settings.
    pub(crate) fn load(&self) {
        let serialized = Application::default().settings().string("sessions");

        let stored_sessions =
            match serde_json::from_str::<Vec<(String, StoredSessionSettings)>>(&serialized) {
                Ok(stored_sessions) => stored_sessions,
                Err(error) => {
                    error!(
                        "Could not load sessions settings, fallback to default settings: {error}"
                    );
                    Default::default()
                }
            };

        // Do we need to update the settings?
        let mut needs_update = false;

        let sessions = stored_sessions
            .into_iter()
            .map(|(mut session_id, stored_session)| {
                // Session IDs have been truncated in version 6 of StoredSession.
                if session_id.len() > SESSION_ID_LENGTH {
                    session_id.truncate(SESSION_ID_LENGTH);
                    needs_update = true;
                }

                let session = SessionSettings::restore(&session_id, stored_session);
                (session_id, session)
            })
            .collect();

        self.imp().sessions.replace(sessions);

        if needs_update {
            self.save();
        }
    }

    /// Save these settings in the application settings.
    pub(crate) fn save(&self) {
        let stored_sessions = self
            .imp()
            .sessions
            .borrow()
            .iter()
            .map(|(session_id, session)| (session_id.clone(), session.stored_settings()))
            .collect::<Vec<_>>();

        if let Err(error) = Application::default().settings().set_string(
            "sessions",
            &serde_json::to_string(&stored_sessions).unwrap(),
        ) {
            error!("Could not save sessions settings: {error}");
        }
    }

    /// Get or create the settings for the session with the given ID.
    pub(crate) fn get_or_create(&self, session_id: &str) -> SessionSettings {
        let sessions = &self.imp().sessions;

        if let Some(session) = sessions.borrow().get(session_id) {
            return session.clone();
        }

        let session = SessionSettings::new(session_id);
        sessions
            .borrow_mut()
            .insert(session_id.to_owned(), session.clone());
        self.save();

        session
    }

    /// Remove the settings of the session with the given ID.
    pub(crate) fn remove(&self, session_id: &str) {
        self.imp().sessions.borrow_mut().shift_remove(session_id);
        self.save();
    }

    /// Get the list of session IDs stored in these settings.
    pub(crate) fn session_ids(&self) -> IndexSet<String> {
        self.imp().sessions.borrow().keys().cloned().collect()
    }
}

impl Default for SessionListSettings {
    fn default() -> Self {
        Self::new()
    }
}
