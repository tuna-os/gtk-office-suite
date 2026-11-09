use gtk::{glib, glib::clone, prelude::*, subclass::prelude::*};
use matrix_sdk::encryption::identities::UserIdentity;
use ruma::{
    MatrixToUri, OwnedMxcUri, OwnedUserId,
    api::client::profile::{AvatarUrl, DisplayName},
};
use tracing::{debug, error};

use super::{IdentityVerification, Room, Session};
use crate::{
    components::{AvatarImage, AvatarUriSource, PillSource},
    prelude::*,
    spawn, spawn_tokio,
};

#[glib::flags(name = "UserActions")]
pub enum UserActions {
    VERIFY = 0b0000_0001,
}

impl Default for UserActions {
    fn default() -> Self {
        Self::empty()
    }
}

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::User)]
    pub struct User {
        /// The ID of this user.
        user_id: OnceCell<OwnedUserId>,
        /// The ID of this user, as a string.
        #[property(get = Self::user_id_string)]
        user_id_string: PhantomData<String>,
        /// The current session.
        #[property(get, construct_only)]
        session: OnceCell<Session>,
        /// Whether this user is the same as the session's user.
        #[property(get)]
        is_own_user: Cell<bool>,
        /// Whether this user has a display name set.
        ///
        /// If the display name is not set, the `display-name` property returns
        /// the localpart of the user ID.
        #[property(get)]
        pub(super) has_display_name: Cell<bool>,
        /// Whether this user has been verified.
        #[property(get)]
        is_verified: Cell<bool>,
        /// Whether this user is currently ignored.
        #[property(get)]
        is_ignored: Cell<bool>,
        ignored_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for User {
        const NAME: &'static str = "User";
        type Type = super::User;
        type ParentType = PillSource;
    }

    #[glib::derived_properties]
    impl ObjectImpl for User {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let avatar_image = AvatarImage::new(&obj.session(), AvatarUriSource::User, None, None);
            obj.avatar_data().set_image(Some(avatar_image));
        }

        fn dispose(&self) {
            if let Some(session) = self.session.get()
                && let Some(handler) = self.ignored_handler.take()
            {
                session.ignored_users().disconnect(handler);
            }
        }
    }

    impl PillSourceImpl for User {
        fn identifier(&self) -> String {
            self.user_id_string()
        }
    }

    impl User {
        /// The ID of this user.
        pub(super) fn user_id(&self) -> &OwnedUserId {
            self.user_id.get().expect("user ID should be initialized")
        }

        /// The ID of this user, as a string.
        fn user_id_string(&self) -> String {
            self.user_id().to_string()
        }

        /// The current session.
        fn session(&self) -> &Session {
            self.session.get().expect("session should be initialized")
        }

        /// Set the ID of this user.
        pub(crate) fn set_user_id(&self, user_id: OwnedUserId) {
            let user_id = self.user_id.get_or_init(|| user_id);

            let obj = self.obj();
            obj.set_name(None);
            obj.bind_property("display-name", &obj.avatar_data(), "display-name")
                .sync_create()
                .build();

            let session = self.session();
            self.is_own_user.set(session.user_id() == user_id);

            let ignored_users = session.ignored_users();
            let ignored_handler = ignored_users.connect_items_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |ignored_users, _, _, _| {
                    let user_id = imp.user_id.get().expect("user ID is initialized");
                    let is_ignored = ignored_users.contains(user_id);

                    if imp.is_ignored.get() != is_ignored {
                        imp.is_ignored.set(is_ignored);
                        imp.obj().notify_is_ignored();
                    }
                }
            ));
            self.is_ignored.set(ignored_users.contains(user_id));
            self.ignored_handler.replace(Some(ignored_handler));

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.init_is_verified().await;
                }
            ));
        }

        /// Set whether this user has a display name set.
        pub(super) fn set_has_display_name(&self, has_display_name: bool) {
            if self.has_display_name.get() == has_display_name {
                return;
            }

            self.has_display_name.set(has_display_name);
            self.obj().notify_has_display_name();
        }

        /// Get the local cryptographic identity (aka cross-signing identity) of
        /// this user.
        ///
        /// Locally, we should always have the crypto identity of our own user
        /// and of users with whom we share an encrypted room.
        pub(super) async fn local_crypto_identity(&self) -> Option<UserIdentity> {
            let encryption = self.session().client().encryption();
            let user_id = self.user_id().clone();
            let handle = spawn_tokio!(async move { encryption.get_user_identity(&user_id).await });

            match handle.await.expect("task was not aborted") {
                Ok(identity) => identity,
                Err(error) => {
                    error!("Could not get local crypto identity: {error}");
                    None
                }
            }
        }

        /// Load whether this user is verified.
        async fn init_is_verified(&self) {
            // If a user is verified, we should have their crypto identity locally.
            let is_verified = self
                .local_crypto_identity()
                .await
                .is_some_and(|i| i.is_verified());

            if self.is_verified.get() == is_verified {
                return;
            }

            self.is_verified.set(is_verified);
            self.obj().notify_is_verified();
        }

        /// Create an encrypted direct chat with this user.
        pub(super) async fn create_direct_chat(&self) -> Result<Room, matrix_sdk::Error> {
            let user_id = self.user_id().clone();
            let client = self.session().client();
            let handle = spawn_tokio!(async move { client.create_dm(&user_id).await });

            match handle.await.expect("task was not aborted") {
                Ok(matrix_room) => {
                    let room = self
                        .session()
                        .room_list()
                        .get_wait(matrix_room.room_id(), None)
                        .await
                        .expect("The newly created room was not found");
                    Ok(room)
                }
                Err(error) => {
                    error!("Could not create direct chat: {error}");
                    Err(error)
                }
            }
        }
    }
}

