use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    components::{Avatar, LoadingButton},
    session::{Room, RoomCategory, TargetRoomCategory},
    toast,
    utils::matrix::MatrixIdUri,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/invite_request.ui")]
    #[properties(wrapper_type = super::InviteRequest)]
    pub struct InviteRequest {
        #[template_child]
        pub(super) header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        room_alias: TemplateChild<gtk::Label>,
        #[template_child]
        room_topic: TemplateChild<gtk::Label>,
        #[template_child]
        retract_button: TemplateChild<LoadingButton>,
        /// The room currently displayed.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: RefCell<Option<Room>>,
        category_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for InviteRequest {
        const NAME: &'static str = "ContentInviteRequest";
        type Type = super::InviteRequest;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for InviteRequest {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.room_alias.connect_label_notify(|room_alias| {
                room_alias.set_visible(!room_alias.label().is_empty());
            });
            self.room_alias
                .set_visible(!self.room_alias.label().is_empty());

            self.room_topic.connect_label_notify(|room_topic| {
                room_topic.set_visible(!room_topic.label().is_empty());
            });
            self.room_topic
                .set_visible(!self.room_topic.label().is_empty());
            self.room_topic.connect_activate_link(clone!(
                #[weak]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, uri| {
                    if MatrixIdUri::parse(uri).is_ok() {
                        let _ =
                            obj.activate_action("session.show-matrix-uri", Some(&uri.to_variant()));
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
            ));
        }

        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for InviteRequest {
        fn grab_focus(&self) -> bool {
            self.retract_button.grab_focus()
        }
    }

    impl BinImpl for InviteRequest {}

    #[gtk::template_callbacks]
    impl InviteRequest {
        /// Set the room currently displayed.
        fn set_room(&self, room: Option<Room>) {
            if *self.room.borrow() == room {
                return;
            }

            self.disconnect_signals();

            if let Some(room) = &room {
                let category_handler = room.connect_category_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |room| {
                        let category = room.category();

                        if category == RoomCategory::Left {
                            // We retracted the request or the request was denied, we should close
                            // the room if it is opened.
                            let Some(session) = room.session() else {
                                return;
                            };
                            let selection = session.sidebar_list_model().selection_model();
                            if selection
                                .selected_item()
                                .and_downcast::<Room>()
                                .is_some_and(|selected_room| selected_room == *room)
                            {
                                selection.set_selected_item(None::<glib::Object>);
                            }
                        }

                        if category != RoomCategory::Knocked {
                            imp.retract_button.set_is_loading(false);

                            if let Some(category_handler) = imp.category_handler.take() {
                                room.disconnect(category_handler);
                            }
                        }
                    }
                ));
                self.category_handler.replace(Some(category_handler));
            }

            self.room.replace(room);

            self.obj().notify_room();
        }

        /// Retract the request.
        #[template_callback]
        async fn retract(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            self.retract_button.set_is_loading(true);

            if room
                .change_category(TargetRoomCategory::Left)
                .await
                .is_err()
            {
                toast!(self.obj(), gettext("Could not retract invite request",),);

                self.retract_button.set_is_loading(false);
            }
        }

        /// Disconnect the signal handlers of this view.
        fn disconnect_signals(&self) {
            if let Some(room) = self.room.take()
                && let Some(handler) = self.category_handler.take()
            {
                room.disconnect(handler);
            }
        }
    }
}

glib::wrapper! {
    /// A view presenting an invitate request to a room.
    pub struct InviteRequest(ObjectSubclass<imp::InviteRequest>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl InviteRequest {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The header bar of the invite request.
    pub fn header_bar(&self) -> &adw::HeaderBar {
        &self.imp().header_bar
    }
}
