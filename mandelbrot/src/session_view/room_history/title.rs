use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use crate::{prelude::*, session::Room, utils::BoundObjectWeakRef};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/title.ui")]
    #[properties(wrapper_type = super::RoomHistoryTitle)]
    pub struct RoomHistoryTitle {
        #[template_child]
        button: TemplateChild<gtk::Button>,
        #[template_child]
        subtitle_label: TemplateChild<gtk::Label>,
        // The room to present the title of.
        #[property(get, set = Self::set_room, explicit_notify, nullable)]
        room: BoundObjectWeakRef<Room>,
        // The title of the room that can be presented on a single line.
        #[property(get)]
        title: RefCell<String>,
        // The subtitle of the room that can be presented on a single line.
        #[property(get)]
        subtitle: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomHistoryTitle {
        const NAME: &'static str = "RoomHistoryTitle";
        type Type = super::RoomHistoryTitle;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_css_name("room-title");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for RoomHistoryTitle {}

    impl WidgetImpl for RoomHistoryTitle {
        fn grab_focus(&self) -> bool {
            self.button.grab_focus()
        }
    }

    impl BinImpl for RoomHistoryTitle {}

    impl RoomHistoryTitle {
        /// Set the room to present the title of.
        fn set_room(&self, room: Option<Room>) {
            if self.room.obj() == room {
                return;
            }

            self.room.disconnect_signals();

            if let Some(room) = room {
                let display_name_handler = room.connect_display_name_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_title();
                    }
                ));
                let topic_handler = room.connect_topic_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        imp.update_subtitle();
                    }
                ));

                self.room
                    .set(&room, vec![display_name_handler, topic_handler]);
            }

            self.obj().notify_room();
            self.update_title();
            self.update_subtitle();
        }

        /// Update the title of the room.
        fn update_title(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            // Remove newlines.
            let mut title = room.display_name().replace('\n', "");
            // Remove trailing spaces.
            title.truncate_end_whitespaces();

            if *self.title.borrow() == title {
                return;
            }

            self.title.replace(title);
            self.obj().notify_title();
        }

        /// Update the subtitle of the room.
        fn update_subtitle(&self) {
            let Some(room) = self.room.obj() else {
                return;
            };

            let subtitle = room
                .topic()
                .map(|s| {
                    // Remove newlines and empty lines and trailing whitespaces.
                    s.collapse_whitespaces(false, true)
                })
                .filter(|s| !s.is_empty());

            if *self.subtitle.borrow() == subtitle {
                return;
            }

            let has_subtitle = subtitle.is_some();

            self.subtitle.replace(subtitle);

            let obj = self.obj();
            obj.notify_subtitle();

            let button_valign = if has_subtitle {
                obj.add_css_class("with-subtitle");
                gtk::Align::Fill
            } else {
                obj.remove_css_class("with-subtitle");
                gtk::Align::Center
            };

            self.button.set_valign(button_valign);
            self.subtitle_label.set_visible(has_subtitle);
        }
    }
}

glib::wrapper! {
    /// A widget to show a room's title and topic in a header bar.
    pub struct RoomHistoryTitle(ObjectSubclass<imp::RoomHistoryTitle>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl RoomHistoryTitle {
    /// Construct a new empty `RoomHistoryTitle`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for RoomHistoryTitle {
    fn default() -> Self {
        Self::new()
    }
}