glib::wrapper! {
    /// `glib::Object` representation of a Matrix user.
    pub struct User(ObjectSubclass<imp::User>) @extends PillSource;
}

impl User {
    /// Constructs a new user with the given user ID for the given session.
    pub fn new(session: &Session, user_id: OwnedUserId) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("session", session)
            .build();

        obj.imp().set_user_id(user_id);
        obj
    }

    /// Get the cryptographic identity (aka cross-signing identity) of this
    /// user.
    ///
    /// First, we try to get the local crypto identity if we are sure that it is
    /// up-to-date. If we do not have the crypto identity locally, we request it
    /// from the homeserver.
    pub(crate) async fn ensure_crypto_identity(&self) -> Option<UserIdentity> {
        let session = self.session();
        let encryption = session.client().encryption();
        let user_id = self.user_id();

        // First, see if we should have an updated crypto identity for the user locally.
        // When we get the remote crypto identity of a user manually, it is cached
        // locally but it is not kept up-to-date unless the user is tracked. That's why
        // it's important to only use the local crypto identity if the user is tracked.
        let should_have_local = if user_id == session.user_id() {
            true
        } else {
            // We should have the updated user identity locally for tracked users.
            let encryption_clone = encryption.clone();
            let handle = spawn_tokio!(async move { encryption_clone.tracked_users().await });

            match handle.await.expect("task was not aborted") {
                Ok(tracked_users) => tracked_users.contains(user_id),
                Err(error) => {
                    error!("Could not get tracked users: {error}");
                    // We are not sure, but let us try to get the local user identity first.
                    true
                }
            }
        };

        // Try to get the local crypto identity.
        if should_have_local && let Some(identity) = self.imp().local_crypto_identity().await {
            return Some(identity);
        }

        // Now, try to request the crypto identity from the homeserver.
        let user_id_clone = user_id.clone();
        let handle =
            spawn_tokio!(async move { encryption.request_user_identity(&user_id_clone).await });

        match handle.await.expect("task was not aborted") {
            Ok(identity) => identity,
            Err(error) => {
                error!("Could not request remote crypto identity: {error}");
                None
            }
        }
    }

    /// Start a verification of the identity of this user.
    pub(crate) async fn verify_identity(&self) -> Result<IdentityVerification, ()> {
        self.session()
            .verification_list()
            .create(Some(self.clone()))
            .await
    }

    /// The existing direct chat with this user, if any.
    ///
    /// A direct chat is a joined room marked as direct, with only our own user
    /// and the other user in it.
    pub(crate) fn direct_chat(&self) -> Option<Room> {
        self.session().room_list().direct_chat(self.user_id())
    }

    /// Get or create a direct chat with this user.
    ///
    /// If there is no existing direct chat, a new one is created.
    pub(crate) async fn get_or_create_direct_chat(&self) -> Result<Room, ()> {
        let user_id = self.user_id();

        if let Some(room) = self.direct_chat() {
            debug!("Using existing direct chat with {user_id}…");
            return Ok(room);
        }

        debug!("Creating direct chat with {user_id}…");
        self.imp().create_direct_chat().await.map_err(|_| ())
    }

    /// Ignore this user.
    pub(crate) async fn ignore(&self) -> Result<(), ()> {
        self.session().ignored_users().add(self.user_id()).await
    }

    /// Stop ignoring this user.
    pub(crate) async fn stop_ignoring(&self) -> Result<(), ()> {
        self.session().ignored_users().remove(self.user_id()).await
    }
}

