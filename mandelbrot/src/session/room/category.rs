use std::fmt;

use gtk::glib;
use matrix_sdk::RoomState;

use crate::session::SidebarSectionName;

/// The category of a room.
#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "RoomCategory")]
pub enum RoomCategory {
    /// The user requested an invite to the room.
    Knocked,
    /// The user was invited to the room.
    Invited,
    /// The room is joined and has the `m.favourite` tag.
    Favorite,
    /// The room is joined and has no known tag.
    #[default]
    Normal,
    /// The room is joined and has the `m.lowpriority` tag.
    LowPriority,
    /// The room was left by the user, or they were kicked or banned.
    Left,
    /// The room was upgraded and their successor was joined.
    Outdated,
    /// The room is a space.
    Space,
    /// The room should be ignored.
    ///
    /// According to the Matrix specification, invites from ignored users
    /// should be ignored.
    Ignored,
}

impl RoomCategory {
    /// Check whether this `RoomCategory` can be changed to the given target
    /// category.
    pub(crate) fn can_change_to(self, category: TargetRoomCategory) -> bool {
        match self {
            Self::Invited => {
                matches!(
                    category,
                    TargetRoomCategory::Favorite
                        | TargetRoomCategory::Normal
                        | TargetRoomCategory::LowPriority
                        | TargetRoomCategory::Left
                )
            }
            Self::Favorite => {
                matches!(
                    category,
                    TargetRoomCategory::Normal
                        | TargetRoomCategory::LowPriority
                        | TargetRoomCategory::Left
                )
            }
            Self::Normal => {
                matches!(
                    category,
                    TargetRoomCategory::Favorite
                        | TargetRoomCategory::LowPriority
                        | TargetRoomCategory::Left
                )
            }
            Self::LowPriority => {
                matches!(
                    category,
                    TargetRoomCategory::Favorite
                        | TargetRoomCategory::Normal
                        | TargetRoomCategory::Left
                )
            }
            Self::Left => {
                matches!(
                    category,
                    TargetRoomCategory::Favorite
                        | TargetRoomCategory::Normal
                        | TargetRoomCategory::LowPriority
                )
            }
            Self::Knocked | Self::Ignored | Self::Outdated | Self::Space => false,
        }
    }

    /// Whether this `RoomCategory` corresponds to the given state.
    pub(crate) fn is_state(self, state: RoomState) -> bool {
        match self {
            RoomCategory::Knocked => state == RoomState::Knocked,
            RoomCategory::Invited | RoomCategory::Ignored => state == RoomState::Invited,
            RoomCategory::Favorite
            | RoomCategory::Normal
            | RoomCategory::LowPriority
            | RoomCategory::Outdated
            | RoomCategory::Space => state == RoomState::Joined,
            RoomCategory::Left => state == RoomState::Left,
        }
    }

    /// Convert this `RoomCategory` into a `TargetRoomCategory`, if possible.
    pub(crate) fn to_target_room_category(self) -> Option<TargetRoomCategory> {
        let target = match self {
            RoomCategory::Favorite => TargetRoomCategory::Favorite,
            RoomCategory::Normal => TargetRoomCategory::Normal,
            RoomCategory::LowPriority => TargetRoomCategory::LowPriority,
            RoomCategory::Left => TargetRoomCategory::Left,
            RoomCategory::Knocked
            | RoomCategory::Invited
            | RoomCategory::Outdated
            | RoomCategory::Space
            | RoomCategory::Ignored => return None,
        };

        Some(target)
    }
}

impl fmt::Display for RoomCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(section_name) = SidebarSectionName::from_room_category(*self) else {
            unimplemented!();
        };

        section_name.fmt(f)
    }
}

/// The room categories that can be targeted by the user.
///
/// This is a subset of [`RoomCategory`].
#[derive(Debug, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "TargetRoomCategory")]
pub enum TargetRoomCategory {
    /// Join or move the room into the favorite category.
    Favorite,
    /// Join or move the room into the normal category.
    Normal,
    /// Join or move the room into the low priority category.
    LowPriority,
    /// Leave the room.
    Left,
}

impl From<TargetRoomCategory> for RoomCategory {
    fn from(value: TargetRoomCategory) -> Self {
        match value {
            TargetRoomCategory::Favorite => Self::Favorite,
            TargetRoomCategory::Normal => Self::Normal,
            TargetRoomCategory::LowPriority => Self::LowPriority,
            TargetRoomCategory::Left => Self::Left,
        }
    }
}

impl PartialEq<RoomCategory> for TargetRoomCategory {
    fn eq(&self, other: &RoomCategory) -> bool {
        match self {
            Self::Favorite => *other == RoomCategory::Favorite,
            Self::Normal => *other == RoomCategory::Normal,
            Self::LowPriority => *other == RoomCategory::LowPriority,
            Self::Left => *other == RoomCategory::Left,
        }
    }
}

impl PartialEq<TargetRoomCategory> for RoomCategory {
    fn eq(&self, other: &TargetRoomCategory) -> bool {
        other.eq(self)
    }
}
