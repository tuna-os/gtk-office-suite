// FIXME: AdwPreferencesWindow is deprecated but we cannot use
// AdwPreferencesDialog yet because we need to be able to open the media viewer.
#![allow(deprecated)]

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use ruma::UserId;

mod addresses_subpage;
mod edit_details_subpage;
mod general_page;
mod history_viewer;
mod history_visibility_subpage;
mod invite_subpage;
mod join_rule_subpage;
mod member_row;
mod members_page;
mod membership_subpage_item;
mod permissions;
mod upgrade_dialog;

use self::{
    addresses_subpage::AddressesSubpage,
    edit_details_subpage::EditDetailsSubpage,
    general_page::GeneralPage,
    history_viewer::{
        AudioHistoryViewer, FileHistoryViewer, HistoryViewerTimeline, VisualMediaHistoryViewer,
    },
    history_visibility_subpage::HistoryVisibilitySubpage,
    invite_subpage::InviteSubpage,
    join_rule_subpage::JoinRuleSubpage,
    member_row::MemberRow,
    members_page::MembersPage,
    membership_subpage_item::MembershipSubpageItem,
    permissions::PermissionsSubpage,
    upgrade_dialog::{UpgradeDialog, UpgradeInfo},
};
use crate::{
    components::UserPage,
    session::{MemberList, MembershipListKind, Room},
    toast,
};

/// The possible subpages of the room details.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy, glib::Variant)]
pub(super) enum SubpageName {
    /// The page to edit the name, topic and avatar of the room.
    EditDetails,
    /// The list of members of the room.
    Members,
    /// The form to invite new members.
    Invite,
    /// The history of visual media.
    VisualMediaHistory,
    /// The history of files.
    FileHistory,
    /// The history of audio.
    AudioHistory,
    /// The page to edit the public addresses of the room.
    Addresses,
    /// The page to edit the permissions of the room.
    Permissions,
    /// The page to edit the join rule of the room.
    JoinRule,
    /// The page to edit the history visibility of the room.
    HistoryVisibility,
}

