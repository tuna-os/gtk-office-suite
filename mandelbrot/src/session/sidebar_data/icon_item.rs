use std::fmt;

use gettextrs::gettext;
use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::session::RoomCategory;

#[derive(Debug, Default, Hash, Eq, PartialEq, Clone, Copy, glib::Enum)]
#[enum_type(name = "SidebarIconItemType")]
pub enum SidebarIconItemType {
    /// The explore view.
    #[default]
    Explore,
    /// An action to forget a room.
    Forget,
}

impl SidebarIconItemType {
    /// The name of the icon for this item type.
    pub(crate) fn icon_name(self) -> &'static str {
        match self {
            Self::Explore => "explore-symbolic",
            Self::Forget => "user-trash-symbolic",
        }
    }
}

impl fmt::Display for SidebarIconItemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Explore => gettext("Explore"),
            Self::Forget => gettext("Forget Room"),
        };

        f.write_str(&label)
    }
}

mod imp {
    use std::{cell::Cell, marker::PhantomData};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SidebarIconItem)]
    pub struct SidebarIconItem {
        /// The type of this item.
        #[property(get, construct_only, builder(SidebarIconItemType::default()))]
        item_type: Cell<SidebarIconItemType>,
        /// The display name of this item.
        #[property(get = Self::display_name)]
        display_name: PhantomData<String>,
        /// The icon name used for this item.
        #[property(get = Self::icon_name)]
        icon_name: PhantomData<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarIconItem {
        const NAME: &'static str = "SidebarIconItem";
        type Type = super::SidebarIconItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarIconItem {}

    impl SidebarIconItem {
        /// The display name of this item.
        fn display_name(&self) -> String {
            self.item_type.get().to_string()
        }

        /// The icon name used for this item.
        fn icon_name(&self) -> String {
            self.item_type.get().icon_name().to_owned()
        }
    }
}

glib::wrapper! {
    /// A top-level row in the sidebar with an icon.
    pub struct SidebarIconItem(ObjectSubclass<imp::SidebarIconItem>);
}

impl SidebarIconItem {
    pub fn new(item_type: SidebarIconItemType) -> Self {
        glib::Object::builder()
            .property("item-type", item_type)
            .build()
    }

    /// Whether this item should be shown for the drag-n-drop of a room with the
    /// given category.
    pub(crate) fn visible_for_room_category(&self, source_category: Option<RoomCategory>) -> bool {
        match self.item_type() {
            SidebarIconItemType::Explore => true,
            SidebarIconItemType::Forget => source_category == Some(RoomCategory::Left),
        }
    }
}
