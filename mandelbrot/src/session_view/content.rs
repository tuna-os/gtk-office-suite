use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use super::{Explore, Invite, InviteRequest, RoomHistory, SpaceOverview};
use crate::{
    identity_verification_view::IdentityVerificationView,
    session::{
        IdentityVerification, Room, RoomCategory, Session, SidebarIconItem, SidebarIconItemType,
    },
    utils::BoundObject,
};

/// A page of the content stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentPage {
    /// The placeholder page when no content is presented.
    Empty,
    /// The history of the selected room.
    RoomHistory,
    /// The selected invite request.
    InviteRequest,
    /// The selected room invite.
    Invite,
    /// The explore page.
    Explore,
    /// The selected identity verification.
    Verification,
    /// The overview of the selected space.
    SpaceOverview,
}

impl ContentPage {
    /// The name of this page.
    const fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::RoomHistory => "room-history",
            Self::InviteRequest => "invite-request",
            Self::Invite => "invite",
            Self::Explore => "explore",
            Self::Verification => "verification",
            Self::SpaceOverview => "space-overview",
        }
    }

    /// Get the page matching the given name.
    ///
    /// Panics if the name does not match any of the variants.
    fn from_name(name: &str) -> Self {
        match name {
            "empty" => Self::Empty,
            "room-history" => Self::RoomHistory,
            "invite-request" => Self::InviteRequest,
            "invite" => Self::Invite,
            "explore" => Self::Explore,
            "verification" => Self::Verification,
            "space-overview" => Self::SpaceOverview,
            _ => panic!("Unknown ContentPage: {name}"),
        }
    }
}

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/content.ui")]
    #[properties(wrapper_type = super::Content)]
    pub struct Content {
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        room_history: TemplateChild<RoomHistory>,
        #[template_child]
        invite_request: TemplateChild<InviteRequest>,
        #[template_child]
        invite: TemplateChild<Invite>,
        #[template_child]
        explore: TemplateChild<Explore>,
        #[template_child]
        space_overview: TemplateChild<SpaceOverview>,
        #[template_child]
        empty_page: TemplateChild<adw::ToolbarView>,
        #[template_child]
        empty_page_header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        verification_page: TemplateChild<adw::ToolbarView>,
        #[template_child]
        verification_page_header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        identity_verification_widget: TemplateChild<IdentityVerificationView>,
        /// The current session.
        #[property(get, set = Self::set_session, explicit_notify, nullable)]
        session: glib::WeakRef<Session>,
        /// Whether this is the only visible view, i.e. there is no sidebar.
        #[property(get, set)]
        only_view: Cell<bool>,
        item_binding: RefCell<Option<glib::Binding>>,
        /// The item currently displayed.
        #[property(get, set = Self::set_item, explicit_notify, nullable)]
        item: BoundObject<glib::Object>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Content {
        const NAME: &'static str = "Content";
        type Type = super::Content;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Content {
        fn constructed(&self) {
            self.parent_constructed();

            self.stack.connect_visible_child_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    if imp.visible_page() != ContentPage::Verification {
                        imp.identity_verification_widget
                            .set_verification(None::<IdentityVerification>);
                    }
                    if imp.visible_page() != ContentPage::SpaceOverview {
                        imp.space_overview.set_room(None::<Room>);
                    }
                }
            ));
        }

        fn dispose(&self) {
            if let Some(binding) = self.item_binding.take() {
                binding.unbind();
            }
        }
    }

    impl WidgetImpl for Content {}

    impl NavigationPageImpl for Content {
        fn hidden(&self) {
            self.obj().set_item(None::<glib::Object>);
        }
    }

    impl Content {
        /// The visible page of the content.
        pub(super) fn visible_page(&self) -> ContentPage {
            ContentPage::from_name(
                &self
                    .stack
                    .visible_child_name()
                    .expect("Content stack should always have a visible child name"),
            )
        }

        /// Set the visible page of the content.
        fn set_visible_page(&self, page: ContentPage) {
            if self.visible_page() == page {
                return;
            }

            self.stack.set_visible_child_name(page.name());
        }

        /// Set the current session.
        fn set_session(&self, session: Option<&Session>) {
            if session == self.session.upgrade().as_ref() {
                return;
            }
            let obj = self.obj();

            if let Some(binding) = self.item_binding.take() {
                binding.unbind();
            }

            if let Some(session) = session {
                let item_binding = session
                    .sidebar_list_model()
                    .selection_model()
                    .bind_property("selected-item", &*obj, "item")
                    .sync_create()
                    .bidirectional()
                    .build();

                self.item_binding.replace(Some(item_binding));
            }

            self.session.set(session);
            obj.notify_session();
        }

        /// Set the item currently displayed.
        fn set_item(&self, item: Option<glib::Object>) {
            if self.item.obj() == item {
                return;
            }

            self.item.disconnect_signals();

            if let Some(item) = item {
                let handler = if let Some(room) = item.downcast_ref::<Room>() {
                    let category_handler = room.connect_category_notify(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.update_visible_child();
                        }
                    ));

                    Some(category_handler)
                } else if let Some(verification) = item.downcast_ref::<IdentityVerification>() {
                    let dismiss_handler = verification.connect_dismiss(clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move |_| {
                            imp.set_item(None);
                        }
                    ));

                    Some(dismiss_handler)
                } else {
                    None
                };

                self.item.set(item, handler.into_iter().collect());
            }

            self.update_visible_child();
            self.obj().notify_item();

            if let Some(page) = self.stack.visible_child() {
                page.grab_focus();
            }
        }

        /// Update the visible child according to the current item.
        fn update_visible_child(&self) {
            let Some(item) = self.item.obj() else {
                self.set_visible_page(ContentPage::Empty);
                return;
            };

            if let Some(room) = item.downcast_ref::<Room>() {
                match room.category() {
                    RoomCategory::Knocked => {
                        self.invite_request.set_room(Some(room.clone()));
                        self.set_visible_page(ContentPage::InviteRequest);
                    }
                    RoomCategory::Invited => {
                        self.invite.set_room(Some(room.clone()));
                        self.set_visible_page(ContentPage::Invite);
                    }
                    RoomCategory::Space => {
                        self.space_overview.set_room(Some(room.clone()));
                        self.set_visible_page(ContentPage::SpaceOverview);
                    }
                    _ => {
                        self.room_history.set_timeline(Some(room.live_timeline()));
                        self.set_visible_page(ContentPage::RoomHistory);
                    }
                }
            } else if item
                .downcast_ref::<SidebarIconItem>()
                .is_some_and(|i| i.item_type() == SidebarIconItemType::Explore)
            {
                self.set_visible_page(ContentPage::Explore);
            } else if let Some(verification) = item.downcast_ref::<IdentityVerification>() {
                self.identity_verification_widget
                    .set_verification(Some(verification.clone()));
                self.set_visible_page(ContentPage::Verification);
            }
        }

        /// Handle a paste action.
        pub(super) fn handle_paste_action(&self) {
            if self.visible_page() == ContentPage::RoomHistory {
                self.room_history.handle_paste_action();
            }
        }

        /// All the header bars of the children of the content.
        pub(super) fn header_bars(&self) -> [&adw::HeaderBar; 7] {
            [
                &self.empty_page_header_bar,
                self.room_history.header_bar(),
                self.invite_request.header_bar(),
                self.invite.header_bar(),
                self.explore.header_bar(),
                &self.verification_page_header_bar,
                self.space_overview.header_bar(),
            ]
        }
    }
}

glib::wrapper! {
    /// A view displaying the selected content in the sidebar.
    pub struct Content(ObjectSubclass<imp::Content>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Content {
    pub fn new(session: &Session) -> Self {
        glib::Object::builder().property("session", session).build()
    }

    /// Handle a paste action.
    pub(crate) fn handle_paste_action(&self) {
        self.imp().handle_paste_action();
    }

    /// All the header bars of the children of the content.
    pub(crate) fn header_bars(&self) -> [&adw::HeaderBar; 7] {
        self.imp().header_bars()
    }
}
