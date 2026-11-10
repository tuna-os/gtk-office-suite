use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use super::HistoryViewerEvent;
use crate::components::{AudioPlayer, AudioPlayerMessage, AudioPlayerSource};

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/history_viewer/audio_row.ui"
    )]
    #[properties(wrapper_type = super::AudioRow)]
    pub struct AudioRow {
        #[template_child]
        player: TemplateChild<AudioPlayer>,
        /// The audio event.
        #[property(get, set = Self::set_event, explicit_notify, nullable)]
        event: RefCell<Option<HistoryViewerEvent>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AudioRow {
        const NAME: &'static str = "ContentAudioHistoryViewerRow";
        type Type = super::AudioRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AudioRow {}

    impl WidgetImpl for AudioRow {}
    impl BinImpl for AudioRow {}

    impl AudioRow {
        /// Set the audio event.
        fn set_event(&self, event: Option<HistoryViewerEvent>) {
            if *self.event.borrow() == event {
                return;
            }

            if let Some(event) = &event
                && let Some(session) = event.room().and_then(|room| room.session())
            {
                self.player
                    .set_source(Some(AudioPlayerSource::Message(AudioPlayerMessage::new(
                        event.media_message(),
                        &session,
                        Default::default(),
                    ))));
            } else {
                self.player.set_source(None);
            }

            self.event.replace(event);
            self.obj().notify_event();
        }
    }
}

glib::wrapper! {
    /// A row presenting an audio event.
    pub struct AudioRow(ObjectSubclass<imp::AudioRow>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AudioRow {
    /// Construct an empty `AudioRow`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for AudioRow {
    fn default() -> Self {
        Self::new()
    }
}
