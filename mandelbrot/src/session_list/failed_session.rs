use gtk::{glib, subclass::prelude::*};

use super::{SessionInfo, SessionInfoImpl};
use crate::{
    components::AvatarData, prelude::*, secret::StoredSession, utils::matrix::ClientSetupError,
};

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default)]
    pub struct FailedSession {
        /// The error encountered when initializing the session.
        error: OnceCell<ClientSetupError>,
        /// The data for the avatar representation for this session.
        avatar_data: OnceCell<AvatarData>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FailedSession {
        const NAME: &'static str = "FailedSession";
        type Type = super::FailedSession;
        type ParentType = SessionInfo;
    }

    impl ObjectImpl for FailedSession {}

    impl SessionInfoImpl for FailedSession {
        fn avatar_data(&self) -> AvatarData {
            self.avatar_data
                .get_or_init(|| {
                    let avatar_data = AvatarData::new();
                    avatar_data.set_display_name(self.obj().user_id().to_string());
                    avatar_data
                })
                .clone()
        }
    }

    impl FailedSession {
        /// Set the error encountered when initializing the session.
        pub(super) fn set_error(&self, error: ClientSetupError) {
            self.error
                .set(error)
                .expect("error should not be initialized");
        }

        /// The error encountered when initializing the session.
        pub(super) fn error(&self) -> &ClientSetupError {
            self.error.get().expect("error should be initialized")
        }
    }
}

glib::wrapper! {
    /// A Matrix user session that encountered an error when initializing the client.
    pub struct FailedSession(ObjectSubclass<imp::FailedSession>)
        @extends SessionInfo;
}

impl FailedSession {
    /// Constructs a new `FailedSession` with the given info and error.
    pub(crate) fn new(stored_session: &StoredSession, error: ClientSetupError) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("info", stored_session)
            .build();
        obj.imp().set_error(error);
        obj
    }

    /// The error encountered when initializing the session.
    pub(crate) fn error(&self) -> &ClientSetupError {
        self.imp().error()
    }
}