pub trait UserExt: IsA<User> {
    /// The current session.
    fn session(&self) -> Session {
        self.upcast_ref().session()
    }

    /// The ID of this user.
    fn user_id(&self) -> &OwnedUserId {
        self.upcast_ref().imp().user_id()
    }

    /// Whether this user is the same as the session's user.
    fn is_own_user(&self) -> bool {
        self.upcast_ref().is_own_user()
    }

    /// Set the name of this user.
    fn set_name(&self, name: Option<String>) {
        let user = self.upcast_ref();
        let name = name.into_clean_string();

        user.imp().set_has_display_name(name.is_some());

        let display_name = name.unwrap_or_else(|| user.user_id().localpart().to_owned());
        user.set_display_name(display_name);
    }

    /// Whether this user has a display name set.
    ///
    /// If the display name is not set, the `display-name` property returns the
    /// localpart of the user ID.
    fn has_display_name(&self) -> bool {
        self.upcast_ref().imp().has_display_name.get()
    }

    /// Set the avatar URL of this user.
    fn set_avatar_url(&self, uri: Option<OwnedMxcUri>) {
        self.upcast_ref()
            .avatar_data()
            .image()
            .expect("avatar data should have an image")
            // User avatars never have information.
            .set_uri_and_info(uri, None);
    }

    /// Get the `matrix.to` URI representation for this `User`.
    fn matrix_to_uri(&self) -> MatrixToUri {
        self.user_id().matrix_to_uri()
    }

    /// Load the user profile from the homeserver.
    ///
    /// This overwrites the already loaded display name and avatar.
    async fn load_profile(&self) -> Result<(), ()> {
        let user_id = self.user_id();

        let client = self.session().client();
        let user_id_clone = user_id.clone();
        let handle =
            spawn_tokio!(
                async move { client.account().fetch_user_profile_of(&user_id_clone).await }
            );

        match handle.await.expect("task was not aborted") {
            Ok(response) => {
                let user = self.upcast_ref::<User>();

                match response.get_static::<DisplayName>() {
                    Ok(display_name) => user.set_name(display_name),
                    Err(error) => {
                        error!(%user_id, "Could not deserialize user display name: {error}");
                    }
                }

                match response.get_static::<AvatarUrl>() {
                    Ok(avatar_url) => user.set_avatar_url(avatar_url),
                    Err(error) => {
                        error!(%user_id, "Could not deserialize user avatar URL: {error}");
                    }
                }

                Ok(())
            }
            Err(error) => {
                error!(%user_id, "Could not load user profile: {error}");
                Err(())
            }
        }
    }

    /// Whether this user is currently ignored.
    fn is_ignored(&self) -> bool {
        self.upcast_ref().is_ignored()
    }

    /// Connect to the signal emitted when the `is-ignored` property changes.
    fn connect_is_ignored_notify<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.upcast_ref().connect_is_ignored_notify(move |user| {
            f(user
                .downcast_ref()
                .expect("downcasting to own type should succeed"));
        })
    }
}

impl<T: IsA<PillSource> + IsA<User>> UserExt for T {}

unsafe impl<T> IsSubclassable<T> for User
where
    T: PillSourceImpl,
    T::Type: IsA<PillSource>,
{
    fn class_init(class: &mut glib::Class<Self>) {
        <glib::Object as IsSubclassable<T>>::class_init(class.upcast_ref_mut());
    }

    fn instance_init(instance: &mut glib::subclass::InitializingObject<T>) {
        <glib::Object as IsSubclassable<T>>::instance_init(instance);
    }
}
