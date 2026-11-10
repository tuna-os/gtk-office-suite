use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gdk, glib, glib::clone};

use super::SidebarRow;
use crate::{
    components::Avatar,
    i18n::{gettext_f, ngettext_f},
    prelude::*,
    session::{HighlightFlags, Room, RoomCategory},
    utils::{BoundObject, TemplateCallbacks},
};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/sidebar/room_row.ui")]
    #[properties(wrapper_type = super::SidebarRoomRow)]
    pub struct SidebarRoomRow {
        #[template_child]
        avatar: TemplateChild<Avatar>,
        #[template_child]
        display_name_box: TemplateChild<gtk::Box>,
        #[template_child]
        room_icon: TemplateChild<gtk::Image>,
        #[template_child]
        display_name: TemplateChild<gtk::Label>,
        #[template_child]
        notification_count: TemplateChild<gtk::Label>,
        /// The room represented by this row.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: BoundObject<Room>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarRoomRow {
        const NAME: &'static str = "SidebarRoomRow";
        type Type = super::SidebarRoomRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            TemplateCallbacks::bind_template_callbacks(klass);

            klass.set_css_name("room");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarRoomRow {
        fn constructed(&self) {
            self.parent_constructed();

            // Allow to drag rooms
            let drag = gtk::DragSource::builder()
                .actions(gdk::DragAction::MOVE)
                .build();
            drag.connect_prepare(clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                None,
                move |drag, x, y| imp.prepare_drag(drag, x, y)
            ));
            drag.connect_drag_begin(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _| {
                    imp.begin_drag();
                }
            ));
            drag.connect_drag_end(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _| {
                    imp.end_drag();
                }
            ));
            self.obj().add_controller(drag);
        }
    }

    impl WidgetImpl for SidebarRoomRow {}
    impl BinImpl for SidebarRoomRow {}

    impl SidebarRoomRow {
        /// Set the room represented by this row.
        fn set_room(&self, room: Option<Room>) {
            if self.room.obj() == room {
                return;
            }

            self.room.disconnect_signals();

            if let Some(room) = room {
                let highlight_handler = room.connect_highlight_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_highlight();
                    }
                ));
                let direct_handler = room.connect_is_direct_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_room_icon();
                        imp.update_accessibility_label();
                    }
                ));
                let name_handler = room.connect_display_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_accessibility_label();
                    }
                ));
                let notifications_count_handler = room.connect_notification_count_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_accessibility_label();
                    }
                ));
                let category_handler = room.connect_category_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_display_name();
                    }
                ));

                self.room.set(
                    room,
                    vec![
                        highlight_handler,
                        direct_handler,
                        name_handler,
                        notifications_count_handler,
                        category_handler,
                    ],
                );

                self.update_accessibility_label();
            }

            self.update_display_name();
            self.update_highlight();
            self.update_room_icon();
            self.obj().notify_room();
        }

        /// Update the display name of the room according to the current state.
        fn update_display_name(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            if matches!(room.category(), RoomCategory::Left) {
                self.display_name.add_css_class("dimmed");
            } else {
                self.display_name.remove_css_class("dimmed");
            }
        }

        /// Update how this row is highlighted according to the current state.
        fn update_highlight(&self) {
            if let Some(room) = self.room.obj() {
                let flags = room.highlight();

                if flags.contains(HighlightFlags::HIGHLIGHT) {
                    self.notification_count.add_css_class("highlight");
                } else {
                    self.notification_count.remove_css_class("highlight");
                }

                if flags.contains(HighlightFlags::BOLD) {
                    self.display_name.add_css_class("bold");
                } else {
                    self.display_name.remove_css_class("bold");
                }
            } else {
                self.notification_count.remove_css_class("highlight");
                self.display_name.remove_css_class("bold");
            }
        }

        /// The parent `SidebarRow` of this row.
        fn parent_row(&self) -> Option<SidebarRow> {
            self.obj().parent().and_downcast()
        }

        /// Prepare a drag action.
        fn prepare_drag(
            &self,
            drag: &gtk::DragSource,
            x: f64,
            y: f64,
        ) -> Option<gdk::ContentProvider> {
            let room = self.room.obj()?;

            if let Some(parent) = self.parent_row() {
                let paintable = gtk::WidgetPaintable::new(Some(&parent));
                // FIXME: The hotspot coordinates don't work.
                // See https://gitlab.gnome.org/GNOME/gtk/-/issues/2341
                drag.set_icon(Some(&paintable), x as i32, y as i32);
            }

            Some(gdk::ContentProvider::for_value(&room.to_value()))
        }

        /// Begin a drag action.
        fn begin_drag(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };
            let Some(row) = self.parent_row() else {
                return;
            };
            let Some(sidebar) = row.sidebar() else {
                return;
            };
            row.add_css_class("drag");

            sidebar.set_drop_source_category(Some(room.category()));
        }

        /// End a drag action.
        fn end_drag(&self) {
            let Some(row) = self.parent_row() else {
                return;
            };
            let Some(sidebar) = row.sidebar() else {
                return;
            };
            sidebar.set_drop_source_category(None);
            row.remove_css_class("drag");
        }

        /// Update the room icon depending on what type of room it is.
        fn update_room_icon(&self) {
            let is_direct = self.room.obj().is_some_and(|room| room.is_direct());
            let is_call = self.room.obj().is_some_and(|room| room.is_call());

            if is_call {
                self.room_icon.set_icon_name(Some("video-symbolic"));
                // Translators: A "call room" is a room where an audio or video call
                // is always active, that users can join and leave at any time.
                self.room_icon.set_tooltip_text(Some(&gettext("Call room")));
                self.room_icon.set_visible(true);
            } else if is_direct {
                self.room_icon.set_icon_name(Some("person-symbolic"));
                self.room_icon
                    .set_tooltip_text(Some(&gettext("Direct chat")));
                self.room_icon.set_visible(true);
            } else {
                self.room_icon.set_visible(false);
            }
        }

        /// Update the accessibility label of this row.
        fn update_accessibility_label(&self) {
            let Some(parent) = self.obj().parent() else {
                return;
            };
            parent.update_property(&[gtk::accessible::Property::Label(&self.accessible_label())]);
        }

        /// Compute the accessibility label of this row.
        fn accessible_label(&self) -> String {
            let Some(room) = self.room.obj() else {
                return String::new();
            };

            let name = if room.is_call() {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name. Presented to screen readers for "call rooms".
                    // A "call room" is a room where an audio or video call is always active,
                    // that users can join and leave at any time.
                    "{name} (call room)",
                    &[("name", &room.display_name())],
                )
            } else if room.is_direct() {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name. Presented to screen readers when a room is a direct chat with
                    // another user.
                    "Direct chat with {name}",
                    &[("name", &room.display_name())],
                )
            } else {
                room.display_name()
            };

            if room.notification_count() > 0 {
                let count = ngettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name. Presented to screen readers when a room has notifications
                    // for unread messages.
                    "1 notification",
                    "{count} notifications",
                    room.notification_count() as u32,
                    &[("count", &room.notification_count().to_string())],
                );
                format!("{name} {count}")
            } else {
                name
            }
        }
    }
}

glib::wrapper! {
    /// A sidebar row representing a room.
    pub struct SidebarRoomRow(ObjectSubclass<imp::SidebarRoomRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SidebarRoomRow {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for SidebarRoomRow {
    fn default() -> Self {
        Self::new()
    }
}
