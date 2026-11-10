use futures_util::StreamExt;
use gtk::{
    glib,
    glib::{clone, closure_local},
    prelude::*,
    subclass::prelude::*,
};
use ruma::events::media_preview_config::{
    InviteAvatars, MediaPreviewConfigEventContent, MediaPreviews,
};
use tokio::task::AbortHandle;
use tracing::error;

use super::{Room, Session};
use crate::{session::JoinRuleValue, spawn, spawn_tokio};

/// We default the media previews setting to on.
const DEFAULT_MEDIA_PREVIEWS: MediaPreviews = MediaPreviews::On;
/// We enable the invite avatars by default.
const DEFAULT_INVITE_AVATARS_ENABLED: bool = true;

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, glib::Properties)]
    #[properties(wrapper_type = super::GlobalAccountData)]
    pub struct GlobalAccountData {
        /// The session this account data belongs to.
        #[property(get, construct_only)]
        session: OnceCell<Session>,
        /// Which rooms display media previews for this session.
        pub(super) media_previews_enabled: RefCell<MediaPreviews>,
        /// Whether to display avatars in invites.
        #[property(get, default = DEFAULT_INVITE_AVATARS_ENABLED)]
        invite_avatars_enabled: Cell<bool>,
        abort_handle: RefCell<Option<AbortHandle>>,
    }

    impl Default for GlobalAccountData {
        fn default() -> Self {
            Self {
                session: Default::default(),
                media_previews_enabled: RefCell::new(DEFAULT_MEDIA_PREVIEWS),
                invite_avatars_enabled: Cell::new(DEFAULT_INVITE_AVATARS_ENABLED),
                abort_handle: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GlobalAccountData {
        const NAME: &'static str = "GlobalAccountData";
        type Type = super::GlobalAccountData;
    }

    #[glib::derived_properties]
    impl ObjectImpl for GlobalAccountData {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("media-previews-enabled-changed").build()]);
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.init_media_previews_settings().await;
                    imp.apply_migrations().await;
                }
            ));
        }

        fn dispose(&self) {
            if let Some(handle) = self.abort_handle.take() {
                handle.abort();
            }
        }
    }

    impl GlobalAccountData {
        /// The session these settings are for.
        fn session(&self) -> &Session {
            self.session.get().expect("session should be initialized")
        }

        /// Initialize the media previews settings from the account data and
        /// watch for changes.
        pub(super) async fn init_media_previews_settings(&self) {
            let client = self.session().client();
            let handle =
                spawn_tokio!(async move { client.account().observe_media_preview_config().await });

            let (account_data, stream) = match handle.await.expect("task was not aborted") {
                Ok((account_data, stream)) => (account_data, stream),
                Err(error) => {
                    error!("Could not initialize media preview settings: {error}");
                    return;
                }
            };

            self.update_media_previews_settings(account_data.unwrap_or_default());

            let obj_weak = glib::SendWeakRef::from(self.obj().downgrade());
            let fut = stream.for_each(move |account_data| {
                let obj_weak = obj_weak.clone();
                async move {
                    let ctx = glib::MainContext::default();
                    ctx.spawn(async move {
                        spawn!(async move {
                            if let Some(obj) = obj_weak.upgrade() {
                                obj.imp().update_media_previews_settings(account_data);
                            }
                        });
                    });
                }
            });

            let abort_handle = spawn_tokio!(fut).abort_handle();
            self.abort_handle.replace(Some(abort_handle));
        }

        /// Update the media previews settings with the given account data.
        fn update_media_previews_settings(&self, account_data: MediaPreviewConfigEventContent) {
            let media_previews = account_data
                .media_previews
                .unwrap_or(DEFAULT_MEDIA_PREVIEWS);
            let media_previews_enabled_changed =
                *self.media_previews_enabled.borrow() != media_previews;
            if media_previews_enabled_changed {
                *self.media_previews_enabled.borrow_mut() = media_previews;
                self.obj()
                    .emit_by_name::<()>("media-previews-enabled-changed", &[]);
            }

            let invite_avatars_enabled = account_data
                .invite_avatars
                .map_or(DEFAULT_INVITE_AVATARS_ENABLED, |invite_avatars| {
                    invite_avatars == InviteAvatars::On
                });
            let invite_avatars_enabled_changed =
                self.invite_avatars_enabled.get() != invite_avatars_enabled;
            if invite_avatars_enabled_changed {
                self.invite_avatars_enabled.set(invite_avatars_enabled);
                self.obj().notify_invite_avatars_enabled();
            }
        }

        /// Apply any necessary migrations.
        pub(super) async fn apply_migrations(&self) {
            let session_settings = self.session().settings();
            let mut stored_settings = session_settings.stored_settings();

            if stored_settings.version != 0 {
                // No migration to apply.
                return;
            }

            // Align the account data with the stored settings.
            let stored_media_previews_enabled = stored_settings
                .media_previews_enabled
                .take()
                .map_or(DEFAULT_MEDIA_PREVIEWS, |setting| setting.global.into());
            let _ = self
                .set_media_previews_enabled(stored_media_previews_enabled)
                .await;

            let stored_invite_avatars_enabled = stored_settings
                .invite_avatars_enabled
                .take()
                .unwrap_or(DEFAULT_INVITE_AVATARS_ENABLED);
            let _ = self
                .set_invite_avatars_enabled(stored_invite_avatars_enabled)
                .await;

            session_settings.apply_version_1_migration();
        }

        /// Set which rooms display media previews.
        pub(super) async fn set_media_previews_enabled(
            &self,
            setting: MediaPreviews,
        ) -> Result<(), ()> {
            if *self.media_previews_enabled.borrow() == setting {
                return Ok(());
            }

            let client = self.session().client();
            let setting_clone = setting.clone();
            let handle = spawn_tokio!(async move {
                client
                    .account()
                    .set_media_previews_display_policy(setting_clone)
                    .await
            });

            if let Err(error) = handle.await.expect("task was not aborted") {
                error!("Could not change media previews enabled setting: {error}");
                return Err(());
            }

            self.media_previews_enabled.replace(setting);

            self.obj()
                .emit_by_name::<()>("media-previews-enabled-changed", &[]);

            Ok(())
        }

        /// Set whether to display avatars in invites.
        pub(super) async fn set_invite_avatars_enabled(&self, enabled: bool) -> Result<(), ()> {
            if self.invite_avatars_enabled.get() == enabled {
                return Ok(());
            }

            let client = self.session().client();
            let setting = if enabled {
                InviteAvatars::On
            } else {
                InviteAvatars::Off
            };
            let handle = spawn_tokio!(async move {
                client
                    .account()
                    .set_invite_avatars_display_policy(setting)
                    .await
            });

            if let Err(error) = handle.await.expect("task was not aborted") {
                error!("Could not change invite avatars enabled setting: {error}");
                return Err(());
            }

            self.invite_avatars_enabled.set(enabled);
            self.obj().notify_invite_avatars_enabled();

            Ok(())
        }
    }
}

