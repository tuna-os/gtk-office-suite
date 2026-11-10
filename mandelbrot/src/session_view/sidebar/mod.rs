use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{
    gio, glib,
    glib::{clone, closure_local},
};
use tracing::error;

mod icon_item_row;
mod room_row;
mod row;
mod section_row;
mod space_row;
mod verification_row;

use self::{
    icon_item_row::SidebarIconItemRow, room_row::SidebarRoomRow, row::SidebarRow,
    section_row::SidebarSectionRow, space_row::SidebarSpaceRow,
    verification_row::SidebarVerificationRow,
};
use crate::{
    account_settings::{AccountSettings, AccountSettingsSubpage},
    account_switcher::AccountSwitcherButton,
    components::{Avatar, OfflineBanner},
    prelude::*,
    session::{
        CryptoIdentityState, RecoveryState, Room, RoomCategory, Session, SessionVerificationState,
        SidebarListModel, SidebarSection, SidebarSpaceList, TargetRoomCategory, User,
    },
    utils::{BoundObjectWeakRef, FixedSelection},
};

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::{InitializingObject, Signal};

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/sidebar/mod.ui")]
    #[properties(wrapper_type = super::Sidebar)]
    pub struct Sidebar {
        #[template_child]
        pub(super) header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        account_switcher_button: TemplateChild<AccountSwitcherButton>,
        #[template_child]
        security_banner: TemplateChild<adw::Banner>,
        #[template_child]
        scrolled_window: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        listview: TemplateChild<gtk::ListView>,
        #[template_child]
        room_search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        room_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub(super) room_search: TemplateChild<gtk::SearchBar>,
        #[template_child]
        room_row_menu: TemplateChild<gio::MenuModel>,
        #[template_child]
        space_switcher_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        space_split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        space_rail: TemplateChild<gtk::ListView>,
        #[template_child]
        space_header: TemplateChild<gtk::Box>,
        #[template_child]
        space_back_button: TemplateChild<gtk::Button>,
        #[template_child]
        space_header_avatar: TemplateChild<Avatar>,
        #[template_child]
        space_header_name: TemplateChild<gtk::Label>,
        room_row_popover: OnceCell<gtk::PopoverMenu>,
        /// The logged-in user.
        #[property(get, set = Self::set_user, explicit_notify, nullable)]
        user: RefCell<Option<User>>,
        /// The category of the source that activated drop mode.
        pub(super) drop_source_category: Cell<Option<RoomCategory>>,
        /// The category of the drop target that is currently hovered.
        pub(super) drop_active_target_category: Cell<Option<TargetRoomCategory>>,
        /// The list model of this sidebar.
        #[property(get, set = Self::set_list_model, explicit_notify, nullable)]
        list_model: glib::WeakRef<SidebarListModel>,
        session_handler: RefCell<Option<glib::SignalHandlerId>>,
        security_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        /// Whether the user has joined any space, so the space switcher is
        /// available.
        #[property(get)]
        has_spaces: Cell<bool>,
        /// The list of items of the space switcher.
        space_list: BoundObjectWeakRef<SidebarSpaceList>,
        /// The selection model of the space switcher.
        ///
        /// It ignores selection requests from the list view, so moving the
        /// focus with the keyboard does not switch spaces: only activating an
        /// item does.
        space_selection: FixedSelection,
        /// The binding of the name of the selected space.
        space_name_binding: RefCell<Option<glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Sidebar {
        const NAME: &'static str = "Sidebar";
        type Type = super::Sidebar;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("sidebar");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Sidebar {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("drop-source-category-changed").build(),
                    Signal::builder("drop-active-target-category-changed").build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(clone!(
                #[weak]
                obj,
                move |_, item| {
                    let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                        error!("List item factory did not receive a list item: {item:?}");
                        return;
                    };
                    let row = SidebarRow::new(&obj);
                    item.set_child(Some(&row));
                    item.bind_property("item", &row, "item").build();
                }
            ));
            self.listview.set_factory(Some(&factory));

            self.listview.connect_activate(clone!(
                #[weak(rename_to = imp)]
                self,
                move |listview, pos| {
                    let Some(model) = listview.model().and_downcast::<FixedSelection>() else {
                        return;
                    };
                    let Some(item) = model.item(pos) else {
                        return;
                    };

                    if let Some(section) = item.downcast_ref::<SidebarSection>() {
                        section.set_is_expanded(!section.is_expanded());
                    } else if let Some(room) = item
                        .downcast_ref::<Room>()
                        .filter(|room| room.category() == RoomCategory::Space)
                    {
                        // Spaces are folders of the room list: open them in the sidebar.
                        imp.enter_space(room);
                    } else {
                        model.set_selected(pos);
                    }
                }
            ));

            self.setup_space_rail();

            obj.property_expression("list-model")
                .chain_property::<SidebarListModel>("selection-model")
                .bind(&*self.listview, "model", None::<&glib::Object>);

            // FIXME: Remove this hack once https://gitlab.gnome.org/GNOME/gtk/-/issues/4938 is resolved
            self.scrolled_window
                .vscrollbar()
                .first_child()
                .unwrap()
                .set_overflow(gtk::Overflow::Hidden);

            // Use the built-in search-changed signal which is already
            // debounced by the search-delay property (default 150ms).
            self.room_search_entry.connect_search_changed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_search_filter();
                }
            ));
        }

        fn dispose(&self) {
            if let Some(binding) = self.space_name_binding.take() {
                binding.unbind();
            }

            if let Some(user) = self.user.take() {
                let session = user.session();
                if let Some(handler) = self.session_handler.take() {
                    session.disconnect(handler);
                }

                let security = session.security();
                for handler in self.security_handlers.take() {
                    security.disconnect(handler);
                }
            }
        }
    }

    impl WidgetImpl for Sidebar {
        fn grab_focus(&self) -> bool {
            if self.listview.grab_focus() {
                true
            } else {
                self.account_switcher_button.grab_focus()
            }
        }
    }

    impl NavigationPageImpl for Sidebar {}

    #[gtk::template_callbacks]
    impl Sidebar {
        /// Set the logged-in user.
        fn set_user(&self, user: Option<User>) {
            let prev_user = self.user.borrow().clone();
            if prev_user == user {
                return;
            }

            if let Some(user) = prev_user {
                let session = user.session();
                if let Some(handler) = self.session_handler.take() {
                    session.disconnect(handler);
                }

                let security = session.security();
                for handler in self.security_handlers.take() {
                    security.disconnect(handler);
                }
            }

            if let Some(user) = &user {
                let session = user.session();

                let offline_handler = session.connect_is_offline_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_security_banner();
                    }
                ));
                self.session_handler.replace(Some(offline_handler));

                let security = session.security();
                let crypto_identity_handler =
                    security.connect_crypto_identity_state_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_security_banner();
                        }
                    ));
                let verification_handler = security.connect_verification_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_security_banner();
                    }
                ));
                let recovery_handler = security.connect_recovery_state_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_security_banner();
                    }
                ));

                self.security_handlers.replace(vec![
                    crypto_identity_handler,
                    verification_handler,
                    recovery_handler,
                ]);
            }

            self.user.replace(user);

            self.update_security_banner();
            self.obj().notify_user();
        }

        /// Set the list model of the sidebar.
        fn set_list_model(&self, list_model: Option<&SidebarListModel>) {
            if self.list_model.upgrade().as_ref() == list_model {
                return;
            }
            let obj = self.obj();

            self.list_model.set(list_model);

            if let Some(list_model) = list_model {
                self.update_search_filter();

                self.update_room_stack(&list_model.selection_model());

                list_model.selection_model().connect_is_empty_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |list| {
                        imp.update_room_stack(list);
                    }
                ));

                self.set_space_list(Some(&list_model.item_list().space_list()));
            } else {
                self.set_space_list(None);
            }

            obj.notify_list_model();
        }

        /// Set up the space switcher.
        fn setup_space_rail(&self) {
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    error!("List item factory did not receive a list item: {item:?}");
                    return;
                };
                let row = SidebarSpaceRow::new();
                item.set_child(Some(&row));
                item.bind_property("item", &row, "item").build();
            });
            self.space_rail.set_factory(Some(&factory));

            self.space_rail.set_model(Some(&self.space_selection));

            // Activating an item filters the room list. Activating the selected
            // item goes back to the top of the space.
            self.space_rail.connect_activate(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, position| {
                    if let Some(space_list) = imp.space_list.obj() {
                        space_list.select(position);
                    }
                    imp.hide_space_rail_overlay();
                }
            ));

            // During a search, the results come from all the spaces.
            self.room_search.connect_search_mode_enabled_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_space_header();
                }
            ));

            self.space_split_view.connect_collapsed_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_space_rail_visibility();
                }
            ));
        }

        /// Set the list of items of the space switcher.
        fn set_space_list(&self, space_list: Option<&SidebarSpaceList>) {
            if self.space_list.obj().as_ref() == space_list {
                return;
            }

            self.space_list.disconnect_signals();
            self.space_selection
                .set_model(space_list.map(|list| list.clone().upcast::<gio::ListModel>()));

            if let Some(space_list) = space_list {
                let has_spaces_handler = space_list.connect_has_spaces_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_space_rail_visibility();
                    }
                ));
                let selected_position_handler =
                    space_list.connect_selected_position_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.sync_space_selection();
                        }
                    ));
                let current_space_handler = space_list.connect_current_space_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_space_header();
                    }
                ));
                let can_go_back_handler = space_list.connect_can_go_back_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |space_list| {
                        imp.space_back_button.set_visible(space_list.can_go_back());
                    }
                ));
                let items_changed_handler = space_list.connect_items_changed(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_, _, _, _| {
                        imp.sync_space_selection();
                    }
                ));

                self.space_list.set(
                    space_list,
                    vec![
                        has_spaces_handler,
                        selected_position_handler,
                        current_space_handler,
                        can_go_back_handler,
                        items_changed_handler,
                    ],
                );
                self.space_back_button.set_visible(space_list.can_go_back());
            }

            self.sync_space_selection();
            self.update_space_header();
            self.update_space_rail_visibility();
        }

        /// Synchronize the selection of the space switcher with the space
        /// list.
        fn sync_space_selection(&self) {
            let Some(space_list) = self.space_list.obj() else {
                return;
            };

            self.space_selection
                .set_selected(space_list.selected_position());
        }

        /// Update the visibility of the space switcher.
        ///
        /// It is only shown if the user has joined spaces. When the window is
        /// narrow, it is hidden in an overlay that can be toggled.
        fn update_space_rail_visibility(&self) {
            let has_spaces = self.space_list.obj().is_some_and(|list| list.has_spaces());
            let collapsed = self.space_split_view.is_collapsed();

            self.space_switcher_button
                .set_visible(has_spaces && collapsed);
            self.space_split_view
                .set_show_sidebar(has_spaces && !collapsed);
            // Do not allow to swipe in an empty space switcher.
            self.space_split_view.set_enable_show_gesture(has_spaces);

            if self.has_spaces.get() != has_spaces {
                self.has_spaces.set(has_spaces);
                self.obj().notify_has_spaces();
            }
        }

        /// Hide the space switcher if it is shown as an overlay.
        fn hide_space_rail_overlay(&self) {
            if self.space_split_view.is_collapsed() {
                self.space_split_view.set_show_sidebar(false);
            }
        }

        /// Update the header showing the selected space.
        fn update_space_header(&self) {
            if let Some(binding) = self.space_name_binding.take() {
                binding.unbind();
            }

            let space = self.space_list.obj().and_then(|list| list.current_space());

            if let Some(space) = &space {
                let binding = space
                    .bind_property("display-name", &*self.space_header_name, "label")
                    .sync_create()
                    .build();
                self.space_name_binding.replace(Some(binding));
            }

            let avatar_data = space.as_ref().map(PillSourceExt::avatar_data);
            self.space_header_avatar.set_data(avatar_data.as_ref());
            self.space_header
                .set_visible(space.is_some() && !self.room_search.is_search_mode());
        }

        /// Navigate to the given space in the sidebar.
        fn enter_space(&self, space: &Room) {
            let Some(space_list) = self.space_list.obj() else {
                return;
            };

            // Show the content of the space instead of the search results.
            self.room_search.set_search_mode(false);
            space_list.enter_space(space);
            self.scrolled_window.vadjustment().set_value(0.0);
        }

        /// Go back to the parent of the selected subspace.
        #[template_callback]
        fn go_back_to_parent_space(&self) {
            if let Some(space_list) = self.space_list.obj() {
                space_list.go_back();
            }
        }

        /// Show the overview of the selected space.
        #[template_callback]
        fn show_space_overview(&self) {
            let Some(space) = self.space_list.obj().and_then(|list| list.current_space()) else {
                return;
            };

            if self
                .obj()
                .activate_action(
                    "session.show-room",
                    Some(&space.room_id().as_str().to_variant()),
                )
                .is_err()
            {
                error!("Could not activate action `session.show-room`");
            }
        }

        /// Update the search filter with the current search text.
        ///
        /// Called when the search term changes and when the list model is set.
        fn update_search_filter(&self) {
            let Some(list_model) = self.list_model.upgrade() else {
                return;
            };

            let text = self.room_search_entry.text();
            let normalized = secular::normalized_lower_lay_string(&text);
            list_model.string_filter().set_search(Some(&normalized));
        }

        /// The current session, if any.
        fn session(&self) -> Option<Session> {
            self.user.borrow().as_ref().map(User::session)
        }

        /// Switch between room list and placeholder status page
        /// depending on if the users search has any results or not
        fn update_room_stack(&self, list: &FixedSelection) {
            self.room_stack.set_visible_child_name(if list.is_empty() {
                "no-results"
            } else {
                "room-list"
            });
        }

        /// Update the security banner.
        fn update_security_banner(&self) {
            let Some(session) = self.session() else {
                return;
            };

            if session.is_offline() {
                // Only show one banner at a time.
                // The user will not be able to solve security issues while offline anyway.
                self.security_banner.set_revealed(false);
                return;
            }

            let security = session.security();
            let crypto_identity_state = security.crypto_identity_state();
            let verification_state = security.verification_state();
            let recovery_state = security.recovery_state();

            if crypto_identity_state == CryptoIdentityState::Unknown
                || verification_state == SessionVerificationState::Unknown
                || recovery_state == RecoveryState::Unknown
            {
                // Do not show the banner prematurely, unknown states should solve themselves.
                self.security_banner.set_revealed(false);
                return;
            }

            if verification_state == SessionVerificationState::Verified
                && recovery_state == RecoveryState::Enabled
            {
                // No need for the banner.
                self.security_banner.set_revealed(false);
                return;
            }

            let (title, button) = if crypto_identity_state == CryptoIdentityState::Missing {
                (gettext("No crypto identity"), gettext("Enable"))
            } else if verification_state == SessionVerificationState::Unverified {
                (gettext("Crypto identity incomplete"), gettext("Verify"))
            } else {
                match recovery_state {
                    RecoveryState::Disabled => {
                        (gettext("Account recovery disabled"), gettext("Enable"))
                    }
                    RecoveryState::Incomplete => {
                        (gettext("Account recovery incomplete"), gettext("Recover"))
                    }
                    _ => unreachable!(),
                }
            };

            self.security_banner.set_title(&title);
            self.security_banner.set_button_label(Some(&button));
            self.security_banner.set_revealed(true);
        }

        /// Set the category of the source that activated drop mode.
        pub(super) fn set_drop_source_category(&self, source_category: Option<RoomCategory>) {
            if self.drop_source_category.get() == source_category {
                return;
            }

            self.drop_source_category.set(source_category);

            if source_category.is_some() {
                self.listview.add_css_class("drop-mode");
            } else {
                self.listview.remove_css_class("drop-mode");
            }

            let Some(item_list) = self.list_model.upgrade().map(|model| model.item_list()) else {
                return;
            };

            item_list.set_show_all_for_room_category(source_category);
            self.obj()
                .emit_by_name::<()>("drop-source-category-changed", &[]);
        }

        /// The shared popover for a room row in the sidebar.
        pub(super) fn room_row_popover(&self) -> &gtk::PopoverMenu {
            self.room_row_popover.get_or_init(|| {
                let popover = gtk::PopoverMenu::builder()
                    .menu_model(&*self.room_row_menu)
                    .has_arrow(false)
                    .halign(gtk::Align::Start)
                    .build();
                popover
                    .update_property(&[gtk::accessible::Property::Label(&gettext("Context Menu"))]);

                popover
            })
        }

        /// Scroll to the currently selected item of the sidebar.
        pub(super) fn scroll_to_selection(&self) {
            let Some(list_model) = self.list_model.upgrade() else {
                return;
            };

            let selected = list_model.selection_model().selected();

            if selected != gtk::INVALID_LIST_POSITION {
                self.listview
                    .scroll_to(selected, gtk::ListScrollFlags::FOCUS, None);
            }
        }

        /// Open the proper security flow to fix the current issue.
        #[template_callback]
        fn fix_security_issue(&self) {
            let Some(session) = self.session() else {
                return;
            };

            let dialog = AccountSettings::new(&session);

            // Show the encryption tab if the user uses the back button.
            dialog.show_encryption_tab();

            let security = session.security();
            let crypto_identity_state = security.crypto_identity_state();
            let verification_state = security.verification_state();

            let subpage = if crypto_identity_state == CryptoIdentityState::Missing
                || verification_state == SessionVerificationState::Unverified
            {
                AccountSettingsSubpage::CryptoIdentitySetup
            } else {
                AccountSettingsSubpage::RecoverySetup
            };
            dialog.show_subpage(subpage);

            dialog.present(Some(&*self.obj()));
        }

        /// Select the first room found in search result,
        /// or noop if no search term has been entered.
        #[template_callback]
        fn activate_first_search_result(&self) {
            if self.room_search_entry.text().is_empty() {
                return;
            }

            let _ = self
                .listview
                .activate_action("list.activate-item", Some(&0u32.into()));
        }
    }
}

