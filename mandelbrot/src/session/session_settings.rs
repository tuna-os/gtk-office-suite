use std::collections::BTreeSet;

use gtk::{glib, prelude::*, subclass::prelude::*};
use indexmap::IndexSet;
use ruma::{OwnedRoomId, OwnedServerName, events::media_preview_config::MediaPreviews};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::SidebarSectionName;
use crate::{Application, session_list::SessionListSettings};

/// The current version of the stored session settings.
const CURRENT_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct StoredSessionSettings {
    /// The version of the stored settings.
    #[serde(default)]
    pub(super) version: u8,

    /// Custom servers to explore.
    #[serde(default, skip_serializing_if = "IndexSet::is_empty")]
    explore_custom_servers: IndexSet<OwnedServerName>,

    /// Whether notifications are enabled for this session.
    #[serde(
        default = "ruma::serde::default_true",
        skip_serializing_if = "ruma::serde::is_true"
    )]
    notifications_enabled: bool,

    /// Whether public read receipts are enabled for this session.
    #[serde(
        default = "ruma::serde::default_true",
        skip_serializing_if = "ruma::serde::is_true"
    )]
    public_read_receipts_enabled: bool,

    /// Whether typing notifications are enabled for this session.
    #[serde(
        default = "ruma::serde::default_true",
        skip_serializing_if = "ruma::serde::is_true"
    )]
    typing_enabled: bool,

    /// The sections that are expanded.
    #[serde(default)]
    sections_expanded: SectionsExpanded,

    /// The navigation path of the space selected in the sidebar, from a
    /// top-level space to the selected (sub)space.
    ///
    /// Empty when "Home" is selected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    space_path: Vec<OwnedRoomId>,

    /// Which rooms display media previews for this session.
    ///
    /// Legacy setting from version 0 of the stored settings.
    #[serde(skip_serializing)]
    pub(super) media_previews_enabled: Option<MediaPreviewsSetting>,

    /// Whether to display avatars in invites.
    ///
    /// Legacy setting from version 0 of the stored settings.
    #[serde(skip_serializing)]
    pub(super) invite_avatars_enabled: Option<bool>,
}

