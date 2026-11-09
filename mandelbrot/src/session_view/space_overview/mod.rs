use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

mod child_row;
mod subpage;

use self::subpage::SpaceOverviewSubpage;
use crate::session::{Room, SpaceHierarchy, SpaceHierarchyChild};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/space_overview/mod.ui")]
    #[properties(wrapper_type = super::SpaceOverview)]
    pub struct SpaceOverview {
        #[template_child]
        nav_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub(super) root_page: TemplateChild<SpaceOverviewSubpage>,
        /// The space currently displayed.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: glib::WeakRef<Room>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SpaceOverview {
        const NAME: &'static str = "ContentSpaceOverview";
        type Type = super::SpaceOverview;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SpaceOverview {}

    impl WidgetImpl for SpaceOverview {}
    impl BinImpl for SpaceOverview {}

    impl SpaceOverview {
        /// Set the space currently displayed.
        fn set_room(&self, room: Option<&Room>) {
            if self.room.upgrade().as_ref() == room {
                return;
            }

            // Go back to the root page.
            self.nav_view
                .replace(&[self.root_page.get().upcast::<adw::NavigationPage>()]);

            let hierarchy = room.and_then(|room| {
                let session = room.session()?;
                Some(SpaceHierarchy::new(&session, room.room_id().to_owned()))
            });
            self.root_page.set_hierarchy(hierarchy);

            self.room.set(room);
            self.obj().notify_room();
        }

        /// Push a page presenting the hierarchy of the given space.
        pub(super) fn push_space(&self, space: &SpaceHierarchyChild) {
            let Some(session) = space.session() else {
                return;
            };

            let hierarchy = SpaceHierarchy::new(&session, space.room_id().clone());
            let subpage = SpaceOverviewSubpage::new(&hierarchy);
            self.nav_view.push(&subpage);
        }
    }
}

glib::wrapper! {
    /// A view presenting the hierarchy of the selected space.
    pub struct SpaceOverview(ObjectSubclass<imp::SpaceOverview>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SpaceOverview {
    /// Push a page presenting the hierarchy of the given space.
    pub(crate) fn push_space(&self, space: &SpaceHierarchyChild) {
        self.imp().push_space(space);
    }

    /// The header bar of the root page of this view.
    pub(crate) fn header_bar(&self) -> &adw::HeaderBar {
        self.imp().root_page.header_bar()
    }
}