glib::wrapper! {
    /// The sidebar of the session view, displaying the list of rooms
    /// available for the current session, among other things.
    pub struct Sidebar(ObjectSubclass<imp::Sidebar>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[gtk::template_callbacks]
impl Sidebar {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The search bar allowing to filter rooms in the sidebar.
    pub(crate) fn room_search_bar(&self) -> gtk::SearchBar {
        self.imp().room_search.clone()
    }

    /// The category of the source that activated drop mode.
    fn drop_source_category(&self) -> Option<RoomCategory> {
        self.imp().drop_source_category.get()
    }

    /// Set the category of the source that activated drop mode.
    fn set_drop_source_category(&self, source_category: Option<RoomCategory>) {
        self.imp().set_drop_source_category(source_category);
    }

    /// The category of the drop target that is currently hovered.
    fn drop_active_target_category(&self) -> Option<TargetRoomCategory> {
        self.imp().drop_active_target_category.get()
    }

    /// Set the category of the drop target that is currently hovered.
    fn set_drop_active_target_category(&self, target_category: Option<TargetRoomCategory>) {
        if self.drop_active_target_category() == target_category {
            return;
        }

        self.imp().drop_active_target_category.set(target_category);
        self.emit_by_name::<()>("drop-active-target-category-changed", &[]);
    }

    /// The shared popover for a room row in the sidebar.
    fn room_row_popover(&self) -> &gtk::PopoverMenu {
        self.imp().room_row_popover()
    }

    /// The `AdwHeaderBar` of the sidebar.
    pub(crate) fn header_bar(&self) -> &adw::HeaderBar {
        &self.imp().header_bar
    }

    /// Scroll to the currently selected item of the sidebar.
    pub(crate) fn scroll_to_selection(&self) {
        self.imp().scroll_to_selection();
    }

    /// Connect to the signal emitted when the drop source category changed.
    pub fn connect_drop_source_category_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "drop-source-category-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }

    /// Connect to the signal emitted when the drop active target category
    /// changed.
    pub fn connect_drop_active_target_category_changed<F: Fn(&Self) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "drop-active-target-category-changed",
            true,
            closure_local!(move |obj: Self| {
                f(&obj);
            }),
        )
    }
}
