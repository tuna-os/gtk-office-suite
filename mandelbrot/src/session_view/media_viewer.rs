use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gdk, glib, glib::clone, graphene};
use ruma::OwnedEventId;
use tracing::warn;

use crate::{
    components::{MediaContentViewer, ScaleRevealer},
    session::Room,
    spawn, toast,
    utils::matrix::VisualMediaMessage,
};

/// The duration of the animation to fade the background, in ms.
const ANIMATION_DURATION: u32 = 250;
/// The duration of the animation to cancel a swipe, in ms.
const CANCEL_SWIPE_ANIMATION_DURATION: u32 = 400;

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        collections::HashMap,
    };

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/tunaos/mandelbrot/ui/session_view/media_viewer.ui")]
    #[properties(wrapper_type = super::MediaViewer)]
    pub struct MediaViewer {
        #[template_child]
        toolbar_view: TemplateChild<adw::ToolbarView>,
        #[template_child]
        header_bar: TemplateChild<gtk::HeaderBar>,
        #[template_child]
        menu: TemplateChild<gtk::MenuButton>,
        #[template_child]
        revealer: TemplateChild<ScaleRevealer>,
        #[template_child]
        media: TemplateChild<MediaContentViewer>,
        /// Whether the viewer is fullscreened.
        #[property(get, set = Self::set_fullscreened, explicit_notify)]
        fullscreened: Cell<bool>,
        /// The room containing the media message.
        #[property(get)]
        room: glib::WeakRef<Room>,
        /// The ID of the event containing the media message.
        event_id: RefCell<Option<OwnedEventId>>,
        /// The media message to display.
        message: RefCell<Option<VisualMediaMessage>>,
        /// The filename of the media.
        #[property(get)]
        filename: RefCell<Option<String>>,
        /// The API to keep track of the animation to fade the background.
        animation: OnceCell<adw::TimedAnimation>,
        swipe_tracker: OnceCell<adw::SwipeTracker>,
        swipe_progress: Cell<f64>,
        actions_expression_watches: RefCell<HashMap<&'static str, gtk::ExpressionWatch>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MediaViewer {
        const NAME: &'static str = "MediaViewer";
        type Type = super::MediaViewer;
        type ParentType = gtk::Widget;
        type Interfaces = (adw::Swipeable,);

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("media-viewer");

            klass.install_action("media-viewer.close", None, |obj, _, _| {
                obj.imp().close();
            });
            klass.add_binding_action(
                gdk::Key::Escape,
                gdk::ModifierType::empty(),
                "media-viewer.close",
            );

            // Menu actions
            klass.install_action("media-viewer.copy-image", None, |obj, _, _| {
                obj.imp().copy_image();
            });

            klass.install_action_async("media-viewer.save-image", None, |obj, _, _| async move {
                obj.imp().save_file().await;
            });

            klass.install_action_async("media-viewer.save-video", None, |obj, _, _| async move {
                obj.imp().save_file().await;
            });

            klass.install_action_async("media-viewer.permalink", None, |obj, _, _| async move {
                obj.imp().copy_permalink().await;
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for MediaViewer {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            self.init_swipe_tracker();

            // Bind `fullscreened` to the window property of the same name.
            obj.connect_root_notify(|obj| {
                if let Some(window) = obj.root().and_downcast::<gtk::Window>() {
                    window
                        .bind_property("fullscreened", obj, "fullscreened")
                        .sync_create()
                        .build();
                }
            });

            self.revealer.connect_transition_done(clone!(
                #[weak]
                obj,
                move |revealer| {
                    if !revealer.reveal_child() {
                        // Hide the viewer when the hiding transition is done.
                        obj.set_visible(false);
                    }
                }
            ));

            self.update_menu_actions();
        }

        fn dispose(&self) {
            self.toolbar_view.unparent();

            for expr_watch in self.actions_expression_watches.take().values() {
                expr_watch.unwatch();
            }
        }
    }

    impl WidgetImpl for MediaViewer {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            // Follow the swipe on the y axis.
            let swipe_y_offset = -f64::from(height) * self.swipe_progress.get();
            let allocation = gtk::Allocation::new(0, swipe_y_offset as i32, width, height);
            self.toolbar_view.size_allocate(&allocation, baseline);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();

            // Compute the progress between the swipe and the animation.
            let progress = {
                let swipe_progress = 1.0 - self.swipe_progress.get().abs();
                let animation_progress = self.animation().value();
                swipe_progress.min(animation_progress)
            };

            if progress > 0.0 {
                // Change the background opacity depending on the progress.
                let background_color = gdk::RGBA::new(0.0, 0.0, 0.0, 1.0 * progress as f32);
                let bounds = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
                snapshot.append_color(&background_color, &bounds);
            }

            obj.snapshot_child(&*self.toolbar_view, snapshot);
        }
    }

    impl SwipeableImpl for MediaViewer {
        fn cancel_progress(&self) -> f64 {
            0.0
        }

        fn distance(&self) -> f64 {
            self.obj().height().into()
        }

        fn progress(&self) -> f64 {
            self.swipe_progress.get()
        }

        fn snap_points(&self) -> Vec<f64> {
            vec![-1.0, 0.0, 1.0]
        }

        fn swipe_area(&self, _: adw::NavigationDirection, _: bool) -> gdk::Rectangle {
            let obj = self.obj();
            gdk::Rectangle::new(0, 0, obj.width(), obj.height())
        }
    }

    #[gtk::template_callbacks]
    impl MediaViewer {
        /// Set whether the viewer is fullscreened.
        fn set_fullscreened(&self, fullscreened: bool) {
            if fullscreened == self.fullscreened.get() {
                return;
            }

            self.fullscreened.set(fullscreened);

            if fullscreened {
                // Upscale the media on fullscreen.
                self.media.set_halign(gtk::Align::Fill);
                self.toolbar_view
                    .set_top_bar_style(adw::ToolbarStyle::Raised);
            } else {
                self.media.set_halign(gtk::Align::Center);
                self.toolbar_view.set_top_bar_style(adw::ToolbarStyle::Flat);
            }

            self.obj().notify_fullscreened();
        }

        /// Set the media message to display.
        pub(super) fn set_message(
            &self,
            room: &Room,
            message: VisualMediaMessage,
            event_id: Option<OwnedEventId>,
        ) {
            self.room.set(Some(room));
            self.event_id.replace(event_id);
            self.set_filename(message.filename());
            self.message.replace(Some(message));

            self.update_menu_actions();
            self.media.show_loading();

            spawn!(clone!(
                #[weak(rename_to = imp)]
                self,
                async move {
                    imp.build().await;
                }
            ));

            self.obj().notify_room();
        }

        /// Set the filename of the media.
        fn set_filename(&self, filename: String) {
            if Some(&filename) == self.filename.borrow().as_ref() {
                return;
            }

            self.filename.replace(Some(filename));
            self.obj().notify_filename();
        }

        /// The API to keep track of the animation to fade the background.
        fn animation(&self) -> &adw::TimedAnimation {
            self.animation.get_or_init(|| {
                let target = adw::CallbackAnimationTarget::new(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |value| {
                        // Fade the header bar content too.
                        imp.header_bar.set_opacity(value);

                        imp.obj().queue_draw();
                    }
                ));
                let animation =
                    adw::TimedAnimation::new(&*self.obj(), 0.0, 1.0, ANIMATION_DURATION, target);

                animation.connect_done(clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |_| {
                        // Clear the media viewer when it is closed and the transition is done.
                        if !imp.revealer.reveal_child() {
                            imp.media.clear();
                        }
                    }
                ));

                animation
            })
        }

        /// Initialize the swipe tracker.
        fn init_swipe_tracker(&self) {
            // Initialize the swipe tracker.
            let swipe_tracker = self
                .swipe_tracker
                .get_or_init(|| adw::SwipeTracker::new(&*self.obj()));
            swipe_tracker.set_orientation(gtk::Orientation::Vertical);
            swipe_tracker.connect_update_swipe(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, progress| {
                    // Hide the header bar.
                    imp.header_bar.set_opacity(0.0);

                    // Update the swipe progress to follow the position on the y axis.
                    imp.swipe_progress.set(progress);

                    // Reposition and redraw the widget.
                    let obj = imp.obj();
                    obj.queue_allocate();
                    obj.queue_draw();
                }
            ));
            swipe_tracker.connect_end_swipe(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, to| {
                    if to != 0.0 {
                        // The swipe is complete, close the viewer.
                        imp.close();
                        imp.header_bar.set_opacity(1.0);
                        return;
                    }

                    // The swipe is cancelled, reset the position of the viewer and animate the
                    // transition.
                    let target = adw::CallbackAnimationTarget::new(clone!(
                        #[weak]
                        imp,
                        move |value| {
                            // Update the swipe progress to fake a swipe back.
                            imp.swipe_progress.set(value);

                            let obj = imp.obj();
                            obj.queue_allocate();
                            obj.queue_draw();
                        }
                    ));
                    let swipe_progress = imp.swipe_progress.get();
                    let animation = adw::TimedAnimation::new(
                        &*imp.obj(),
                        swipe_progress,
                        0.0,
                        CANCEL_SWIPE_ANIMATION_DURATION,
                        target,
                    );
                    animation.set_easing(adw::Easing::EaseOutCubic);
                    animation.connect_done(clone!(
                        #[weak]
                        imp,
                        move |_| {
                            // Show the header bar again.
                            imp.header_bar.set_opacity(1.0);
                        }
                    ));
                    animation.play();
                }
            ));
        }

        /// Update the actions of the menu according to the current message.
        fn update_menu_actions(&self) {
            let borrowed_message = self.message.borrow();
            let message = borrowed_message.as_ref();
            let has_image = message.is_some_and(|m| matches!(m, VisualMediaMessage::Image(_)));
            let has_video = message.is_some_and(|m| matches!(m, VisualMediaMessage::Video(_)));

            let has_event_id = self.event_id.borrow().is_some();

            let obj = self.obj();
            obj.action_set_enabled("media-viewer.copy-image", has_image);
            obj.action_set_enabled("media-viewer.save-image", has_image);
            obj.action_set_enabled("media-viewer.save-video", has_video);
            obj.action_set_enabled("media-viewer.permalink", has_event_id);
        }

        /// Build the content of this viewer.
        async fn build(&self) {
            let Some(session) = self.room.upgrade().and_then(|r| r.session()) else {
                return;
            };
            let Some(message) = self.message.borrow().clone() else {
                return;
            };

            let content_type = message.content_type();

            let client = session.client();
            match message.into_tmp_file(&client).await {
                Ok(file) => {
                    self.media.view_file(file, Some(content_type)).await;
                }
                Err(error) => {
                    warn!("Could not retrieve media file: {error}");
                    self.media.show_fallback(content_type);
                }
            }
        }

        /// Close the viewer.
        fn close(&self) {
            if self.fullscreened.get() {
                // Deactivate the fullscreen.
                let _ = self.obj().activate_action("win.toggle-fullscreen", None);
            }

            // Trigger the revealer animation.
            self.revealer.set_reveal_child(false);

            // Fade out the background.
            let animation = self.animation();
            animation.set_value_from(animation.value());
            animation.set_value_to(0.0);
            animation.play();
        }

        /// Reveal this widget by transitioning from `source_widget`.
        pub(super) fn reveal(&self, source_widget: &gtk::Widget) {
            self.obj().set_visible(true);
            self.menu.grab_focus();

            // Reset the swipe.
            self.swipe_progress.set(0.0);

            // Trigger the revealer.
            self.revealer.set_source_widget(Some(source_widget));
            self.revealer.set_reveal_child(true);

            // Fade in the background.
            let animation = self.animation();
            animation.set_value_from(animation.value());
            animation.set_value_to(1.0);
            animation.play();
        }

        /// Reveal or hide the headerbar.
        fn reveal_headerbar(&self, reveal: bool) {
            if self.fullscreened.get() {
                self.toolbar_view.set_reveal_top_bars(reveal);
            }
        }

        /// Toggle whether the header bar is revealed.
        fn toggle_headerbar(&self) {
            let revealed = self.toolbar_view.reveals_top_bars();
            self.reveal_headerbar(!revealed);
        }

        /// Handle when motion was detected in the viewer.
        #[template_callback]
        fn handle_motion(&self, _x: f64, y: f64) {
            if y <= 50.0 {
                // Reveal the header bar if the pointer is at the top of the view.
                self.reveal_headerbar(true);
            }
        }

        /// Handle a click in the viewer.
        #[template_callback]
        fn handle_click(&self, n_pressed: i32) {
            if self.fullscreened.get() && n_pressed == 1 {
                // When the view if fullscreened, clicking reveals and hides the header bar.
                self.toggle_headerbar();
            } else if n_pressed == 2 {
                // A double-click toggles fullscreen.
                let _ = self.obj().activate_action("win.toggle-fullscreen", None);
            }
        }

        /// Copy the current image to the clipboard.
        fn copy_image(&self) {
            let Some(texture) = self.media.texture() else {
                return;
            };

            let obj = self.obj();
            obj.clipboard().set_texture(&texture);
            toast!(obj, gettext("Image copied to clipboard"));
        }

        /// Save the current file to the clipboard.
        async fn save_file(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Some(media_message) = self.message.borrow().clone() else {
                return;
            };
            let Some(session) = room.session() else {
                return;
            };
            let client = session.client();

            media_message
                .save_to_file(
                    // The timestamp should be unused for visual media messages.
                    &glib::DateTime::now_local().expect("Getting local time should work"),
                    &client,
                    &*self.obj(),
                )
                .await;
        }

        /// Copy the permalink of the event of the media message to the
        /// clipboard.
        async fn copy_permalink(&self) {
            let Some(room) = self.room.upgrade() else {
                return;
            };
            let Some(event_id) = self.event_id.borrow().clone() else {
                return;
            };

            let permalink = room.matrix_to_event_uri(event_id).await;

            let obj = self.obj();
            obj.clipboard().set_text(&permalink.to_string());
            toast!(obj, gettext("Message link copied to clipboard"));
        }
    }
}

glib::wrapper! {
    /// A widget allowing to view a media file.
    ///
    /// Swiping to the top or bottom closes this viewer.
    pub struct MediaViewer(ObjectSubclass<imp::MediaViewer>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, adw::Swipeable;
}

impl MediaViewer {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Reveal this widget by transitioning from `source_widget`.
    pub(crate) fn reveal(&self, source_widget: &impl IsA<gtk::Widget>) {
        self.imp().reveal(source_widget.upcast_ref());
    }

    /// Set the media message to display in the given room.
    pub(crate) fn set_message(
        &self,
        room: &Room,
        message: VisualMediaMessage,
        event_id: Option<OwnedEventId>,
    ) {
        self.imp().set_message(room, message, event_id);
    }
}
