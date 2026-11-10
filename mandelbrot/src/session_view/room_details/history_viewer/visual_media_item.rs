use gtk::{gdk, glib, glib::clone, prelude::*, subclass::prelude::*};
use ruma::api::client::media::get_content_thumbnail::v3::Method;

use super::{HistoryViewerEvent, VisualMediaHistoryViewer};
use crate::{
    session::Session,
    spawn,
    utils::{
        key_bindings,
        matrix::VisualMediaMessage,
        media::{
            FrameDimensions,
            image::{Blurhash, ImageRequestPriority, ThumbnailSettings},
        },
    },
};

/// The default size for the preview.
const PREVIEW_SIZE: u32 = 300;
/// The default dimensions of the preview.
const PREVIEW_DIMENSIONS: FrameDimensions = FrameDimensions {
    width: PREVIEW_SIZE,
    height: PREVIEW_SIZE,
};

/// The possible sources of the preview of a visual media.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum MediaPreview {
    /// There is no media preview.
    #[default]
    None,
    /// The media preview is the low-quality placeholder.
    Placeholder,
    /// The media preview is the thumbnail.
    Thumbnail,
}

mod imp {
    use std::cell::{Cell, RefCell};

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(
        resource = "/org/tunaos/mandelbrot/ui/session_view/room_details/history_viewer/visual_media_item.ui"
    )]
    #[properties(wrapper_type = super::VisualMediaItem)]
    pub struct VisualMediaItem {
        #[template_child]
        overlay: TemplateChild<gtk::Overlay>,
        #[template_child]
        picture: TemplateChild<gtk::Picture>,
        #[template_child]
        play_icon: TemplateChild<gtk::Image>,
        /// The event that is previewed.
        #[property(get, set = Self::set_event, explicit_notify, nullable)]
        event: RefCell<Option<HistoryViewerEvent>>,
        /// Which preview is presented by the picture.
        preview: Cell<MediaPreview>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VisualMediaItem {
        const NAME: &'static str = "ContentVisualMediaHistoryViewerItem";
        type Type = super::VisualMediaItem;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("visual-media-history-viewer-item");

            klass.install_action("visual-media-item.activate", None, |obj, _, _| {
                obj.imp().activate();
            });

            key_bindings::add_activate_bindings(klass, "visual-media-item.activate");
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for VisualMediaItem {
        fn dispose(&self) {
            self.overlay.unparent();
        }
    }

    impl WidgetImpl for VisualMediaItem {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            // Keep the widget squared
            let (min, ..) = self.overlay.measure(orientation, for_size);
            (min, for_size.max(min), -1, -1)
        }

        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.overlay.allocate(width, height, baseline, None);
        }
    }

    #[gtk::template_callbacks]
    impl VisualMediaItem {
        /// Set the media event.
        fn set_event(&self, event: Option<HistoryViewerEvent>) {
            if *self.event.borrow() == event {
                return;
            }

            // Reset the preview.
            self.preview.take();
            self.picture.set_paintable(None::<&gdk::Paintable>);

            self.event.replace(event);

            self.update();
            self.obj().notify_event();
        }

        /// Update this item for the current state.
        fn update(&self) {
            let Some(event) = self.event.borrow().clone() else {
                return;
            };
            let Some(media_message) = event.visual_media_message() else {
                return;
            };

            let is_video = matches!(media_message, VisualMediaMessage::Video(_));
            self.play_icon.set_visible(is_video);

            self.obj().set_tooltip_text(Some(&media_message.filename()));

            if let Some(blurhash) = media_message.blurhash() {
                spawn!(
                    glib::Priority::LOW,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        async move {
                            imp.load_placeholder(blurhash).await;
                        }
                    )
                );
            }

            let Some(room) = event.room() else {
                return;
            };
            let Some(session) = room.session() else {
                return;
            };

            if session
                .global_account_data()
                .should_room_show_media_previews(&room)
            {
                spawn!(
                    glib::Priority::LOW,
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        async move {
                            imp.load_thumbnail(media_message, &session).await;
                        }
                    )
                );
            }
        }

        /// Load the thumbnail for the given Blurhash.
        async fn load_placeholder(&self, blurhash: Blurhash) {
            let Some(placeholder_texture) = blurhash.into_texture(PREVIEW_DIMENSIONS).await else {
                return;
            };

            // Do not replace the thumbnail by the placeholder, in case the thumbnail is
            // loaded before.
            if self.preview.get() != MediaPreview::Thumbnail {
                self.picture.set_paintable(Some(&placeholder_texture));
                self.preview.set(MediaPreview::Placeholder);
            }
        }

        /// Load the thumbnail for the given media message.
        async fn load_thumbnail(&self, media_message: VisualMediaMessage, session: &Session) {
            let client = session.client();

            let scale_factor = u32::try_from(self.obj().scale_factor()).unwrap_or(1);
            let dimensions = PREVIEW_DIMENSIONS.scale(scale_factor);

            let settings = ThumbnailSettings {
                dimensions,
                method: Method::Scale,
                animated: false,
                prefer_thumbnail: false,
            };

            if let Ok(Some(image)) = media_message
                .thumbnail(client, settings, ImageRequestPriority::Default)
                .await
            {
                self.picture
                    .set_paintable(Some(&gdk::Paintable::from(image)));
                self.preview.set(MediaPreview::Thumbnail);
            }
        }

        /// The item was activated.
        #[template_callback]
        fn activate(&self) {
            let obj = self.obj();

            let Some(media_history_viewer) = obj
                .ancestor(VisualMediaHistoryViewer::static_type())
                .and_downcast::<VisualMediaHistoryViewer>()
            else {
                return;
            };

            media_history_viewer.show_media_viewer(&obj);
        }
    }
}

glib::wrapper! {
    /// An item presenting a visual media (image or video) event.
    pub struct VisualMediaItem(ObjectSubclass<imp::VisualMediaItem>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl VisualMediaItem {
    /// Construct a new empty `VisualMediaItem`.
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for VisualMediaItem {
    fn default() -> Self {
        Self::new()
    }
}
