use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    components::{
        Avatar, AvatarImageSafetySetting, LabelWithWidgets, LoadingButton, Pill,
        confirm_leave_room_dialog,
    },
    gettext_f,
    prelude::*,
    session::{MemberList, Room, RoomCategory, TargetRoomCategory, User},
    toast,
    utils::matrix::MatrixIdUri,
};

mod imp {
    use std::{cell::RefCell, collections::HashSet};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/invite.ui")]
    #[properties(wrapper_type = super::Invite)]
    pub struct Invite {
        #[template_child]
        pub(super) header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        room_alias: TemplateChild<gtk::Label>,
        #[template_child]
        room_topic: TemplateChild<gtk::Label>,
        #[template_child]
        inviter: TemplateChild<LabelWithWidgets>,
        #[template_child]
        accept_button: TemplateChild<LoadingButton>,
        #[template_child]
        decline_button: TemplateChild<LoadingButton>,
        /// The room currently displayed.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: RefCell<Option<Room>>,
        /// The list of members in the room.
        room_members: RefCell<Option<MemberList>>,
        /// The rooms that are currently being accepted.
        accept_requests: RefCell<HashSet<Room>>,
        /// The rooms that are currently being declined.
        decline_requests: RefCell<HashSet<Room>>,
        category_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Invite {
        const NAME: &'static str = "ContentInvite";
        type Type = super::Invite;
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
    impl ObjectImpl for Invite {
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

    impl WidgetImpl for Invite {
        fn grab_focus(&self) -> bool {
            self.accept_button.grab_focus()
        }
    }

    impl BinImpl for Invite {}

    #[gtk::template_callbacks]
    impl Invite {
        /// Set the room currently displayed.
        fn set_room(&self, room: Option<Room>) {
            if *self.room.borrow() == room {
                return;
            }

            self.disconnect_signals();

            match &room {
                Some(room) if self.accept_requests.borrow().contains(room) => {
                    self.decline_button.set_is_loading(false);
                    self.decline_button.set_sensitive(false);
                    self.accept_button.set_is_loading(true);
                }
                Some(room) if self.decline_requests.borrow().contains(room) => {
                    self.accept_button.set_is_loading(false);
                    self.accept_button.set_sensitive(false);
                    self.decline_button.set_is_loading(true);
                }
                _ => self.reset(),
            }

            if let Some(room) = &room {
                let category_handler = room.connect_category_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |room| {
                        let category = room.category();

                        if category == RoomCategory::Left {
                            // We declined the invite or the invite was retracted, we should close
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

                        if category != RoomCategory::Invited {
                            imp.decline_requests.borrow_mut().remove(room);
                            imp.accept_requests.borrow_mut().remove(room);
                            imp.reset();

                            if let Some(category_handler) = imp.category_handler.take() {
                                room.disconnect(category_handler);
                            }
                        }
                    }
                ));
                self.category_handler.replace(Some(category_handler));

                if let Some(inviter) = room.inviter() {
                    let pill = Pill::new(
                        &inviter,
                        AvatarImageSafetySetting::InviteAvatars,
                        Some(room.clone()),
                    );

                    let label = gettext_f(
                        // Translators: Do NOT translate the content between '{' and '}', these
                        // are variable names.
                        "{user_name} ({user_id}) invited you",
                        &[
                            ("user_name", LabelWithWidgets::PLACEHOLDER),
                            ("user_id", inviter.user_id().as_str()),
                        ],
                    );

                    self.inviter
                        .set_label_and_widgets(label, vec![pill.clone()]);
                }
            }

            // Keep a strong reference to the members list.
            self.room_members
                .replace(room.as_ref().map(Room::get_or_create_members));
            self.room.replace(room);

            self.obj().notify_room();
        }

        /// Reset the state of the view.
        fn reset(&self) {
            self.accept_button.set_is_loading(false);
            self.accept_button.set_sensitive(true);

            self.decline_button.set_is_loading(false);
            self.decline_button.set_sensitive(true);
        }

        /// Accept the invite.
        #[template_callback]
        async fn accept(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            self.decline_button.set_sensitive(false);
            self.accept_button.set_is_loading(true);
            self.accept_requests.borrow_mut().insert(room.clone());

            if room
                .change_category(TargetRoomCategory::Normal)
                .await
                .is_err()
            {
                toast!(
                    self.obj(),
                    gettext(
                        // Translators: Do NOT translate the content between '{' and '}', this
                        // is a variable name.
                        "Could not accept invitation for {room}",
                    ),
                    @room,
                );

                self.accept_requests.borrow_mut().remove(&room);
                self.reset();
            }
        }

        /// Decline the invite.
        #[template_callback]
        async fn decline(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            let obj = self.obj();

            let Some(response) = confirm_leave_room_dialog(&room, &*obj).await else {
                return;
            };

            self.accept_button.set_sensitive(false);
            self.decline_button.set_is_loading(true);
            self.decline_requests.borrow_mut().insert(room.clone());

            let ignored_inviter = response.ignore_inviter.then(|| room.inviter()).flatten();

            let closed = if room.change_category(TargetRoomCategory::Left).await.is_ok() {
                // A room where we were invited is usually empty so just close it.
                let _ = obj.activate_action("session.close-room", None);
                true
            } else {
                toast!(
                    obj,
                    gettext(
                        // Translators: Do NOT translate the content between '{' and '}', this
                        // is a variable name.
                        "Could not decline invitation for {room}",
                    ),
                    @room,
                );

                self.decline_requests.borrow_mut().remove(&room);
                self.reset();
                false
            };

            if let Some(inviter) = ignored_inviter {
                if inviter.upcast::<User>().ignore().await.is_err() {
                    toast!(obj, gettext("Could not ignore user"));
                } else if !closed {
                    // Ignoring the user should remove the room from the sidebar so close it.
                    let _ = obj.activate_action("session.close-room", None);
                }
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
    /// A view presenting an invitation to a room.
    pub struct Invite(ObjectSubclass<imp::Invite>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Invite {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The header bar of the invite.
    pub fn header_bar(&self) -> &adw::HeaderBar {
        &self.imp().header_bar
    }
}
