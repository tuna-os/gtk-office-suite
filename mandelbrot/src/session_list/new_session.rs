use gtk::{glib, subclass::prelude::*};

use super::{SessionInfo, SessionInfoImpl};
use crate::{components::AvatarData, prelude::*, secret::StoredSession};

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default)]
    pub struct NewSession {
        /// The data for the avatar representation for this session.
        avatar_data: OnceCell<AvatarData>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for NewSession {
        const NAME: &'static str = "NewSession";
        type Type = super::NewSession;
        type ParentType = SessionInfo;
    }

    impl ObjectImpl for NewSession {}

    impl SessionInfoImpl for NewSession {
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
}

glib::wrapper! {
    /// A brand new Matrix user session that is not constructed yet.
    ///
    /// This is just a wrapper around [`StoredSession`].
    pub struct NewSession(ObjectSubclass<imp::NewSession>)
        @extends SessionInfo;
}

impl NewSession {
    /// Constructs a new `NewSession` with the given info.
    pub fn new(stored_session: &StoredSession) -> Self {
        glib::Object::builder()
            .property("info", stored_session)
            .build()
    }
}
