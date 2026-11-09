use std::cell::Cell;

use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};

use super::{
    SidebarIconItem, SidebarIconItemType, SidebarItem, SidebarSection, SidebarSectionName,
    SidebarSpaceList,
};
use crate::session::{RoomCategory, RoomList, VerificationList};

/// The number of top-level items in the sidebar.
const TOP_LEVEL_ITEMS_COUNT: usize = 10;

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SidebarItemList)]
    pub struct SidebarItemList {
        /// The list of top-level items.
        list: OnceCell<[SidebarItem; TOP_LEVEL_ITEMS_COUNT]>,
        /// The list of rooms.
        #[property(get, construct_only)]
        room_list: OnceCell<RoomList>,
        /// The list of verification requests.
        #[property(get, construct_only)]
        verification_list: OnceCell<VerificationList>,
        /// The list of items of the space switcher, which also filters the
        /// rooms by space.
        #[property(get = Self::space_list)]
        space_list: OnceCell<SidebarSpaceList>,
        /// The room category to show all compatible sections and icon items
        /// for.
        ///
        /// The UI is updated to show possible drop actions for a room with the
        /// given category.
        show_all_for_room_category: Cell<Option<RoomCategory>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarItemList {
        const NAME: &'static str = "SidebarItemList";
        type Type = super::SidebarItemList;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarItemList {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let room_list = obj.room_list();
            let verification_list = obj.verification_list();
            let space_list = self
                .space_list
                .get_or_init(|| SidebarSpaceList::new(&room_list));

            let list = self.list.get_or_init(|| {
                [
                    SidebarItem::new(SidebarIconItem::new(SidebarIconItemType::Explore)),
                    SidebarItem::new(SidebarSection::new(
                        SidebarSectionName::VerificationRequest,
                        &verification_list,
                    )),
                    SidebarItem::new(SidebarSection::new(
                        SidebarSectionName::InviteRequest,
                        &room_list,
                    )),
                    SidebarItem::new(SidebarSection::new(SidebarSectionName::Invited, &room_list)),
                    SidebarItem::new(SidebarSection::new(SidebarSectionName::Space, &room_list)),
                    SidebarItem::new(SidebarSection::new(
                        SidebarSectionName::Favorite,
                        &room_list,
                    )),
                    SidebarItem::new(SidebarSection::new(SidebarSectionName::Normal, &room_list)),
                    SidebarItem::new(SidebarSection::new(
                        SidebarSectionName::LowPriority,
                        &room_list,
                    )),
                    SidebarItem::new(SidebarSection::new(SidebarSectionName::Left, &room_list)),
                    SidebarItem::new(SidebarIconItem::new(SidebarIconItemType::Forget)),
                ]
            });

            for item in list {
                if let Some(section) = item.inner_item().downcast_ref::<SidebarSection>() {
                    // Only show the rooms of the selected space.
                    if section.name() == SidebarSectionName::Space {
                        section.add_room_filter(space_list.subspace_filter());
                    } else {
                        section.add_room_filter(space_list.room_filter());
                    }

                    section.connect_is_empty_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        #[weak]
                        item,
                        move |_| {
                            imp.update_item_visibility(&item);
                        }
                    ));
                }
                self.update_item_visibility(item);
            }
        }
    }

    impl ListModelImpl for SidebarItemList {
        fn item_type(&self) -> glib::Type {
            SidebarItem::static_type()
        }

        fn n_items(&self) -> u32 {
            TOP_LEVEL_ITEMS_COUNT as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.list().get(position as usize).cloned().and_upcast()
        }
    }

    impl SidebarItemList {
        /// The list of top-level items.
        pub(super) fn list(&self) -> &[SidebarItem; TOP_LEVEL_ITEMS_COUNT] {
            self.list.get().unwrap()
        }

        /// The list of items of the space switcher.
        fn space_list(&self) -> SidebarSpaceList {
            self.space_list
                .get()
                .expect("space list should be initialized")
                .clone()
        }

        /// Set the room category to show all compatible sections and icon items
        /// for.
        pub(super) fn set_show_all_for_room_category(&self, category: Option<RoomCategory>) {
            if self.show_all_for_room_category.get() == category {
                return;
            }

            self.show_all_for_room_category.set(category);
            for item in self.list() {
                self.update_item_visibility(item);
            }
        }

        /// Update the visibility of the given item.
        fn update_item_visibility(&self, item: &SidebarItem) {
            item.update_visibility_for_room_category(self.show_all_for_room_category.get());
        }

        /// Set whether to inhibit the expanded state of the sections.
        ///
        /// It means that all the sections will be expanded regardless of
        /// their "is-expanded" property.
        pub(super) fn inhibit_expanded(&self, inhibit: bool) {
            for item in self.list() {
                item.set_inhibit_expanded(inhibit);
            }
        }
    }
}

glib::wrapper! {
    /// Fixed list of all subcomponents in the sidebar.
    ///
    /// Implements the `gio::ListModel` interface and yields the top-level
    /// items of the sidebar.
    pub struct SidebarItemList(ObjectSubclass<imp::SidebarItemList>)
        @implements gio::ListModel;
}

impl SidebarItemList {
    /// Construct a new `SidebarItemList` with the given room list and
    /// verification list.
    pub fn new(room_list: &RoomList, verification_list: &VerificationList) -> Self {
        glib::Object::builder()
            .property("room-list", room_list)
            .property("verification-list", verification_list)
            .build()
    }

    /// Set the room category to show all compatible sections and icon items
    /// for.
    pub(crate) fn set_show_all_for_room_category(&self, category: Option<RoomCategory>) {
        self.imp().set_show_all_for_room_category(category);
    }

    /// Set whether to inhibit the expanded state of the sections.
    ///
    /// It means that all the sections will be expanded regardless of their
    /// "is-expanded" property.
    pub(crate) fn inhibit_expanded(&self, inhibit: bool) {
        self.imp().inhibit_expanded(inhibit);
    }

    /// Returns the [`SidebarSection`] for the given room category.
    pub(crate) fn section_from_room_category(
        &self,
        category: RoomCategory,
    ) -> Option<SidebarSection> {
        const FIRST_ROOM_SECTION_INDEX: usize = 2;

        let index = match category {
            RoomCategory::Knocked => FIRST_ROOM_SECTION_INDEX,
            RoomCategory::Invited => FIRST_ROOM_SECTION_INDEX + 1,
            RoomCategory::Space => FIRST_ROOM_SECTION_INDEX + 2,
            RoomCategory::Favorite => FIRST_ROOM_SECTION_INDEX + 3,
            RoomCategory::Normal => FIRST_ROOM_SECTION_INDEX + 4,
            RoomCategory::LowPriority => FIRST_ROOM_SECTION_INDEX + 5,
            RoomCategory::Left => FIRST_ROOM_SECTION_INDEX + 6,
            _ => return None,
        };

        self.imp()
            .list()
            .get(index)
            .map(SidebarItem::inner_item)
            .and_downcast()
    }
}