impl Default for StoredSessionSettings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            explore_custom_servers: Default::default(),
            notifications_enabled: true,
            public_read_receipts_enabled: true,
            typing_enabled: true,
            sections_expanded: Default::default(),
            space_path: Default::default(),
            media_previews_enabled: Default::default(),
            invite_avatars_enabled: Default::default(),
        }
    }
}

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SessionSettings)]
    pub struct SessionSettings {
        /// The ID of the session these settings are for.
        #[property(get, construct_only)]
        session_id: OnceCell<String>,
        /// The stored settings.
        pub(super) stored_settings: RefCell<StoredSessionSettings>,
        /// Whether notifications are enabled for this session.
        #[property(get = Self::notifications_enabled, set = Self::set_notifications_enabled, explicit_notify, default = true)]
        notifications_enabled: PhantomData<bool>,
        /// Whether public read receipts are enabled for this session.
        #[property(get = Self::public_read_receipts_enabled, set = Self::set_public_read_receipts_enabled, explicit_notify, default = true)]
        public_read_receipts_enabled: PhantomData<bool>,
        /// Whether typing notifications are enabled for this session.
        #[property(get = Self::typing_enabled, set = Self::set_typing_enabled, explicit_notify, default = true)]
        typing_enabled: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionSettings {
        const NAME: &'static str = "SessionSettings";
        type Type = super::SessionSettings;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SessionSettings {}

    impl SessionSettings {
        /// Whether notifications are enabled for this session.
        fn notifications_enabled(&self) -> bool {
            self.stored_settings.borrow().notifications_enabled
        }

        /// Set whether notifications are enabled for this session.
        fn set_notifications_enabled(&self, enabled: bool) {
            if self.notifications_enabled() == enabled {
                return;
            }

            self.stored_settings.borrow_mut().notifications_enabled = enabled;
            session_list_settings().save();
            self.obj().notify_notifications_enabled();
        }

        /// Whether public read receipts are enabled for this session.
        fn public_read_receipts_enabled(&self) -> bool {
            self.stored_settings.borrow().public_read_receipts_enabled
        }

        /// Set whether public read receipts are enabled for this session.
        fn set_public_read_receipts_enabled(&self, enabled: bool) {
            if self.public_read_receipts_enabled() == enabled {
                return;
            }

            self.stored_settings
                .borrow_mut()
                .public_read_receipts_enabled = enabled;
            session_list_settings().save();
            self.obj().notify_public_read_receipts_enabled();
        }

        /// Whether typing notifications are enabled for this session.
        fn typing_enabled(&self) -> bool {
            self.stored_settings.borrow().typing_enabled
        }

        /// Set whether typing notifications are enabled for this session.
        fn set_typing_enabled(&self, enabled: bool) {
            if self.typing_enabled() == enabled {
                return;
            }

            self.stored_settings.borrow_mut().typing_enabled = enabled;
            session_list_settings().save();
            self.obj().notify_typing_enabled();
        }

        /// Apply the migration of the stored settings from version 0 to version
        /// 1.
        pub(crate) fn apply_version_1_migration(&self) {
            {
                let mut stored_settings = self.stored_settings.borrow_mut();

                if stored_settings.version > 0 {
                    return;
                }

                info!(
                    session = self.obj().session_id(),
                    "Migrating store session to version 1"
                );

                stored_settings.media_previews_enabled.take();
                stored_settings.invite_avatars_enabled.take();
                stored_settings.version = 1;
            }

            session_list_settings().save();
        }
    }
}

glib::wrapper! {
    /// The settings of a [`Session`](super::Session).
    pub struct SessionSettings(ObjectSubclass<imp::SessionSettings>);
}

impl SessionSettings {
    /// Create a new `SessionSettings` for the given session ID.
    pub(crate) fn new(session_id: &str) -> Self {
        glib::Object::builder()
            .property("session-id", session_id)
            .build()
    }

    /// Restore existing `SessionSettings` with the given session ID and stored
    /// settings.
    pub(crate) fn restore(session_id: &str, stored_settings: StoredSessionSettings) -> Self {
        let obj = Self::new(session_id);
        *obj.imp().stored_settings.borrow_mut() = stored_settings;
        obj
    }

    /// The stored settings.
    pub(crate) fn stored_settings(&self) -> StoredSessionSettings {
        self.imp().stored_settings.borrow().clone()
    }

    /// Apply the migration of the stored settings from version 0 to version 1.
    pub(crate) fn apply_version_1_migration(&self) {
        self.imp().apply_version_1_migration();
    }

    /// Delete the settings from the application settings.
    pub(crate) fn delete(&self) {
        session_list_settings().remove(&self.session_id());
    }

    /// Custom servers to explore.
    pub(crate) fn explore_custom_servers(&self) -> IndexSet<OwnedServerName> {
        self.imp()
            .stored_settings
            .borrow()
            .explore_custom_servers
            .clone()
    }

    /// Set the custom servers to explore.
    pub(crate) fn set_explore_custom_servers(&self, servers: IndexSet<OwnedServerName>) {
        if self.explore_custom_servers() == servers {
            return;
        }

        self.imp()
            .stored_settings
            .borrow_mut()
            .explore_custom_servers = servers;
        session_list_settings().save();
    }

    /// Whether the section with the given name is expanded.
    pub(crate) fn is_section_expanded(&self, section_name: SidebarSectionName) -> bool {
        self.imp()
            .stored_settings
            .borrow()
            .sections_expanded
            .is_section_expanded(section_name)
    }

    /// Set whether the section with the given name is expanded.
    pub(crate) fn set_section_expanded(&self, section_name: SidebarSectionName, expanded: bool) {
        self.imp()
            .stored_settings
            .borrow_mut()
            .sections_expanded
            .set_section_expanded(section_name, expanded);
        session_list_settings().save();
    }

    /// The navigation path of the space selected in the sidebar.
    pub(crate) fn space_path(&self) -> Vec<OwnedRoomId> {
        self.imp().stored_settings.borrow().space_path.clone()
    }

    /// Set the navigation path of the space selected in the sidebar.
    pub(crate) fn set_space_path(&self, path: Vec<OwnedRoomId>) {
        if self.imp().stored_settings.borrow().space_path == path {
            return;
        }

        self.imp().stored_settings.borrow_mut().space_path = path;
        session_list_settings().save();
    }
}

/// The sections that are expanded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct SectionsExpanded(BTreeSet<SidebarSectionName>);

impl SectionsExpanded {
    /// Whether the section with the given name is expanded.
    pub(crate) fn is_section_expanded(&self, section_name: SidebarSectionName) -> bool {
        self.0.contains(&section_name)
    }

    /// Set whether the section with the given name is expanded.
    pub(crate) fn set_section_expanded(
        &mut self,
        section_name: SidebarSectionName,
        expanded: bool,
    ) {
        if expanded {
            self.0.insert(section_name);
        } else {
            self.0.remove(&section_name);
        }
    }
}

impl Default for SectionsExpanded {
    fn default() -> Self {
        Self(BTreeSet::from([
            SidebarSectionName::VerificationRequest,
            SidebarSectionName::InviteRequest,
            SidebarSectionName::Invited,
            SidebarSectionName::Space,
            SidebarSectionName::Favorite,
            SidebarSectionName::Normal,
            SidebarSectionName::LowPriority,
        ]))
    }
}

/// Setting about which rooms display media previews.
///
/// Legacy setting from version 0 of the stored settings.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct MediaPreviewsSetting {
    /// The default setting for all rooms.
    #[serde(default)]
    pub(super) global: MediaPreviewsGlobalSetting,
}

/// Possible values of the global setting about which rooms display media
/// previews.
///
/// Legacy setting from version 0 of the stored settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum MediaPreviewsGlobalSetting {
    /// All rooms show media previews.
    #[default]
    All,
    /// Only private rooms show media previews.
    Private,
    /// No rooms show media previews.
    None,
}

impl From<MediaPreviewsGlobalSetting> for MediaPreviews {
    fn from(value: MediaPreviewsGlobalSetting) -> Self {
        match value {
            MediaPreviewsGlobalSetting::All => Self::On,
            MediaPreviewsGlobalSetting::Private => Self::Private,
            MediaPreviewsGlobalSetting::None => Self::Off,
        }
    }
}

/// The session list settings of the application.
fn session_list_settings() -> SessionListSettings {
    Application::default().session_list().settings()
}
