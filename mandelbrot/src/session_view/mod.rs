use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, gio, glib, glib::clone};
use ruma::{OwnedEventId, OwnedUserId, RoomId, RoomOrAliasId};
use tracing::{error, warn};

// TODO: Use the remaining widgets when the call UI is mounted in the room
// view.
#[allow(dead_code)]
mod call;
use self::call::present_call_dialog;
pub(crate) use self::call::{CallConnectionState, CallParticipant, CallState};
mod content;
mod create_direct_chat_dialog;
mod create_room_dialog;
mod explore;
mod invite;
mod invite_request;
mod media_viewer;
mod room_details;
mod room_history;
mod sidebar;
mod space_overview;

use self::{
    content::Content, create_direct_chat_dialog::CreateDirectChatDialog,
    create_room_dialog::CreateRoomDialog, explore::Explore, invite::Invite,
    invite_request::InviteRequest, media_viewer::MediaViewer, room_details::RoomDetails,
    room_history::RoomHistory, sidebar::Sidebar, space_overview::SpaceOverview,
};
use crate::{
    Window,
    components::{RoomPreviewDialog, UserProfileDialog},
    intent::SessionIntent,
    prelude::*,
    session::{
        IdentityVerification, Room, RoomCategory, RoomList, Session, SidebarItemList,
        SidebarListModel, VerificationKey,
    },
    utils::matrix::{MatrixEventIdUri, MatrixIdUri, MatrixRoomIdUri, VisualMediaMessage},
};

