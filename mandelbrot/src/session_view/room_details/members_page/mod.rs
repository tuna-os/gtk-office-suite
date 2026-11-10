use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

mod members_list_view;

use self::members_list_view::MembersListView;
use crate::session::{MemberList, MembershipListKind, Room};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/members_page/mod.ui"
    )]
    #[properties(wrapper_type = super::MembersPage)]
    pub struct MembersPage {
        #[template_child]
        navigation_view: TemplateChild<adw::NavigationView>,
        /// The room containing the members.
        #[property(get, construct_only)]
        room: glib::WeakRef<Room>,
        /// The lists of members in the room.
        #[property(get, construct_only)]
        members: glib::WeakRef<MemberList>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MembersPage {
        const NAME: &'static str = "MembersPage";
        type Type = super::MembersPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action(
                "members.show-membership-list",
                Some(&MembershipListKind::static_variant_type()),
                |obj, _, param| {
                    let Some(kind) = param.and_then(glib::Variant::get::<MembershipListKind>)
                    else {
                        return;
                    };

                    obj.imp().show_membership_list(kind);
                },
            );
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MembersPage {
        fn constructed(&self) {
            self.parent_constructed();

            // Initialize the first page.
            self.show_membership_list(MembershipListKind::Join);
        }
    }

    impl WidgetImpl for MembersPage {}
    impl NavigationPageImpl for MembersPage {}

    impl MembersPage {
        /// Show the subpage for the list with the given membership.
        pub(super) fn show_membership_list(&self, kind: MembershipListKind) {
            let tag = kind.tag();

            if self.navigation_view.find_page(tag).is_some() {
                self.navigation_view.push_by_tag(tag);
                return;
            }

            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Some(members) = self.members.upgrade() else {
                return;
            };

            let subpage = MembersListView::new(&room, &members, kind);
            self.navigation_view.push(&subpage);
        }
    }
}

glib::wrapper! {
    /// A page showing the members of a room.
    pub struct MembersPage(ObjectSubclass<imp::MembersPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MembersPage {
    /// Construct a `MembersPage` for the given room and members list.
    pub fn new(room: &Room, members: &MemberList) -> Self {
        glib::Object::builder()
            .property("room", room)
            .property("members", members)
            .build()
    }

    /// Show the subpage for the list with the given membership.
    pub(super) fn show_membership_list(&self, kind: MembershipListKind) {
        self.imp().show_membership_list(kind);
    }
}
