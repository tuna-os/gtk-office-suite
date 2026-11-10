use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gdk, gio, glib, glib::clone};
use ruma::api::client::receipt::create_receipt::v3::ReceiptType;
use tracing::error;

use super::{
    Sidebar, SidebarIconItemRow, SidebarRoomRow, SidebarSectionRow, SidebarVerificationRow,
};
use crate::{
    components::{ContextMenuBin, confirm_leave_room_dialog},
    prelude::*,
    session::{
        IdentityVerification, ReceiptPosition, Room, RoomCategory, SidebarIconItem,
        SidebarIconItemType, SidebarSection, TargetRoomCategory, User,
    },
    spawn, spawn_tokio, toast,
    utils::BoundObjectWeakRef,
};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SidebarRow)]
    pub struct SidebarRow {
        /// The ancestor sidebar of this row.
        #[property(get, set = Self::set_sidebar, construct_only)]
        sidebar: BoundObjectWeakRef<Sidebar>,
        /// The item of this row.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: RefCell<Option<glib::Object>>,
        room_handler: RefCell<Option<glib::SignalHandlerId>>,
        room_join_rule_handler: RefCell<Option<glib::SignalHandlerId>>,
        room_is_read_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarRow {
        const NAME: &'static str = "SidebarRow";
        type Type = super::SidebarRow;
        type ParentType = ContextMenuBin;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("sidebar-row");
            klass.set_accessible_role(gtk::AccessibleRole::ListItem);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarRow {
        fn constructed(&self) {
            self.parent_constructed();

            // Set up drop controller
            let drop = gtk::DropTarget::builder()
                .actions(gdk::DragAction::MOVE)
                .formats(&gdk::ContentFormats::for_type(Room::static_type()))
                .build();
            drop.connect_accept(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                false,
                move |_, drop| imp.drop_accept(drop)
            ));
            drop.connect_leave(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.drop_leave();
                }
            ));
            drop.connect_drop(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                false,
                move |_, v, _, _| imp.drop_end(v)
            ));
            self.obj().add_controller(drop);
        }

        fn dispose(&self) {
            if let Some(room) = self.room() {
                if let Some(handler) = self.room_join_rule_handler.take() {
                    room.join_rule().disconnect(handler);
                }
                if let Some(handler) = self.room_is_read_handler.take() {
                    room.disconnect(handler);
                }
            }
        }
    }

    impl WidgetImpl for SidebarRow {}

    impl ContextMenuBinImpl for SidebarRow {
        fn menu_opened(&self) {
            if !self
                .item
                .borrow()
                .as_ref()
                .is_some_and(glib::Object::is::<Room>)
            {
                // No context menu.
                return;
            }

            let obj = self.obj();
            if let Some(sidebar) = obj.sidebar() {
                let popover = sidebar.room_row_popover();
                obj.set_popover(Some(popover.clone()));
            }
        }
    }

    impl SidebarRow {
        /// Set the ancestor sidebar of this row.
        fn set_sidebar(&self, sidebar: &Sidebar) {
            let drop_source_category_handler =
                sidebar.connect_drop_source_category_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_for_drop_source_category();
                    }
                ));

            let drop_active_target_category_handler = sidebar
                .connect_drop_active_target_category_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_for_drop_active_target_category();
                    }
                ));

            self.sidebar.set(
                sidebar,
                vec![
                    drop_source_category_handler,
                    drop_active_target_category_handler,
                ],
            );
        }

        /// Set the item of this row.
        fn set_item(&self, item: Option<glib::Object>) {
            if *self.item.borrow() == item {
                return;
            }
            let obj = self.obj();

            if let Some(room) = self.room() {
                if let Some(handler) = self.room_handler.take() {
                    room.disconnect(handler);
                }
                if let Some(handler) = self.room_join_rule_handler.take() {
                    room.join_rule().disconnect(handler);
                }
                if let Some(handler) = self.room_is_read_handler.take() {
                    room.disconnect(handler);
                }
            }

            self.item.replace(item.clone());

            self.update_context_menu();

            if let Some(item) = item {
                if let Some(section) = item.downcast_ref::<SidebarSection>() {
                    let child = obj.child_or_else::<SidebarSectionRow>(|| {
                        let child = SidebarSectionRow::new();
                        obj.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
                            child.labelled_by()
                        ])]);
                        child
                    });
                    child.set_section(Some(section.clone()));
                } else if let Some(room) = item.downcast_ref::<Room>() {
                    let child = obj.child_or_default::<SidebarRoomRow>();

                    let room_is_direct_handler = room.connect_is_direct_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_context_menu();
                        }
                    ));
                    self.room_handler.replace(Some(room_is_direct_handler));
                    let room_join_rule_handler =
                        room.join_rule().connect_we_can_join_notify(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_| {
                                imp.update_context_menu();
                            }
                        ));
                    self.room_join_rule_handler
                        .replace(Some(room_join_rule_handler));

                    let room_is_read_handler = room.connect_is_read_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_context_menu();
                        }
                    ));
                    self.room_is_read_handler
                        .replace(Some(room_is_read_handler));

                    child.set_room(Some(room.clone()));
                } else if let Some(icon_item) = item.downcast_ref::<SidebarIconItem>() {
                    let child = obj.child_or_default::<SidebarIconItemRow>();
                    child.set_icon_item(Some(icon_item.clone()));
                } else if let Some(verification) = item.downcast_ref::<IdentityVerification>() {
                    let child = obj.child_or_default::<SidebarVerificationRow>();
                    child.set_identity_verification(Some(verification.clone()));
                } else {
                    panic!("Wrong row item: {item:?}");
                }

                self.update_for_drop_source_category();
            }

            self.update_context_menu();
            obj.notify_item();
        }

        /// Get the `Room` of this item, if this is a room row.
        pub(super) fn room(&self) -> Option<Room> {
            self.item.borrow().clone().and_downcast()
        }

        /// Get the `RoomCategory` of this row, if any.
        ///
        /// If this does not display a room or a section containing rooms,
        /// returns `None`.
        pub(super) fn room_category(&self) -> Option<RoomCategory> {
            let borrowed_item = self.item.borrow();
            let item = borrowed_item.as_ref()?;

            if let Some(room) = item.downcast_ref::<Room>() {
                Some(room.category())
            } else {
                item.downcast_ref::<SidebarSection>()
                    .and_then(|section| section.name().into_room_category())
            }
        }

        /// Get the `TargetRoomCategory` of this row, if any.
        pub(super) fn target_room_category(&self) -> Option<TargetRoomCategory> {
            self.room_category()
                .and_then(RoomCategory::to_target_room_category)
        }

        /// Get the [`SidebarIconItemType`] of the icon item displayed by this
        /// row, if any.
        pub(super) fn item_type(&self) -> Option<SidebarIconItemType> {
            let borrowed_item = self.item.borrow();
            borrowed_item
                .as_ref()?
                .downcast_ref::<SidebarIconItem>()
                .map(SidebarIconItem::item_type)
        }

        /// Whether this has a room context menu.
        fn has_room_context_menu(&self) -> bool {
            self.room().is_some_and(|r| {
                matches!(
                    r.category(),
                    RoomCategory::Invited
                        | RoomCategory::Favorite
                        | RoomCategory::Normal
                        | RoomCategory::LowPriority
                        | RoomCategory::Left
                )
            })
        }

        /// Update the context menu according to the current state.
        fn update_context_menu(&self) {
            let obj = self.obj();

            if !self.has_room_context_menu() {
                obj.insert_action_group("room-row", None::<&gio::ActionGroup>);
                obj.set_has_context_menu(false);
                return;
            }

            obj.insert_action_group("room-row", self.room_actions().as_ref());
            obj.set_has_context_menu(true);
        }

        /// An action group with the available room actions.
        #[allow(clippy::too_many_lines)]
        fn room_actions(&self) -> Option<gio::SimpleActionGroup> {
            let room = self.room()?;

            let action_group = gio::SimpleActionGroup::new();
            let category = room.category();

            match category {
                RoomCategory::Knocked => {
                    action_group.add_action_entries([gio::ActionEntry::builder(
                        "retract-invite-request",
                    )
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            if let Some(room) = imp.room() {
                                spawn!(async move {
                                    imp.set_room_category(&room, TargetRoomCategory::Left).await;
                                });
                            }
                        }
                    ))
                    .build()]);
                }
                RoomCategory::Invited => {
                    action_group.add_action_entries([
                        gio::ActionEntry::builder("accept-invite")
                            .activate(clone!(
                                #[weak(rename_to = imp)]
                                self,
                                move |_, _, _| {
                                    if let Some(room) = imp.room() {
                                        spawn!(async move {
                                            imp.set_room_category(
                                                &room,
                                                TargetRoomCategory::Normal,
                                            )
                                            .await;
                                        });
                                    }
                                }
                            ))
                            .build(),
                        gio::ActionEntry::builder("decline-invite")
                            .activate(clone!(
                                #[weak(rename_to = imp)]
                                self,
                                move |_, _, _| {
                                    if let Some(room) = imp.room() {
                                        spawn!(async move {
                                            imp.set_room_category(&room, TargetRoomCategory::Left)
                                                .await;
                                        });
                                    }
                                }
                            ))
                            .build(),
                    ]);
                }
                RoomCategory::Favorite | RoomCategory::Normal | RoomCategory::LowPriority => {
                    if matches!(category, RoomCategory::Favorite | RoomCategory::LowPriority) {
                        action_group.add_action_entries([gio::ActionEntry::builder("set-normal")
                            .activate(clone!(
                                #[weak(rename_to = imp)]
                                self,
                                move |_, _, _| {
                                    if let Some(room) = imp.room() {
                                        spawn!(async move {
                                            imp.set_room_category(
                                                &room,
                                                TargetRoomCategory::Normal,
                                            )
                                            .await;
                                        });
                                    }
                                }
                            ))
                            .build()]);
                    }

                    if matches!(category, RoomCategory::Normal | RoomCategory::LowPriority) {
                        action_group.add_action_entries([gio::ActionEntry::builder(
                            "set-favorite",
                        )
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                if let Some(room) = imp.room() {
                                    spawn!(async move {
                                        imp.set_room_category(&room, TargetRoomCategory::Favorite)
                                            .await;
                                    });
                                }
                            }
                        ))
                        .build()]);
                    }

                    if matches!(category, RoomCategory::Favorite | RoomCategory::Normal) {
                        action_group.add_action_entries([gio::ActionEntry::builder(
                            "set-lowpriority",
                        )
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                if let Some(room) = imp.room() {
                                    spawn!(async move {
                                        imp.set_room_category(
                                            &room,
                                            TargetRoomCategory::LowPriority,
                                        )
                                        .await;
                                    });
                                }
                            }
                        ))
                        .build()]);
                    }

                    action_group.add_action_entries([gio::ActionEntry::builder("leave")
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                if let Some(room) = imp.room() {
                                    spawn!(async move {
                                        imp.set_room_category(&room, TargetRoomCategory::Left)
                                            .await;
                                    });
                                }
                            }
                        ))
                        .build()]);

                    if room.is_read() {
                        action_group.add_action_entries([gio::ActionEntry::builder(
                            "mark-as-unread",
                        )
                        .activate(clone!(
                            #[weak]
                            room,
                            move |_, _, _| {
                                spawn!(async move {
                                    room.mark_as_unread().await;
                                });
                            }
                        ))
                        .build()]);
                    } else {
                        action_group.add_action_entries([gio::ActionEntry::builder(
                            "mark-as-read",
                        )
                        .activate(clone!(
                            #[weak]
                            room,
                            move |_, _, _| {
                                spawn!(async move {
                                    room.send_receipt(ReceiptType::Read, ReceiptPosition::End)
                                        .await;
                                });
                            }
                        ))
                        .build()]);
                    }
                }
                RoomCategory::Left => {
                    if room.join_rule().we_can_join() {
                        action_group.add_action_entries([gio::ActionEntry::builder("join")
                            .activate(clone!(
                                #[weak(rename_to = imp)]
                                self,
                                move |_, _, _| {
                                    if let Some(room) = imp.room() {
                                        spawn!(async move {
                                            imp.set_room_category(
                                                &room,
                                                TargetRoomCategory::Normal,
                                            )
                                            .await;
                                        });
                                    }
                                }
                            ))
                            .build()]);
                    }

                    action_group.add_action_entries([gio::ActionEntry::builder("forget")
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                if let Some(room) = imp.room() {
                                    spawn!(async move {
                                        imp.forget_room(&room).await;
                                    });
                                }
                            }
                        ))
                        .build()]);
                }
                RoomCategory::Outdated | RoomCategory::Space | RoomCategory::Ignored => {}
            }

            if matches!(
                category,
                RoomCategory::Favorite
                    | RoomCategory::Normal
                    | RoomCategory::LowPriority
                    | RoomCategory::Left
            ) {
                if room.is_direct() {
                    action_group.add_action_entries([gio::ActionEntry::builder(
                        "unset-direct-chat",
                    )
                    .activate(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_, _, _| {
                            if let Some(room) = imp.room() {
                                spawn!(async move {
                                    imp.set_room_is_direct(&room, false).await;
                                });
                            }
                        }
                    ))
                    .build()]);
                } else {
                    action_group.add_action_entries([gio::ActionEntry::builder("set-direct-chat")
                        .activate(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            move |_, _, _| {
                                if let Some(room) = imp.room() {
                                    spawn!(async move {
                                        imp.set_room_is_direct(&room, true).await;
                                    });
                                }
                            }
                        ))
                        .build()]);
                }
            }

            Some(action_group)
        }

        /// Update the disabled or empty state of this drop target.
        fn update_for_drop_source_category(&self) {
            let obj = self.obj();
            let source_category = self.sidebar.obj().and_then(|s| s.drop_source_category());

            if let Some(source_category) = source_category {
                if self
                    .target_room_category()
                    .is_some_and(|row_category| source_category.can_change_to(row_category))
                {
                    obj.remove_css_class("drop-disabled");

                    if self
                        .item
                        .borrow()
                        .as_ref()
                        .and_then(glib::Object::downcast_ref)
                        .is_some_and(SidebarSection::is_empty)
                    {
                        obj.add_css_class("drop-empty");
                    } else {
                        obj.remove_css_class("drop-empty");
                    }
                } else {
                    let is_forget_item = self
                        .item_type()
                        .is_some_and(|item_type| item_type == SidebarIconItemType::Forget);
                    if is_forget_item && source_category == RoomCategory::Left {
                        obj.remove_css_class("drop-disabled");
                    } else {
                        obj.add_css_class("drop-disabled");
                        obj.remove_css_class("drop-empty");
                    }
                }
            } else {
                // Clear style
                obj.remove_css_class("drop-disabled");
                obj.remove_css_class("drop-empty");
                obj.remove_css_class("drop-active");
            }

            if let Some(section_row) = obj.child().and_downcast::<SidebarSectionRow>() {
                section_row.set_show_label_for_room_category(source_category);
            }
        }

        /// Update the active state of this drop target.
        fn update_for_drop_active_target_category(&self) {
            let obj = self.obj();

            let Some(room_category) = self.room_category() else {
                obj.remove_css_class("drop-active");
                return;
            };

            let target_category = self
                .sidebar
                .obj()
                .and_then(|s| s.drop_active_target_category());

            if target_category.is_some_and(|target_category| target_category == room_category) {
                obj.add_css_class("drop-active");
            } else {
                obj.remove_css_class("drop-active");
            }
        }

        /// Handle the drag-n-drop hovering this row.
        fn drop_accept(&self, drop: &gdk::Drop) -> bool {
            let Some(sidebar) = self.sidebar.obj() else {
                return false;
            };

            let room = drop
                .drag()
                .map(|drag| drag.content())
                .and_then(|content| content.value(Room::static_type()).ok())
                .and_then(|value| value.get::<Room>().ok());
            if let Some(room) = room {
                if let Some(target_category) = self.target_room_category() {
                    if room.category().can_change_to(target_category) {
                        sidebar.set_drop_active_target_category(Some(target_category));
                        return true;
                    }
                } else if self
                    .item_type()
                    .is_some_and(|item_type| item_type == SidebarIconItemType::Forget)
                    && room.category() == RoomCategory::Left
                {
                    self.obj().add_css_class("drop-active");
                    sidebar.set_drop_active_target_category(None);
                    return true;
                }
            }

            false
        }

        /// Handle the drag-n-drop leaving this row.
        fn drop_leave(&self) {
            self.obj().remove_css_class("drop-active");
            if let Some(sidebar) = self.sidebar.obj() {
                sidebar.set_drop_active_target_category(None);
            }
        }

        /// Handle the drop on this row.
        fn drop_end(&self, value: &glib::Value) -> bool {
            let mut ret = false;
            if let Ok(room) = value.get::<Room>() {
                if let Some(target_category) = self.target_room_category() {
                    if room.category().can_change_to(target_category) {
                        spawn!(clone!(
                            #[weak(rename_to = imp)]
                            self,
                            async move {
                                imp.set_room_category(&room, target_category).await;
                            }
                        ));
                        ret = true;
                    }
                } else if self
                    .item_type()
                    .is_some_and(|item_type| item_type == SidebarIconItemType::Forget)
                    && room.category() == RoomCategory::Left
                {
                    spawn!(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        async move {
                            imp.forget_room(&room).await;
                        }
                    ));
                    ret = true;
                }
            }
            if let Some(sidebar) = self.sidebar.obj() {
                sidebar.set_drop_source_category(None);
            }
            ret
        }

        /// Change the category of the given room.
        async fn set_room_category(&self, room: &Room, category: TargetRoomCategory) {
            let obj = self.obj();

            let ignored_inviter = if category == TargetRoomCategory::Left {
                let Some(response) = confirm_leave_room_dialog(room, &*obj).await else {
                    return;
                };

                response.ignore_inviter.then(|| room.inviter()).flatten()
            } else {
                None
            };

            let previous_category = room.category();
            if room.change_category(category).await.is_err() {
                match previous_category {
                    RoomCategory::Invited => {
                        if category == RoomCategory::Left {
                            toast!(
                                obj,
                                gettext(
                                    // Translators: Do NOT translate the content between '{' and '}', this
                                    // is a variable name.
                                    "Could not decline invitation for {room}",
                                ),
                                @room,
                            );
                        } else {
                            toast!(
                                obj,
                                gettext(
                                    // Translators: Do NOT translate the content between '{' and '}', this
                                    // is a variable name.
                                    "Could not accept invitation for {room}",
                                ),
                                @room,
                            );
                        }
                    }
                    RoomCategory::Left => {
                        toast!(
                            obj,
                            gettext(
                                // Translators: Do NOT translate the content between '{' and '}', this is a
                                // variable name.
                                "Could not join {room}",
                            ),
                            @room,
                        );
                    }
                    _ => {
                        if category == RoomCategory::Left {
                            toast!(
                                obj,
                                gettext(
                                    // Translators: Do NOT translate the content between '{' and '}', this is a variable name.
                                    "Could not leave {room}",
                                ),
                                @room,
                            );
                        } else {
                            toast!(
                                obj,
                                gettext(
                                    // Translators: Do NOT translate the content between '{' and '}', this is a variable name.
                                    "Could not move {room} from {previous_category} to {new_category}",
                                ),
                                @room,
                                previous_category = previous_category.to_string(),
                                new_category = RoomCategory::from(category).to_string(),
                            );
                        }
                    }
                }
            }

            if let Some(inviter) = ignored_inviter
                && inviter.upcast::<User>().ignore().await.is_err()
            {
                toast!(obj, gettext("Could not ignore user"));
            }
        }

        /// Forget the given room.
        async fn forget_room(&self, room: &Room) {
            if room.forget().await.is_err() {
                toast!(
                    self.obj(),
                    // Translators: Do NOT translate the content between '{' and '}', this is a variable name.
                    gettext("Could not forget {room}"),
                    @room,
                );
            }
        }

        /// Set or unset the room as a direct chat.
        async fn set_room_is_direct(&self, room: &Room, is_direct: bool) {
            let matrix_room = room.matrix_room().clone();
            let handle = spawn_tokio!(async move { matrix_room.set_is_direct(is_direct).await });

            if let Err(error) = handle.await.unwrap() {
                let obj = self.obj();

                if is_direct {
                    error!("Could not mark room as direct chat: {error}");
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    toast!(obj, gettext("Could not mark {room} as direct chat"), @room);
                } else {
                    error!("Could not unmark room as direct chat: {error}");
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    toast!(obj, gettext("Could not unmark {room} as direct chat"), @room);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A row of the sidebar.
    pub struct SidebarRow(ObjectSubclass<imp::SidebarRow>)
        @extends gtk::Widget, ContextMenuBin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SidebarRow {
    pub fn new(sidebar: &Sidebar) -> Self {
        glib::Object::builder().property("sidebar", sidebar).build()
    }
}

impl ChildPropertyExt for SidebarRow {
    fn child_property(&self) -> Option<gtk::Widget> {
        self.child()
    }

    fn set_child_property(&self, child: Option<&impl IsA<gtk::Widget>>) {
        self.set_child(child);
    }
}