/// The default minimum width of the sidebar, in `sp`, the same as
/// `AdwNavigationSplitView`'s default.
const SIDEBAR_MIN_WIDTH: f64 = 180.0;
/// The default maximum width of the sidebar, in `sp`, the same as
/// `AdwNavigationSplitView`'s default.
const SIDEBAR_MAX_WIDTH: f64 = 280.0;
/// The minimum width of the sidebar when the space switcher is visible, in
/// `sp`.
///
/// The space switcher is 60sp wide, so the list of rooms keeps the default
/// minimum width. With the minimum width of the content, it fits in the
/// smallest window width where the sidebar is not collapsed.
const SIDEBAR_WITH_SPACES_MIN_WIDTH: f64 = SIDEBAR_MIN_WIDTH + 60.0;
/// The maximum width of the sidebar when the space switcher is visible, in
/// `sp`.
const SIDEBAR_WITH_SPACES_MAX_WIDTH: f64 = SIDEBAR_MAX_WIDTH + 60.0;

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/mod.ui")]
    #[properties(wrapper_type = super::SessionView)]
    pub struct SessionView {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        overlay: TemplateChild<gtk::Overlay>,
        #[template_child]
        split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        sidebar: TemplateChild<Sidebar>,
        #[template_child]
        content: TemplateChild<Content>,
        #[template_child]
        media_viewer: TemplateChild<MediaViewer>,
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
        window_active_handler_id: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionView {
        const NAME: &'static str = "SessionView";
        type Type = super::SessionView;
        type ParentType = adw::Bin;

        #[allow(clippy::too_many_lines)]
        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action("session.open-account-settings", None, |obj, _, _| {
                let Some(session) = obj.session() else {
                    return;
                };

                if obj
                    .activate_action(
                        "win.open-account-settings",
                        Some(&session.session_id().to_variant()),
                    )
                    .is_err()
                {
                    error!("Could not activate action `win.open-account-settings`");
                }
            });
            klass.add_binding_action(
                gdk::Key::comma,
                gdk::ModifierType::CONTROL_MASK,
                "session.open-account-settings",
            );

            klass.install_action("session.close-room", None, |obj, _, _| {
                obj.imp().select_item(None);
            });
            klass.add_binding_action(
                gdk::Key::Escape,
                gdk::ModifierType::empty(),
                "session.close-room",
            );

            klass.install_action(
                "session.show-room",
                Some(&String::static_variant_type()),
                |obj, _, parameter| {
                    let Some(parameter) = parameter else {
                        error!("Could not show room without an ID");
                        return;
                    };
                    let Some(room_id_str) = parameter.get::<String>() else {
                        error!("Could not show room with non-string ID");
                        return;
                    };
                    let Ok(room_id) = <&RoomId>::try_from(room_id_str.as_str()) else {
                        error!("Could not show room with invalid ID");
                        return;
                    };

                    obj.imp().select_room_by_id(room_id);
                },
            );

            klass.install_action("session.create-room", None, |obj, _, _| {
                obj.imp().create_room();
            });

            klass.install_action("session.join-room", None, |obj, _, _| {
                obj.imp().preview_room(None);
            });
            klass.add_binding_action(
                gdk::Key::L,
                gdk::ModifierType::CONTROL_MASK,
                "session.join-room",
            );

            klass.install_action("session.create-direct-chat", None, |obj, _, _| {
                obj.imp().create_direct_chat();
            });

            klass.install_action("session.toggle-room-search", None, |obj, _, _| {
                obj.imp().toggle_room_search();
            });
            klass.add_binding_action(
                gdk::Key::k,
                gdk::ModifierType::CONTROL_MASK,
                "session.toggle-room-search",
            );

            klass.install_action("session.select-unread-room", None, |obj, _, _| {
                obj.imp().select_unread_room();
            });
            klass.add_binding_action(
                gdk::Key::asterisk,
                gdk::ModifierType::CONTROL_MASK,
                "session.select-unread-room",
            );

            klass.install_action("session.select-prev-room", None, |obj, _, _| {
                obj.imp().select_next_room(ReadState::Any, Direction::Up);
            });

            klass.install_action("session.select-prev-unread-room", None, |obj, _, _| {
                obj.imp().select_next_room(ReadState::Unread, Direction::Up);
            });

            klass.install_action("session.select-next-room", None, |obj, _, _| {
                obj.imp().select_next_room(ReadState::Any, Direction::Down);
            });

            klass.install_action("session.select-next-unread-room", None, |obj, _, _| {
                obj.imp()
                    .select_next_room(ReadState::Unread, Direction::Down);
            });

            klass.install_action("session.select-prev-space", None, |obj, _, _| {
                if let Some(item_list) = obj.imp().item_list() {
                    item_list.space_list().select_relative(false);
                }
            });

            klass.install_action("session.select-next-space", None, |obj, _, _| {
                if let Some(item_list) = obj.imp().item_list() {
                    item_list.space_list().select_relative(true);
                }
            });

            klass.install_action("session.select-home", None, |obj, _, _| {
                if let Some(item_list) = obj.imp().item_list() {
                    item_list.space_list().select(0);
                }
            });

            klass.install_action(
                "session.show-matrix-uri",
                Some(&MatrixIdUri::static_variant_type()),
                |obj, _, parameter| {
                    let Some(parameter) = parameter else {
                        error!("Could not show missing Matrix URI");
                        return;
                    };
                    let Some(uri) = parameter.get::<MatrixIdUri>() else {
                        error!("Could not show invalid Matrix URI");
                        return;
                    };

                    obj.imp().show_matrix_uri(uri);
                },
            );
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SessionView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.content.connect_item_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |content| {
                    let show_content = content.item().is_some();
                    imp.split_view.set_show_content(show_content);

                    // Only grab focus for the sidebar here. We handle the other case in
                    // `Content::set_item()` directly, because we need to grab focus only
                    // after the visible content changed.
                    if !show_content {
                        imp.sidebar.grab_focus();
                    }

                    // Withdraw the notifications of the newly selected item.
                    imp.withdraw_selected_item_notifications();
                }
            ));

            obj.connect_root_notify(|obj| {
                let imp = obj.imp();

                let Some(window) = imp.parent_window() else {
                    return;
                };

                let handler_id = window.connect_is_active_notify(clone!(
                    #[weak]
                    imp,
                    move |window| {
                        if !window.is_active() {
                            return;
                        }

                        // When the window becomes active, withdraw the notifications
                        // of the selected item.
                        imp.withdraw_selected_item_notifications();
                    }
                ));
                imp.window_active_handler_id.replace(Some(handler_id));
            });

            // Make sure all header bars on the same screen have the same height.
            // Necessary when the text scaling changes.
            let size_group = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
            size_group.add_widget(self.sidebar.header_bar());

            for header_bar in self.content.header_bars() {
                size_group.add_widget(header_bar);
            }

            // Leave more room to the list of rooms when the space switcher is
            // visible next to it.
            self.sidebar.connect_has_spaces_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_sidebar_width();
                }
            ));
            self.update_sidebar_width();
        }

        fn dispose(&self) {
            if let Some(handler_id) = self.window_active_handler_id.take()
                && let Some(window) = self.parent_window()
            {
                window.disconnect(handler_id);
            }
        }
    }

    impl WidgetImpl for SessionView {}
    impl BinImpl for SessionView {}

    impl SessionView {
        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if self.session.upgrade().as_ref() == session {
                return;
            }

            self.session.set(session);
            self.obj().notify_session();
        }

        /// Get the [`SidebarListModel`] of the current session.
        fn sidebar_list_model(&self) -> Option<SidebarListModel> {
            self.session
                .upgrade()
                .map(|session| session.sidebar_list_model())
        }

        /// Get the [`SidebarItemList`] of the current session.
        fn item_list(&self) -> Option<SidebarItemList> {
            self.sidebar_list_model()
                .map(|sidebar_list_model| sidebar_list_model.item_list())
        }

        /// Get the [`RoomList`] of the current session.
        fn room_list(&self) -> Option<RoomList> {
            self.session.upgrade().map(|session| session.room_list())
        }

        /// Select the given item.
        pub(super) fn select_item(&self, item: Option<glib::Object>) {
            let Some(sidebar_list_model) = self.sidebar_list_model() else {
                return;
            };

            sidebar_list_model.selection_model().set_selected_item(item);
        }

        /// The currently selected item, if any.
        pub(super) fn selected_item(&self) -> Option<glib::Object> {
            self.content.item()
        }

        /// Select the given room.
        pub(super) fn select_room(&self, room: Room) {
            // Make sure the room is visible in the sidebar.
            // First, make sure that it is in the selected space, or navigate to it
            // if it is a space.
            if let Some(item_list) = self.item_list() {
                item_list.space_list().show_room(&room);
            }

            // Then, ensure that the section containing the room is expanded.
            if let Some(section) = self
                .item_list()
                .and_then(|item_list| item_list.section_from_room_category(room.category()))
            {
                section.set_is_expanded(true);
            }

            self.select_item(Some(room.upcast()));

            // Now scroll to the room to make sure that it is in the viewport, and that it
            // is focused in the list for users using keyboard navigation.
            self.sidebar.scroll_to_selection();
        }

        /// The currently selected room, if any.
        pub(super) fn selected_room(&self) -> Option<Room> {
            self.selected_item().and_downcast()
        }

        /// Select the room with the given ID in this view.
        pub(super) fn select_room_by_id(&self, room_id: &RoomId) {
            if let Some(room) = self
                .room_list()
                .and_then(|room_list| room_list.get(room_id))
            {
                self.select_room(room);
            } else {
                warn!("The room with ID {room_id} could not be found");
            }
        }

        /// Select the room with the given identifier in this view, if it
        /// exists.
        ///
        /// Returns `true` if the room was found.
        pub(super) fn select_room_if_exists(&self, identifier: &RoomOrAliasId) -> bool {
            if let Some(room) = self
                .room_list()
                .and_then(|room_list| room_list.get_by_identifier(identifier))
            {
                self.select_room(room);
                true
            } else {
                false
            }
        }

        /// Select the identity verification with the given key in this view.
        pub(super) fn select_identity_verification_by_id(&self, key: &VerificationKey) {
            if let Some(verification) = self
                .session
                .upgrade()
                .and_then(|s| s.verification_list().get(key))
            {
                self.select_identity_verification(verification);
            } else {
                warn!(
                    "Identity verification for user {} with flow ID {} could not be found",
                    key.user_id, key.flow_id
                );
            }
        }

        /// Select the given identity verification in this view.
        pub(super) fn select_identity_verification(&self, verification: IdentityVerification) {
            self.select_item(Some(verification.upcast()));
        }

        /// Withdraw the notifications for the currently selected item.
        fn withdraw_selected_item_notifications(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };
            let Some(item) = self.selected_item() else {
                return;
            };

            let notifications = session.notifications();

            if let Some(room) = item.downcast_ref::<Room>() {
                notifications.withdraw_all_for_room(room.room_id());
            } else if let Some(verification) = item.downcast_ref::<IdentityVerification>() {
                notifications.withdraw_identity_verification(&verification.key());
            }
        }

        /// Select the next room with the given read state in the given
        /// direction.
        ///
        /// The search wraps: if no room matches below (for `direction == Down`)
        /// then search continues in the down direction from the first room.
        fn select_next_room(&self, read_state: ReadState, direction: Direction) {
            let Some(sidebar_list_model) = self.sidebar_list_model() else {
                return;
            };

            let selection_list = sidebar_list_model.selection_model();
            let len = selection_list.n_items();
            let current_index = selection_list.selected().min(len);

            let search_order: Box<dyn Iterator<Item = u32>> = {
                // Iterate over every item except the current one.
                let order = ((current_index + 1)..len).chain(0..current_index);
                match direction {
                    Direction::Up => Box::new(order.rev()),
                    Direction::Down => Box::new(order),
                }
            };

            for index in search_order {
                let Some(item) = selection_list.item(index) else {
                    // The list of rooms was mutated: let's give up responding to the key binding.
                    return;
                };

                if let Ok(room) = item.downcast::<Room>()
                    && (read_state == ReadState::Any || !room.is_read())
                {
                    self.select_room(room);
                    return;
                }
            }
        }

        /// Select a room with unread messages.
        fn select_unread_room(&self) {
            let Some(room_list) = self.room_list() else {
                return;
            };
            let current_room = self.selected_room();

            if let Some((unread_room, _score)) = room_list
                .snapshot()
                .into_iter()
                .filter(|room| Some(room) != current_room.as_ref())
                .filter_map(|room| Self::score_for_unread_room(&room).map(|score| (room, score)))
                .max_by_key(|(_room, score)| *score)
            {
                self.select_room(unread_room);
            }
        }

        /// The score to determine the order in which unread rooms are selected.
        ///
        /// First by category, then by notification count so DMs are selected
        /// before group chats, and finally by recency.
        ///
        /// Returns `None` if the room should never be selected.
        fn score_for_unread_room(room: &Room) -> Option<(u8, u64, u64)> {
            if room.is_read() {
                return None;
            }

            let category_score = match room.category() {
                RoomCategory::Invited => 5,
                RoomCategory::Favorite => 4,
                RoomCategory::Normal => 3,
                RoomCategory::LowPriority => 2,
                RoomCategory::Left => 1,
                RoomCategory::Knocked
                | RoomCategory::Ignored
                | RoomCategory::Outdated
                | RoomCategory::Space => return None,
            };

            Some((
                category_score,
                room.notification_count(),
                room.latest_activity(),
            ))
        }

        /// Toggle the visibility of the room search bar.
        fn toggle_room_search(&self) {
            let room_search = self.sidebar.room_search_bar();
            room_search.set_search_mode(!room_search.is_search_mode());
        }

        /// Update the width of the sidebar according to whether the space
        /// switcher is visible.
        fn update_sidebar_width(&self) {
            let (min_width, max_width) = if self.sidebar.has_spaces() {
                (SIDEBAR_WITH_SPACES_MIN_WIDTH, SIDEBAR_WITH_SPACES_MAX_WIDTH)
            } else {
                (SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH)
            };

            self.split_view.set_min_sidebar_width(min_width);
            self.split_view.set_max_sidebar_width(max_width);
        }

        /// Returns the ancestor window containing this widget.
        fn parent_window(&self) -> Option<Window> {
            self.obj().root().and_downcast()
        }

        /// Show the dialog to create a room.
        fn create_room(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let dialog = CreateRoomDialog::new(&session);
            dialog.present(Some(&*self.obj()));
        }

        /// Show the dialog to create a direct chat.
        fn create_direct_chat(&self) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let dialog = CreateDirectChatDialog::new(&session);
            dialog.present(Some(&*self.obj()));
        }

        /// Show the dialog to preview a room.
        ///
        /// If no room URI is provided, the user will have to enter one.
        pub(super) fn preview_room(&self, room_uri: Option<MatrixRoomIdUri>) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            if room_uri
                .as_ref()
                .is_some_and(|room_uri| self.select_room_if_exists(&room_uri.id))
            {
                return;
            }

            let dialog = RoomPreviewDialog::new(&session);

            if let Some(uri) = room_uri {
                dialog.set_uri(uri);
            }

            dialog.present(Some(&*self.obj()));
        }

        /// Handle when the paste shortcut was activated.
        pub(super) fn handle_paste_action(&self) {
            self.content.handle_paste_action();
        }

        /// Send the given file in the room being shown, if a room is.
        pub(super) async fn send_file(&self, file: gio::File) -> bool {
            self.content.send_file(file).await
        }

        /// Show the given media event in the media viewer.
        pub(super) fn show_media_viewer(
            &self,
            source_widget: &gtk::Widget,
            room: &Room,
            media_message: VisualMediaMessage,
            event_id: Option<OwnedEventId>,
        ) {
            self.media_viewer.set_message(room, media_message, event_id);
            self.media_viewer.reveal(source_widget);
        }

        /// Show the profile of the given user.
        pub(super) fn show_user_profile_dialog(&self, user_id: OwnedUserId) {
            let Some(session) = self.session.upgrade() else {
                return;
            };

            let dialog = UserProfileDialog::new();
            dialog.load_user(&session, user_id);
            dialog.present(Some(&*self.obj()));
        }

        /// Process the given intent.
        pub(super) fn process_intent(&self, intent: SessionIntent) {
            match intent {
                SessionIntent::ShowMatrixId(matrix_uri) => {
                    self.show_matrix_uri(matrix_uri);
                }
                SessionIntent::ShowIdentityVerification(key) => {
                    self.select_identity_verification_by_id(&key);
                }
                SessionIntent::ShowRoomCall(room_uri) => {
                    self.show_room_call(&room_uri);
                }
            }
        }

        /// Show the call view of the room with the given URI.
        pub(super) fn show_room_call(&self, uri: &MatrixIdUri) {
            let (MatrixIdUri::Room(room_uri)
            | MatrixIdUri::Event(MatrixEventIdUri { room_uri, .. })) = uri
            else {
                return;
            };
            let Some(session) = self.session.upgrade() else {
                return;
            };
            if !self.select_room_if_exists(&room_uri.id) {
                return;
            }
            let Some(room) = self.selected_room() else {
                return;
            };

            present_call_dialog(&*self.obj(), &session, &room);
        }

        /// Show the given `MatrixIdUri`.
        pub(super) fn show_matrix_uri(&self, uri: MatrixIdUri) {
            match uri {
                MatrixIdUri::Room(room_uri)
                | MatrixIdUri::Event(MatrixEventIdUri { room_uri, .. }) => {
                    self.preview_room(Some(room_uri));
                }
                MatrixIdUri::User(user_id) => {
                    self.show_user_profile_dialog(user_id);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A view for a Matrix user session.
    pub struct SessionView(ObjectSubclass<imp::SessionView>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SessionView {
    /// Create a new session view.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The currently selected room, if any.
    pub(crate) fn selected_room(&self) -> Option<Room> {
        self.imp().selected_room()
    }

    /// Select the given room.
    pub(crate) fn select_room(&self, room: Room) {
        self.imp().select_room(room);
    }

    /// Select the room with the given identifier in this view, if it exists.
    ///
    /// Returns `true` if the room was found.
    pub(crate) fn select_room_if_exists(&self, identifier: &RoomOrAliasId) -> bool {
        self.imp().select_room_if_exists(identifier)
    }

    /// Select the given identity verification in this view.
    pub(crate) fn select_identity_verification(&self, verification: IdentityVerification) {
        self.imp().select_identity_verification(verification);
    }

    /// Handle when the paste action was activated.
    pub(crate) fn handle_paste_action(&self) {
        self.imp().handle_paste_action();
    }

    /// Send the given file in the room being shown, if a room is.
    ///
    /// Returns `false` if no room is shown.
    pub(crate) async fn send_file(&self, file: gio::File) -> bool {
        self.imp().send_file(file).await
    }

    /// Show the given media event in the media viewer.
    pub(crate) fn show_media_viewer(
        &self,
        source_widget: &impl IsA<gtk::Widget>,
        room: &Room,
        media_message: VisualMediaMessage,
        event_id: Option<OwnedEventId>,
    ) {
        self.imp()
            .show_media_viewer(source_widget.upcast_ref(), room, media_message, event_id);
    }

    /// Show the given `MatrixIdUri`.
    pub(crate) fn show_matrix_uri(&self, uri: MatrixIdUri) {
        self.imp().show_matrix_uri(uri);
    }

    /// Process the given intent.
    pub(crate) fn process_intent(&self, intent: SessionIntent) {
        self.imp().process_intent(intent);
    }
}

/// A predicate to filter rooms depending on whether they have unread messages.
#[derive(Eq, PartialEq, Copy, Clone)]
enum ReadState {
    /// Any room can be selected.
    Any,
    /// Only rooms with unread messages can be selected.
    Unread,
}

/// A direction in the room list.
#[derive(Eq, PartialEq, Copy, Clone)]
enum Direction {
    /// We are navigating from bottom to top.
    Up,
    /// We are navigating from top to bottom.
    Down,
}
