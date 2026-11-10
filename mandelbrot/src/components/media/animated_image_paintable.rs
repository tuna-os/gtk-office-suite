use glycin::{Frame, Image};
use gtk::{gdk, glib, glib::clone, graphene, prelude::*, subclass::prelude::*};
use tracing::error;

use crate::{
    prelude::*,
    spawn,
    utils::{CountedRef, File},
};

mod imp {
    use std::cell::{OnceCell, RefCell};

    use super::*;

    #[derive(Default)]
    pub struct AnimatedImagePaintable {
        /// The image decoder.
        decoder: OnceCell<Image>,
        /// The file of the image.
        ///
        /// We need to keep a strong reference to the temporary file or it will
        /// be destroyed.
        file: OnceCell<File>,
        /// The current frame that is displayed.
        pub(super) current_frame: RefCell<Option<Frame>>,
        /// The next frame of the animation, if any.
        next_frame: RefCell<Option<Frame>>,
        /// The source ID of the timeout to load the next frame, if any.
        timeout_source_id: RefCell<Option<glib::SourceId>>,
        /// The counted reference for the animation.
        ///
        /// When the count is 0, the animation is paused.
        animation_ref: OnceCell<CountedRef>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AnimatedImagePaintable {
        const NAME: &'static str = "AnimatedImagePaintable";
        type Type = super::AnimatedImagePaintable;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for AnimatedImagePaintable {}

    impl PaintableImpl for AnimatedImagePaintable {
        fn intrinsic_height(&self) -> i32 {
            self.current_frame
                .borrow()
                .as_ref()
                .map_or_else(|| self.decoder().height(), glycin::Frame::height)
                .try_into()
                .unwrap_or(i32::MAX)
        }

        fn intrinsic_width(&self) -> i32 {
            self.current_frame
                .borrow()
                .as_ref()
                .map_or_else(|| self.decoder().width(), glycin::Frame::width)
                .try_into()
                .unwrap_or(i32::MAX)
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            if let Some(frame) = &*self.current_frame.borrow() {
                frame.texture().snapshot(snapshot, width, height);
            } else {
                let snapshot = snapshot.downcast_ref::<gtk::Snapshot>().unwrap();
                snapshot.append_color(
                    &gdk::RGBA::BLACK,
                    &graphene::Rect::new(0., 0., width as f32, height as f32),
                );
            }
        }

        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::STATIC_SIZE
        }

        fn current_image(&self) -> gdk::Paintable {
            let snapshot = gtk::Snapshot::new();
            self.snapshot(
                snapshot.upcast_ref(),
                self.intrinsic_width().into(),
                self.intrinsic_height().into(),
            );

            snapshot
                .to_paintable(None)
                .expect("snapshot should always work")
        }
    }

    impl AnimatedImagePaintable {
        /// The image decoder.
        fn decoder(&self) -> &Image {
            self.decoder.get().expect("decoder should be initialized")
        }

        /// Initialize the image.
        pub(super) fn init(&self, decoder: Image, first_frame: Frame, file: Option<File>) {
            self.decoder
                .set(decoder)
                .expect("decoder should be uninitialized");
            self.current_frame.replace(Some(first_frame));

            if let Some(file) = file {
                self.file.set(file).expect("file should be uninitialized");
            }

            self.update_animation();
        }

        /// Show the next frame of the animation.
        fn show_next_frame(&self) {
            // Drop the timeout source ID so we know we are not waiting for it.
            self.timeout_source_id.take();

            let Some(next_frame) = self.next_frame.take() else {
                // Wait for the next frame to be loaded.
                return;
            };

            self.current_frame.replace(Some(next_frame));

            // Invalidate the contents so that the new frame will be rendered.
            self.obj().invalidate_contents();

            self.update_animation();
        }

        /// The counted reference of the animation.
        pub(super) fn animation_ref(&self) -> &CountedRef {
            self.animation_ref.get_or_init(|| {
                CountedRef::new(
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move || {
                            imp.update_animation();
                        }
                    ),
                    clone!(
                        #[weak(rename_to = imp)]
                        self,
                        move || {
                            imp.update_animation();
                        }
                    ),
                )
            })
        }

        /// Prepare the next frame of the animation or stop the animation,
        /// depending on the refcount.
        fn update_animation(&self) {
            if self.animation_ref().count() == 0 {
                // We should not animate, remove the timeout if it exists.
                if let Some(source) = self.timeout_source_id.take() {
                    source.remove();
                }
                return;
            } else if self.timeout_source_id.borrow().is_some() {
                // We are already waiting for the next update.
                return;
            }

            let Some(delay) = self
                .current_frame
                .borrow()
                .as_ref()
                .and_then(GlycinFrameExt::delay_duration)
            else {
                return;
            };

            // Set the timeout to update the animation.
            let source_id = glib::timeout_add_local_once(
                delay,
                clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move || {
                        imp.show_next_frame();
                    }
                ),
            );
            self.timeout_source_id.replace(Some(source_id));

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.load_next_frame_inner().await;
                }
            ));
        }

        async fn load_next_frame_inner(&self) {
            match self.decoder().next_frame_future().await {
                Ok(next_frame) => {
                    self.next_frame.replace(Some(next_frame));

                    // In case loading the frame took longer than the delay between frames.
                    if self.timeout_source_id.borrow().is_none() {
                        self.show_next_frame();
                    }
                }
                Err(error) => {
                    error!("Failed to load next frame: {error}");
                    // Do nothing, the animation will stop.
                }
            }
        }
    }
}

glib::wrapper! {
    /// A paintable to display an animated image.
    pub struct AnimatedImagePaintable(ObjectSubclass<imp::AnimatedImagePaintable>)
        @implements gdk::Paintable;
}

impl AnimatedImagePaintable {
    /// Construct an `AnimatedImagePaintable` with the given  decoder, first
    /// frame, and the file containing the image, if any.
    pub(crate) fn new(decoder: Image, first_frame: Frame, file: Option<File>) -> Self {
        let obj = glib::Object::new::<Self>();

        obj.imp().init(decoder, first_frame, file);

        obj
    }

    /// Get the current `GdkTexture` of this paintable, if any.
    pub(crate) fn current_texture(&self) -> Option<gdk::Texture> {
        Some(self.imp().current_frame.borrow().as_ref()?.texture())
    }

    /// Get an animation ref.
    pub(crate) fn animation_ref(&self) -> CountedRef {
        self.imp().animation_ref().clone()
    }
}