/// The view to present when opening the room details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InitialView {
    /// Present the default page.
    None,
    /// Present the given subpage.
    Subpage(SubpageName),
    /// Present the members subpage with the given kind.
    Members(MembershipListKind),
}

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        collections::HashMap,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/mod.ui")]
    #[properties(wrapper_type = super::RoomDetails)]
    pub struct RoomDetails {
        /// The room to show the details for.
        #[property(get, set = Self::set_room, construct_only)]
        room: OnceCell<Room>,
        /// The list of members in the room.
        #[property(get)]
        members: OnceCell<MemberList>,
        /// The timeline for the history viewers.
        #[property(get)]
        timeline: OnceCell<HistoryViewerTimeline>,
        /// The general page.
        general_page: OnceCell<GeneralPage>,
        /// The subpages that are loaded.
        ///
        /// We keep them around to avoid reloading them if the user reopens the
        /// same subpage.
        subpages: RefCell<HashMap<SubpageName, adw::NavigationPage>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomDetails {
        const NAME: &'static str = "RoomDetails";
        type Type = super::RoomDetails;
        type ParentType = adw::PreferencesWindow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action(
                "details.show-subpage",
                Some(&SubpageName::static_variant_type()),
                |obj, _, param| {
                    let subpage = param
                        .and_then(glib::Variant::get::<SubpageName>)
                        .expect("parameter should be a valid subpage name");

                    obj.imp().show_subpage(subpage, false);
                },
            );

            klass.install_action(
                "details.show-member",
                Some(&String::static_variant_type()),
                |obj, _, param| {
                    let Some(user_id) = param
                        .and_then(glib::Variant::get::<String>)
                        .and_then(|s| UserId::parse(s).ok())
                    else {
                        return;
                    };

                    let member = obj.members().get_or_create(user_id);
                    let user_page = UserPage::new(&member);
                    user_page.connect_close(clone!(
                        #[weak]
                        obj,
                        move |_| {
                            obj.pop_subpage();
                            toast!(
                                obj,
                                gettext("The user is not in the room members list anymore"),
                            );
                        }
                    ));

                    obj.push_subpage(&user_page);
                },
            );

            klass.install_action("win.toggle-fullscreen", None, |obj, _, _| {
                if obj.is_fullscreen() {
                    obj.unfullscreen();
                } else {
                    obj.fullscreen();
                }
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoomDetails {}

    impl WidgetImpl for RoomDetails {}
    impl WindowImpl for RoomDetails {}
    impl AdwWindowImpl for RoomDetails {}
    impl PreferencesWindowImpl for RoomDetails {}

    impl RoomDetails {
        /// Set the room to show the details for.
        fn set_room(&self, room: Room) {
            let room = self.room.get_or_init(|| room);

            // Initialize the media history viewers timeline.
            self.timeline
                .set(HistoryViewerTimeline::new(room))
                .expect("timeline should be uninitialized");

            // Keep a strong reference to members list.
            self.members
                .set(room.get_or_create_members())
                .expect("members should be uninitialized");

            // Initialize the general page.
            let general_page = self
                .general_page
                .get_or_init(|| GeneralPage::new(room, self.members()));
            self.obj().add(general_page);
        }

        /// The list of members in the room.
        fn members(&self) -> &MemberList {
            self.members.get().expect("members should be initialized")
        }

        /// The timeline for the history viewers.
        fn timeline(&self) -> &HistoryViewerTimeline {
            self.timeline.get().expect("timeline should be initialized")
        }

        /// Get the given subpage.
        fn subpage(&self, name: SubpageName) -> adw::NavigationPage {
            let room = self.room.get().expect("room should be initialized");

            self.subpages
                .borrow_mut()
                .entry(name)
                .or_insert_with(|| match name {
                    SubpageName::EditDetails => EditDetailsSubpage::new(room).upcast(),
                    SubpageName::Members => MembersPage::new(room, self.members()).upcast(),
                    SubpageName::Invite => InviteSubpage::new(room).upcast(),
                    SubpageName::VisualMediaHistory => {
                        VisualMediaHistoryViewer::new(self.timeline()).upcast()
                    }
                    SubpageName::FileHistory => FileHistoryViewer::new(self.timeline()).upcast(),
                    SubpageName::AudioHistory => AudioHistoryViewer::new(self.timeline()).upcast(),
                    SubpageName::Addresses => AddressesSubpage::new(room).upcast(),
                    SubpageName::Permissions => {
                        PermissionsSubpage::new(&room.permissions()).upcast()
                    }
                    SubpageName::JoinRule => JoinRuleSubpage::new(room).upcast(),
                    SubpageName::HistoryVisibility => HistoryVisibilitySubpage::new(room).upcast(),
                })
                .clone()
        }

        /// Show the subpage with the given name.
        pub(super) fn show_subpage(&self, name: SubpageName, is_initial: bool) {
            let subpage = self.subpage(name);

            if is_initial {
                subpage.set_can_pop(false);
            }

            self.obj().push_subpage(&subpage);
        }

        /// Show the members subpage with the given kind.
        fn show_members_subpage(&self, kind: MembershipListKind) {
            let subpage = self
                .subpage(SubpageName::Members)
                .downcast::<MembersPage>()
                .expect("we should have the members subpage");

            subpage.show_membership_list(kind);

            self.obj().push_subpage(&subpage);
        }

        /// Show the given initial view.
        pub(super) fn show_initial_view(&self, initial_view: InitialView) {
            match initial_view {
                InitialView::None => {}
                InitialView::Subpage(name) => {
                    self.show_subpage(name, true);
                }
                InitialView::Members(kind) => {
                    self.show_members_subpage(kind);
                }
            }
        }
    }
}

glib::wrapper! {
    /// Preference Window to display and update room details.
    pub struct RoomDetails(ObjectSubclass<imp::RoomDetails>)
        @extends gtk::Widget, gtk::Window, adw::Window, adw::PreferencesWindow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Root, gtk::Native,
                    gtk::ShortcutManager;
}

impl RoomDetails {
    /// Construct a `RoomDetails` for the given room with the given parent
    /// window, showing the given initial view.
    pub(super) fn new(
        parent_window: Option<&gtk::Window>,
        room: &Room,
        initial_view: InitialView,
    ) -> Self {
        let obj = glib::Object::builder::<Self>()
            .property("transient-for", parent_window)
            .property("room", room)
            .build();

        obj.imp().show_initial_view(initial_view);

        obj
    }
}
