use gtk::{glib, prelude::*, subclass::prelude::*};
use matrix_sdk::ruma::{OwnedMxcUri, OwnedUserId};

use crate::{
    components::PillSource,
    prelude::*,
    session::{Room, Session, User},
};

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::DirectChatUser)]
    pub struct DirectChatUser {
        /// The direct chat with this user, if any.
        #[property(get, set = Self::set_direct_chat, explicit_notify, nullable)]
        direct_chat: glib::WeakRef<Room>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DirectChatUser {
        const NAME: &'static str = "DirectChatUser";
        type Type = super::DirectChatUser;
        type ParentType = User;
    }

    #[glib::derived_properties]
    impl ObjectImpl for DirectChatUser {}

    impl PillSourceImpl for DirectChatUser {
        fn identifier(&self) -> String {
            self.obj().upcast_ref::<User>().user_id_string()
        }
    }

    impl DirectChatUser {
        /// Set the direct chat with this user.
        fn set_direct_chat(&self, direct_chat: Option<&Room>) {
            if self.direct_chat.upgrade().as_ref() == direct_chat {
                return;
            }

            self.direct_chat.set(direct_chat);
            self.obj().notify_direct_chat();
        }
    }
}

glib::wrapper! {
    /// A User in the context of creating a direct chat.
    pub struct DirectChatUser(ObjectSubclass<imp::DirectChatUser>)
        @extends PillSource, User;
}

impl DirectChatUser {
    pub fn new(
        session: &Session,
        user_id: OwnedUserId,
        display_name: Option<&str>,
        avatar_url: Option<OwnedMxcUri>,
    ) -> Self {
        let display_name = display_name.unwrap_or_else(|| user_id.localpart());

        let obj: Self = glib::Object::builder()
            .property("session", session)
            .property("display-name", display_name)
            .build();

        let user = obj.upcast_ref::<User>();
        user.set_avatar_url(avatar_url);
        user.imp().set_user_id(user_id);
        obj.set_direct_chat(user.direct_chat());

        obj
    }
}