glib::wrapper! {
    /// The settings in the global account data of a [`Session`].
    pub struct GlobalAccountData(ObjectSubclass<imp::GlobalAccountData>);
}

impl GlobalAccountData {
    /// Create a new `GlobalAccountData` for the given session.
    pub(crate) fn new(session: &Session) -> Self {
        glib::Object::builder::<Self>()
            .property("session", session)
            .build()
    }

    /// Which rooms display media previews.
    pub(crate) fn media_previews_enabled(&self) -> MediaPreviews {
        self.imp().media_previews_enabled.borrow().clone()
    }

    /// Whether the given room should display media previews.
    pub(crate) fn should_room_show_media_previews(&self, room: &Room) -> bool {
        match &*self.imp().media_previews_enabled.borrow() {
            MediaPreviews::Off => false,
            MediaPreviews::Private => matches!(
                room.join_rule().value(),
                JoinRuleValue::Invite | JoinRuleValue::RoomMembership
            ),
            _ => true,
        }
    }

    /// Set which rooms display media previews.
    pub(crate) async fn set_media_previews_enabled(
        &self,
        setting: MediaPreviews,
    ) -> Result<(), ()> {
        self.imp().set_media_previews_enabled(setting).await
    }

    /// Set whether to display avatars in invites.
    pub(crate) async fn set_invite_avatars_enabled(&self, enabled: bool) -> Result<(), ()> {
        self.imp().set_invite_avatars_enabled(enabled).await
    }

    /// Connect to the signal emitted when the media previews setting changed.
    pub fn connect_media_previews_enabled_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "media-previews-enabled-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
