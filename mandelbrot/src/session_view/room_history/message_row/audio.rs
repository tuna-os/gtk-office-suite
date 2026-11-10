use gettextrs::gettext;
use gtk::{glib, prelude::*, subclass::prelude::*};

use super::ContentFormat;
use crate::{
    components::{AudioPlayer, AudioPlayerMessage, AudioPlayerSource},
    gettext_f,
};

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_history/message_row/audio.ui"
    )]
    #[properties(wrapper_type = super::MessageAudio)]
    pub struct MessageAudio {
        #[template_child]
        player: TemplateChild<AudioPlayer>,
        /// The name of the audio file.
        #[property(get)]
        name: RefCell<String>,
        /// Whether to display this audio message in a compact format.
        #[property(get)]
        compact: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MessageAudio {
        const NAME: &'static str = "ContentMessageAudio";
        type Type = super::MessageAudio;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MessageAudio {}

    impl WidgetImpl for MessageAudio {}
    impl BoxImpl for MessageAudio {}

    impl MessageAudio {
        /// Set the name of the audio file.
        fn set_name(&self, name: Option<String>) {
            let name = name.unwrap_or_default();

            if *self.name.borrow() == name {
                return;
            }
            let obj = self.obj();

            let accessible_label = if name.is_empty() {
                gettext("Audio")
            } else {
                gettext_f("Audio: {filename}", &[("filename", &name)])
            };
            obj.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);

            self.name.replace(name);
            obj.notify_name();
        }

        /// Set the compact format of this audio message.
        fn set_compact(&self, compact: bool) {
            let obj = self.obj();
            self.compact.set(compact);

            obj.notify_compact();
        }

        /// Display the given `audio` message.
        pub(super) fn set_audio_message(&self, message: AudioPlayerMessage, format: ContentFormat) {
            self.set_name(Some(message.message.display_name()));

            let compact = matches!(format, ContentFormat::Compact | ContentFormat::Ellipsized);
            self.set_compact(compact);

            if compact {
                self.player.set_source(None);
            } else {
                self.player
                    .set_source(Some(AudioPlayerSource::Message(message)));
            }
        }
    }
}

glib::wrapper! {
    /// A widget displaying an audio message in the timeline.
    pub struct MessageAudio(ObjectSubclass<imp::MessageAudio>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MessageAudio {
    /// Create a new audio message.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Display the given `audio` message.
    pub(crate) fn set_audio_message(&self, message: AudioPlayerMessage, format: ContentFormat) {
        self.imp().set_audio_message(message, format);
    }
}

impl Default for MessageAudio {
    fn default() -> Self {
        Self::new()
    }
}
