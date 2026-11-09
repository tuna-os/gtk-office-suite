use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{glib, glib::clone};

use super::SpaceOverview;
use crate::{
    Window,
    components::{Avatar, LoadingButton},
    gettext_f, ngettext_f,
    prelude::*,
    session::SpaceHierarchyChild,
    toast,
    utils::matrix::MatrixIdUri,
};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/space_overview/child_row.ui")]
    #[properties(wrapper_type = super::SpaceChildRow)]
    pub struct SpaceChildRow {
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        display_name: TemplateChild<gtk::Label>,
        #[template_child]
        suggested_badge: TemplateChild<gtk::Label>,
        #[template_child]
        description: TemplateChild<gtk::Label>,
        #[template_child]
        alias: TemplateChild<gtk::Label>,
        #[template_child]
        browse_button: TemplateChild<gtk::Button>,
        #[template_child]
        button: TemplateChild<LoadingButton>,
        #[template_child]
        members_count_box: TemplateChild<gtk::Box>,
        #[template_child]
        members_count: TemplateChild<gtk::Label>,
        #[template_child]
        rooms_count: TemplateChild<gtk::Label>,
        /// The room displayed by this row.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: RefCell<Option<SpaceHierarchyChild>>,
        room_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        room_list_info_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SpaceChildRow {
        const NAME: &'static str = "SpaceChildRow";
        type Type = super::SpaceChildRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Avatar::ensure_type();
            LoadingButton::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SpaceChildRow {
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

    impl WidgetImpl for SpaceChildRow {}
    impl BinImpl for SpaceChildRow {}

    #[gtk::template_callbacks]
    impl SpaceChildRow {
        /// Set the room displayed by this row.
        fn set_room(&self, room: Option<SpaceHierarchyChild>) {
            if *self.room.borrow() == room {
                return;
            }

            self.disconnect_signals();

            if let Some(room) = &room {
                let update_row_handlers = ["display-name", "topic-linkified", "alias-string"]
                    .iter()
                    .map(|prop| {
                        room.connect_notify_local(
                            Some(prop),
                            clone!(
                                #[weak(rename_to = imp)]
                                self,
                                move |_, _| {
                                    imp.update_row();
                                }
                            ),
                        )
                    });
                let update_counts_handlers =
                    ["joined-members-count", "children-count"]
                        .iter()
                        .map(|prop| {
                            room.connect_notify_local(
                                Some(prop),
                                clone!(
                                    #[weak(rename_to = imp)]
                                    self,
                                    move |_, _| {
                                        imp.update_counts();
                                    }
                                ),
                            )
                        });
                self.room_handlers
                    .replace(update_row_handlers.chain(update_counts_handlers).collect());

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
            }

            self.room.replace(room);

            self.update_row();
            self.update_counts();
            self.update_button();
            self.obj().notify_room();
        }

        /// Update this row for the current state.
        fn update_row(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            self.avatar.set_data(Some(room.avatar_data()));
            self.display_name.set_text(&room.display_name());
            self.suggested_badge.set_visible(room.is_suggested());

            if let Some(topic) = room.topic_linkified() {
                self.description.set_label(&topic);
                self.description.set_visible(!topic.is_empty());
            } else {
                self.description.set_visible(false);
            }

            let alias = room.alias_string();
            if let Some(alias) = &alias {
                self.alias.set_text(alias);
            }
            self.alias.set_visible(alias.is_some());
        }

        /// Update the member and room counts of this row.
        fn update_counts(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            let members_count = u32::try_from(room.joined_members_count()).unwrap_or(u32::MAX);
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

            if room.is_space() {
                let rooms_count = u32::try_from(room.children_count()).unwrap_or(u32::MAX);
                let rooms_count_label = ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}',
                    // this is a variable name.
                    "1 room",
                    "{n} rooms",
                    rooms_count,
                    &[("n", &rooms_count.to_string())],
                );
                self.rooms_count.set_text(&rooms_count_label);
                self.rooms_count.set_visible(true);
            } else {
                self.rooms_count.set_visible(false);
            }
        }

        /// Update the join/view button of this row.
        fn update_button(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            self.browse_button.set_visible(room.is_space());

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

        /// Browse the hierarchy of this space.
        #[template_callback]
        fn browse(&self) {
            let Some(room) = self.room.borrow().clone() else {
                return;
            };

            if let Some(overview) = self
                .obj()
                .ancestor(SpaceOverview::static_type())
                .and_downcast::<SpaceOverview>()
            {
                overview.push_space(&room);
            }
        }

        /// Join or view the room.
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

                let identifier = room.room_id().clone().into();
                let via = room.via();

                let result = if room.can_knock() {
                    session.room_list().knock(identifier, via).await
                } else {
                    session
                        .room_list()
                        .join_by_id_or_alias(identifier, via)
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
                for handler in self.room_handlers.take() {
                    room.disconnect(handler);
                }

                let room_list_info = room.room_list_info();
                for handler in self.room_list_info_handlers.take() {
                    room_list_info.disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A row representing a room or space in the hierarchy of a space.
    pub struct SpaceChildRow(ObjectSubclass<imp::SpaceChildRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SpaceChildRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for SpaceChildRow {
    fn default() -> Self {
        Self::new()
    }
}
