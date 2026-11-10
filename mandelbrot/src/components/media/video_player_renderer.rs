use gst_play::{prelude::*, subclass::prelude::*};
use gtk::{gdk, glib};

mod imp {
    use std::{cell::OnceCell, marker::PhantomData};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::VideoPlayerRenderer)]
    pub struct VideoPlayerRenderer {
        /// The sink to use to display the video.
        sink: OnceCell<gst::Element>,
        /// The [`gdk::Paintable`] where the video is rendered.
        #[property(get = Self::paintable)]
        paintable: PhantomData<gdk::Paintable>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VideoPlayerRenderer {
        const NAME: &'static str = "VideoPlayerRenderer";
        type Type = super::VideoPlayerRenderer;
        type Interfaces = (gst_play::PlayVideoRenderer,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for VideoPlayerRenderer {}

    impl PlayVideoRendererImpl for VideoPlayerRenderer {
        fn create_video_sink(&self, _player: &gst_play::Play) -> gst::Element {
            self.sink().clone().upcast()
        }
    }

    impl VideoPlayerRenderer {
        /// The sink to use to display the video.
        fn sink(&self) -> &gst::Element {
            self.sink.get_or_init(|| {
                gst::ElementFactory::make("gtk4paintablesink")
                    .build()
                    .expect("gst-plugin-gtk4 should be available")
            })
        }

        /// The [`gdk::Paintable`] where the video is rendered.
        fn paintable(&self) -> gdk::Paintable {
            self.sink().property("paintable")
        }
    }
}

glib::wrapper! {
    /// A `GstPlayVideoRenderer` that renders to a `GdkPaintable`.
    pub struct VideoPlayerRenderer(ObjectSubclass<imp::VideoPlayerRenderer>)
        @implements gst_play::PlayVideoRenderer;
}

impl VideoPlayerRenderer {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for VideoPlayerRenderer {
    fn default() -> Self {
        Self::new()
    }
}
