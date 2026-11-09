use std::fmt;

use gettextrs::gettext;
use gtk::glib;
use serde::{Deserialize, Serialize};

use crate::session::{RoomCategory, TargetRoomCategory};

/// The possible names of the sections in the sidebar.
#[derive(
    Debug, Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, glib::Enum, Serialize, Deserialize,
)]
#[enum_type(name = "SidebarSectionName")]
#[serde(rename_all = "kebab-case")]
pub enum SidebarSectionName {
    /// The section for verification requests.
    VerificationRequest,
    /// The section for invite requests.
    InviteRequest,
    /// The section for room invites.
    Invited,
    /// The section for joined spaces.
    Space,
    /// The section for favorite rooms.
    Favorite,
    /// The section for joined rooms without a tag.
    #[default]
    Normal,
    /// The section for low-priority rooms.
    LowPriority,
    /// The section for room that were left.
    Left,
}

impl SidebarSectionName {
    /// Convert the given `RoomCategory` to a `SidebarSectionName`, if possible.
    pub(crate) fn from_room_category(category: RoomCategory) -> Option<Self> {
        let name = match category {
            RoomCategory::Knocked => Self::InviteRequest,
            RoomCategory::Invited => Self::Invited,
            RoomCategory::Space => Self::Space,
            RoomCategory::Favorite => Self::Favorite,
            RoomCategory::Normal => Self::Normal,
            RoomCategory::LowPriority => Self::LowPriority,
            RoomCategory::Left => Self::Left,
            RoomCategory::Outdated | RoomCategory::Ignored => return None,
        };

        Some(name)
    }

    /// Convert this `SidebarSectionName` to a `RoomCategory`, if possible.
    pub(crate) fn into_room_category(self) -> Option<RoomCategory> {
        let category = match self {
            Self::VerificationRequest => return None,
            Self::InviteRequest => RoomCategory::Knocked,
            Self::Invited => RoomCategory::Invited,
            Self::Space => RoomCategory::Space,
            Self::Favorite => RoomCategory::Favorite,
            Self::Normal => RoomCategory::Normal,
            Self::LowPriority => RoomCategory::LowPriority,
            Self::Left => RoomCategory::Left,
        };

        Some(category)
    }

    /// Convert this `SidebarSectionName` to a `TargetRoomCategory`, if
    /// possible.
    pub(crate) fn into_target_room_category(self) -> Option<TargetRoomCategory> {
        let category = match self {
            Self::VerificationRequest | Self::InviteRequest | Self::Invited | Self::Space => {
                return None;
            }
            Self::Favorite => TargetRoomCategory::Favorite,
            Self::Normal => TargetRoomCategory::Normal,
            Self::LowPriority => TargetRoomCategory::LowPriority,
            Self::Left => TargetRoomCategory::Left,
        };

        Some(category)
    }
}

impl fmt::Display for SidebarSectionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            SidebarSectionName::VerificationRequest => gettext("Verifications"),
            SidebarSectionName::InviteRequest => gettext("Invite Requests"),
            SidebarSectionName::Invited => gettext("Invited"),
            SidebarSectionName::Space => gettext("Spaces"),
            SidebarSectionName::Favorite => gettext("Favorites"),
            SidebarSectionName::Normal => gettext("Rooms"),
            SidebarSectionName::LowPriority => gettext("Low Priority"),
            SidebarSectionName::Left => gettext("Historical"),
        };
        f.write_str(&label)
    }
}
