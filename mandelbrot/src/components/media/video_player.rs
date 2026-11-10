use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, glib::clone};
use tracing::{error, warn};

use super::video_player_renderer::VideoPlayerRenderer;
use crate::utils::{LoadingState, media};

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/components/media/video_player.ui")]
    #[properties(wrapper_type = super::VideoPlayer)]
    pub struct VideoPlayer {
        #[template_child]
        video: TemplateChild<gtk::Picture>,
        #[template_child]
        timestamp: TemplateChild<gtk::Label>,
        #[template_child]
        player: TemplateChild<gst_play::Play>,
        /// The file that is currently played.
        file: RefCell<Option<gio::File>>,
        /// Whether the player is displayed in its compact form.
        #[property(get, set = Self::set_compact, explicit_notify)]
        compact: Cell<bool>,
        /// The state of the video in this player.
        #[property(get, builder(LoadingState::default()))]
        state: Cell<LoadingState>,
        /// The current error, if any.
        pub(super) error: RefCell<Option<glib::Error>>,
        /// The duration of the video, if it is known.
        duration: Cell<Option<gst::ClockTime>>,
        bus_guard: OnceCell<gst::bus::BusWatchGuard>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VideoPlayer {
        const NAME: &'static str = "VideoPlayer";
        type Type = super::VideoPlayer;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            VideoPlayerRenderer::ensure_type();

            Self::bind_template(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for VideoPlayer {
        fn constructed(&self) {
            self.parent_constructed();

            let bus_guard = self
                .player
                .message_bus()
                .add_watch_local(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move |_, message| {
                        if let Ok(message) = gst_play::PlayMessage::parse(message) {
                            imp.handle_message(&message);
                        }

                        glib::ControlFlow::Continue
                    }
                ))
                .expect("adding message bus watch succeeds");
            self.bus_guard
                .set(bus_guard)
                .expect("bus guard is uninitialized");
        }

        fn dispose(&self) {
            self.player.message_bus().set_flushing(true);
        }
    }

    impl WidgetImpl for VideoPlayer {
        fn map(&self) {
            self.parent_map();

            // Avoid more errors in the logs.
            if self.state.get() != LoadingState::Error {
                self.player.play();
            }
        }

        fn unmap(&self) {
            self.player.stop();
            self.parent_unmap();
        }
    }

    impl BinImpl for VideoPlayer {}

    impl VideoPlayer {
        /// Set whether this player should be displayed in a compact format.
        fn set_compact(&self, compact: bool) {
            if self.compact.get() == compact {
                return;
            }

            self.compact.set(compact);

            self.update_timestamp();
            self.obj().notify_compact();
        }

        /// Set the state of the media.
        fn set_state(&self, state: LoadingState) {
            if self.state.get() == state {
                return;
            }

            self.state.set(state);
            self.obj().notify_state();
        }

        /// Set the video file to play.
        pub(super) fn play_video_file(&self, file: gio::File) {
            let uri = file.uri();
            self.file.replace(Some(file));

            self.set_duration(None);
            self.set_state(LoadingState::Loading);

            self.player.set_uri(Some(uri.as_ref()));
            self.player.set_audio_track_enabled(false);

            if self.obj().is_mapped() {
                self.player.play();
            } else {
                // Pause, unlike stop, loads the info of the video.
                self.player.pause();
            }
        }

        /// Handle a message from the player.
        fn handle_message(&self, message: &gst_play::PlayMessage) {
            match message {
                gst_play::PlayMessage::StateChanged(change) => {
                    if matches!(
                        change.state(),
                        gst_play::PlayState::Playing | gst_play::PlayState::Paused
                    ) {
                        // Files that fail to play go from `Buffering` to `Stopped`.
                        self.set_state(LoadingState::Ready);
                    }
                }
                gst_play::PlayMessage::DurationChanged(change) => {
                    self.set_duration(change.duration());
                }
                gst_play::PlayMessage::Warning(warning) => {
                    warn!("Warning playing video: {}", warning.error());
                }
                gst_play::PlayMessage::Error(error) => {
                    let error = error.error().clone();
                    error!("Error playing video: {error}");
                    self.error.replace(Some(error));
                    self.set_state(LoadingState::Error);
                }
                _ => {}
            }
        }

        /// Set the duration of the video.
        fn set_duration(&self, duration: Option<gst::ClockTime>) {
            if self.duration.get() == duration {
                return;
            }

            self.duration.set(duration);
            self.update_timestamp();
        }

        /// Update the timestamp for the current state.
        fn update_timestamp(&self) {
            // We show the duration if we know it and if we are not in compact mode.
            let visible_duration = self.duration.get().filter(|_| !self.compact.get());
            let is_visible = visible_duration.is_some();

            if let Some(duration) = visible_duration {
                let label = media::time_to_label(&duration.into());
                self.timestamp.set_label(&label);
            }

            self.timestamp.set_visible(is_visible);
        }
    }
}

glib::wrapper! {
    /// A widget to preview a video file without controls or sound.
    pub struct VideoPlayer(ObjectSubclass<imp::VideoPlayer>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl VideoPlayer {
    /// Create a new video player.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the video file to play.
    pub(crate) fn play_video_file(&self, file: gio::File) {
        self.imp().play_video_file(file);
    }

    /// The current error, if any.
    pub(crate) fn error(&self) -> Option<glib::Error> {
        self.imp().error.borrow().clone()
    }
}
