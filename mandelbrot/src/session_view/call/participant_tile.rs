use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::clone};

use super::state::CallParticipant;
use crate::components::Avatar;

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/call/participant_tile.ui")]
    #[properties(wrapper_type = super::CallParticipantTile)]
    pub struct CallParticipantTile {
        #[template_child]
        video_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub(super) video_picture: TemplateChild<gtk::Picture>,
        #[template_child]
        avatar: TemplateChild<Avatar>,
        /// The participant displayed by this tile.
        #[property(get, set = Self::set_participant, explicit_notify, nullable)]
        participant: RefCell<Option<CallParticipant>>,
        participant_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CallParticipantTile {
        const NAME: &'static str = "CallParticipantTile";
        type Type = super::CallParticipantTile;
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
    impl ObjectImpl for CallParticipantTile {
        fn dispose(&self) {
            self.disconnect_signals();
        }
    }

    impl WidgetImpl for CallParticipantTile {}
    impl BinImpl for CallParticipantTile {}

    impl CallParticipantTile {
        /// Set the participant displayed by this tile.
        fn set_participant(&self, participant: Option<CallParticipant>) {
            if *self.participant.borrow() == participant {
                return;
            }

            self.disconnect_signals();

            if let Some(participant) = &participant {
                let speaking_handler = participant.connect_speaking_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |participant| {
                        imp.update_speaking(participant.speaking());
                    }
                ));
                let camera_handler = participant.connect_camera_on_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |participant| {
                        imp.update_video(participant.camera_on());
                    }
                ));
                let video_handler = participant.connect_video_paintable_notify(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |participant| {
                        imp.update_paintable(participant);
                    }
                ));
                self.participant_handlers.replace(vec![
                    speaking_handler,
                    camera_handler,
                    video_handler,
                ]);

                self.update_speaking(participant.speaking());
                self.update_video(participant.camera_on());
                self.update_paintable(participant);
            } else {
                self.update_speaking(false);
                self.update_video(false);
                self.video_picture.set_paintable(gtk::gdk::Paintable::NONE);
            }

            self.participant.replace(participant);
            self.obj().notify_participant();
        }

        /// Update the video paintable of this tile.
        ///
        /// The video is shown whenever a video stream is available, even if
        /// the participant did not advertise their camera.
        fn update_paintable(&self, participant: &CallParticipant) {
            let paintable = participant.video_paintable();
            self.video_picture.set_paintable(paintable.as_ref());
            participant.set_camera_on(paintable.is_some());
        }

        /// Update the speaking highlight of this tile.
        fn update_speaking(&self, speaking: bool) {
            if speaking {
                self.obj().add_css_class("speaking");
            } else {
                self.obj().remove_css_class("speaking");
            }
        }

        /// Show the video or the avatar, depending on whether the camera of
        /// the participant is enabled.
        fn update_video(&self, camera_on: bool) {
            let page = if camera_on { "video" } else { "avatar" };
            self.video_stack.set_visible_child_name(page);
        }

        /// Disconnect the signal handlers of the current participant.
        fn disconnect_signals(&self) {
            if let Some(participant) = self.participant.borrow().as_ref() {
                for handler in self.participant_handlers.take() {
                    participant.disconnect(handler);
                }
            }
        }
    }
}

glib::wrapper! {
    /// A tile displaying a single participant of a call.
    pub struct CallParticipantTile(ObjectSubclass<imp::CallParticipantTile>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CallParticipantTile {
    /// Create a new tile for the given participant.
    pub fn new(participant: &CallParticipant) -> Self {
        glib::Object::builder()
            .property("participant", participant)
            .build()
    }

    /// Set the paintable displaying the video stream of this participant.
    ///
    /// Integration point: this will receive the `gdk::Paintable` of a
    /// `gtk4paintablesink` fed by the `LiveKit` video track.
    pub fn set_video_paintable(&self, paintable: Option<&gtk::gdk::Paintable>) {
        self.imp().video_picture.set_paintable(paintable);
    }
}

impl Default for CallParticipantTile {
    fn default() -> Self {
        glib::Object::new()
    }
}
