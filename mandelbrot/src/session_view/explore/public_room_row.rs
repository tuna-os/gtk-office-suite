use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use crate::{
    Window,
    components::{Avatar, LoadingButton},
    gettext_f, ngettext_f,
    prelude::*,
    session::RemoteRoom,
    toast,
    utils::{matrix::MatrixIdUri, string::linkify},
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/explore/public_room_row.ui")]
    #[properties(wrapper_type = super::PublicRoomRow)]
    pub struct PublicRoomRow {
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        display_name: TemplateChild<gtk::Label>,
        #[template_child]
        description: TemplateChild<gtk::Label>,
        #[template_child]
        alias: TemplateChild<gtk::Label>,
        #[template_child]
        members_count: TemplateChild<gtk::Label>,
        #[template_child]
        members_count_box: TemplateChild<gtk::Box>,
        #[template_child]
        button: TemplateChild<LoadingButton>,
        /// The room displayed by this row.
        #[property(get, set= Self::set_room, explicit_notify)]
        room: RefCell<Option<RemoteRoom>>,
        room_list_info_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PublicRoomRow {
        const NAME: &'static str = "PublicRoomRow";
        type Type = super::PublicRoomRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for PublicRoomRow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.description.connect_activate_link(clone!(
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

    impl WidgetImpl for PublicRoomRow {}
    impl BinImpl for PublicRoomRow {}

    #[gtk::template_callbacks]
    impl PublicRoomRow {
        /// Set the room displayed by this row.
        fn set_room(&self, room: RemoteRoom) {
            if self.room.borrow().as_ref().is_some_and(|r| *r == room) {
                return;
            }

            self.disconnect_signals();

            let room_list_info = room.room_list_info();
            let is_joining_handler = room_list_info.connect_is_joining_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_button();
                }
            ));
            let local_room_handler = room_list_info.connect_local_room_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.update_button();
                }
            ));

            self.room_list_info_handlers
                .replace(vec![is_joining_handler, local_room_handler]);

            self.room.replace(Some(room));

            self.update_button();
            self.update_row();
            self.obj().notify_room();
        }

        /// Update this row for the current state.
        fn update_row(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            self.avatar.set_data(Some(room.avatar_data()));
            self.display_name.set_text(&room.display_name());

            if let Some(topic) = room.topic() {
                // Detect links.
                let mut t = linkify(&topic);
                // Remove trailing spaces.
                t.truncate_end_whitespaces();

                self.description.set_label(&t);
                self.description.set_visible(!t.is_empty());
            } else {
                self.description.set_visible(false);
            }

            let canonical_alias = room.canonical_alias();
            if let Some(alias) = &canonical_alias {
                self.alias.set_text(alias.as_str());
            }
            self.alias.set_visible(canonical_alias.is_some());

            let members_count = room.joined_members_count();
            self.members_count.set_text(&members_count.to_string());
            let members_count_tooltip = ngettext_f(
                // Translators: Do NOT translate the content between '{' and '}',
                // this is a variable name.
                "1 member",
                "{n} members",
                members_count,
                &[("n", &members_count.to_string())],
            );
            self.members_count_box
                .set_tooltip_text(Some(&members_count_tooltip));
        }

        /// Update the join/view button of this row.
        fn update_button(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            let room_list_info = room.room_list_info();
            let room_name = room.display_name();

            let (label, accessible_desc) = if room_list_info.local_room().is_some() {
                (
                    // Translators: This is a verb, as in 'View Room'.
                    gettext("View"),
                    gettext_f("View {room_name}", &[("room_name", &room_name)]),
                )
            } else if room.can_knock() {
                (
                    gettext("Request an Invite"),
                    gettext_f(
                        "Request an invite to {room_name}",
                        &[("room_name", &room_name)],
                    ),
                )
            } else {
                (
                    gettext("Join"),
                    gettext_f("Join {room_name}", &[("room_name", &room_name)]),
                )
            };

            self.button.set_content_label(label);
            self.button
                .update_property(&[gtk::accessible::Property::Description(&accessible_desc)]);

            self.button.set_is_loading(room_list_info.is_joining());
        }

        /// Join or view the public room.
        #[template_callback]
        async fn join_or_view(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            let obj = self.obj();

            if let Some(local_room) = room.room_list_info().local_room() {
                if let Some(window) = obj.root().and_downcast::<Window>() {
                    window.session_view().select_room(local_room);
                }
            } else {
                let Some(session) = room.session() else {
                    return;
                };

                let uri = room.uri();

                let result = if room.can_knock() {
                    session
                        .room_list()
                        .knock(uri.id.clone(), uri.via.clone())
                        .await
                } else {
                    session
                        .room_list()
                        .join_by_id_or_alias(uri.id.clone(), uri.via.clone())
                        .await
                };

                if let Err(error) = result {
                    toast!(obj, error);
                }
            }
        }

        /// Disconnect the signal handlers of this row.
        fn disconnect_signals(&self) {
            if let Some(room) = self.room.borrow().as_ref() {
                let room_list_info = room.room_list_info();
                for handler in self.room_list_info_handlers.take() {
                    room_list_info.disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A row representing a room in a homeserver's public directory.
    pub struct PublicRoomRow(ObjectSubclass<imp::PublicRoomRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PublicRoomRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for PublicRoomRow {
    fn default() -> Self {
        Self::new()
    }
}
