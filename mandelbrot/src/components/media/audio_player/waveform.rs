use adw::prelude::*;
use gtk::{
    gdk, glib,
    glib::{clone, closure_local},
    graphene, gsk,
    subclass::prelude::*,
};
use tracing::error;

use super::waveform_paintable::WaveformPaintable;

/// The height of the waveform.
pub(super) const WAVEFORM_HEIGHT: f32 = 60.0;
/// The height of the waveform, as an integer.
pub(super) const WAVEFORM_HEIGHT_I32: i32 = 60;
/// The duration of the animation, in milliseconds.
const ANIMATION_DURATION: u32 = 250;
/// The error margin when comparing two `f32`s.
const F32_ERROR_MARGIN: f32 = 0.0001;

mod imp {
    use std::{
        cell::{Cell, OnceCell, RefCell},
        sync::LazyLock,
    };

    use glib::subclass::Signal;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::Waveform)]
    pub struct Waveform {
        /// The paintable that draws the waveform.
        #[property(get)]
        paintable: WaveformPaintable,
        /// The current position in the audio stream.
        ///
        /// Must be a value between 0 and 1.
        #[property(get, set = Self::set_position, explicit_notify, minimum = 0.0, maximum = 1.0)]
        position: Cell<f32>,
        /// The animation for the transition between waveforms.
        animation: OnceCell<adw::TimedAnimation>,
        /// The current hover position, if any.
        hover_position: Cell<Option<f32>>,
        /// The cached paintable.
        ///
        /// We only need to redraw it when the waveform changes of the widget is
        /// resized.
        paintable_cache: RefCell<Option<gdk::Paintable>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Waveform {
        const NAME: &'static str = "Waveform";
        type Type = super::Waveform;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("waveform");
            klass.set_accessible_role(gtk::AccessibleRole::Slider);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Waveform {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder("seek")
                        .param_types([f32::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.init_event_controllers();

            let obj = self.obj();
            obj.set_focusable(true);
            obj.update_property(&[
                gtk::accessible::Property::ValueMin(0.0),
                gtk::accessible::Property::ValueMax(1.0),
                gtk::accessible::Property::ValueNow(0.0),
            ]);

            self.paintable.connect_invalidate_contents(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.queue_draw();
                }
            ));
        }
    }

    impl WidgetImpl for Waveform {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Vertical {
                // The height is fixed.
                (WAVEFORM_HEIGHT_I32, WAVEFORM_HEIGHT_I32, -1, -1)
            } else {
                // We accept any width, the optimal width is the default width of the paintable.
                (0, self.paintable.intrinsic_width(), -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            if self
                .paintable_cache
                .borrow()
                .as_ref()
                .is_some_and(|paintable| width != paintable.intrinsic_width())
            {
                // We need to adjust the waveform to the new width.
                self.paintable_cache.take();
                self.obj().queue_draw();
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let width = obj.width();

            if width <= 0 {
                return;
            }

            let Some(paintable) = self.paintable() else {
                return;
            };

            let width = width as f32;
            let is_rtl = obj.direction() == gtk::TextDirection::Rtl;

            // Use the waveform as a mask that we will apply to the colored rectangles
            // below.
            snapshot.push_mask(gsk::MaskMode::Alpha);
            snapshot.save();

            // Invert the paintable horizontally if we are in right-to-left direction.
            if is_rtl {
                snapshot.translate(&graphene::Point::new(width, 0.0));
                snapshot.scale(-1.0, 1.0);
            }

            paintable.snapshot(snapshot, width.into(), WAVEFORM_HEIGHT.into());

            snapshot.restore();
            snapshot.pop();

            // Paint three colored rectangles to mark the two positions:
            //
            //  ----------------------------
            // | played | hover | remaining |
            //  ----------------------------
            //
            // The "played" part stops at the first of the `position` or the
            // `hover_position` and the "hover" part stops at the last of the
            // `position` or the `hover_position`.
            //
            // The order is inverted in right-to-left direction, and any rectangle that is
            // not visible (i.e. has a width of 0) is not drawn.
            let (start, end) = if is_rtl { (width, 0.0) } else { (0.0, width) };
            let mut position = self.position.get() * width;
            if is_rtl {
                position = width - position;
            }
            let hover_position = self.hover_position.get();

            let (played_end, hover_end) = if let Some(hover_position) = hover_position {
                if (!is_rtl && hover_position > position) || (is_rtl && hover_position < position) {
                    (position, hover_position)
                } else {
                    (hover_position, position)
                }
            } else {
                (position, position)
            };

            let color = obj.color();
            let is_high_contrast = adw::StyleManager::default().is_high_contrast();

            if (played_end - start).abs() > F32_ERROR_MARGIN {
                let rect = graphene::Rect::new(start, 0.0, played_end - start, WAVEFORM_HEIGHT);
                snapshot.append_color(&color, &rect);
            }

            if (hover_end - played_end).abs() > F32_ERROR_MARGIN {
                let color = color.with_alpha(if is_high_contrast { 0.7 } else { 0.45 });

                let rect =
                    graphene::Rect::new(played_end, 0.0, hover_end - played_end, WAVEFORM_HEIGHT);
                snapshot.append_color(&color, &rect);
            }

            if (end - hover_end).abs() > F32_ERROR_MARGIN {
                let color = color.with_alpha(if is_high_contrast { 0.4 } else { 0.2 });

                let rect = graphene::Rect::new(hover_end, 0.0, end - hover_end, WAVEFORM_HEIGHT);
                snapshot.append_color(&color, &rect);
            }

            snapshot.pop();
        }
    }

    impl Waveform {
        /// Set the waveform to display.
        ///
        /// The values must be normalized between 0 and 1.
        pub(super) fn set_waveform(&self, waveform: Vec<f32>) {
            let animate_transition = self.paintable.set_waveform(waveform);
            self.paintable_cache.take();

            if animate_transition {
                self.animation().play();
            }
        }

        /// Set the current position in the audio stream.
        pub(super) fn set_position(&self, position: f32) {
            if (self.position.get() - position).abs() < F32_ERROR_MARGIN {
                return;
            }

            self.position.set(position);

            let obj = self.obj();
            obj.update_property(&[gtk::accessible::Property::ValueNow(position.into())]);
            obj.notify_position();
            obj.queue_draw();
        }

        /// The animation for the waveform change.
        fn animation(&self) -> &adw::TimedAnimation {
            self.animation.get_or_init(|| {
                adw::TimedAnimation::builder()
                    .widget(&*self.obj())
                    .value_to(1.0)
                    .duration(ANIMATION_DURATION)
                    .target(&adw::PropertyAnimationTarget::new(
                        &self.paintable,
                        "transition-progress",
                    ))
                    .easing(adw::Easing::EaseInOutQuad)
                    .build()
            })
        }

        // Get the waveform shape as a monochrome paintable.
        //
        // If we are not in a transition phase, we cache it because the shape only
        // changes if the widget is resized.
        fn paintable(&self) -> Option<gdk::Paintable> {
            let transition_is_ongoing = self
                .animation
                .get()
                .is_some_and(|animation| animation.state() == adw::AnimationState::Playing);

            if !transition_is_ongoing && let Some(paintable) = self.paintable_cache.borrow().clone()
            {
                return Some(paintable);
            }

            let width = self.obj().width() as f32;
            let cache_snapshot = gtk::Snapshot::new();

            self.paintable
                .snapshot(&cache_snapshot, width.into(), WAVEFORM_HEIGHT.into());
            let Some(paintable) =
                cache_snapshot.to_paintable(Some(&graphene::Size::new(width, WAVEFORM_HEIGHT)))
            else {
                error!("Could not convert snapshot to paintable");
                return None;
            };

            if !transition_is_ongoing {
                self.paintable_cache.replace(Some(paintable.clone()));
            }

            Some(paintable)
        }

        /// Convert the given x coordinate on the waveform to a relative
        /// position.
        ///
        /// Takes into account the text direction.
        ///
        /// Returns a value between 0 and 1.
        fn x_coord_to_position(&self, mut x: f64) -> f32 {
            let obj = self.obj();
            let width = f64::from(obj.width());

            // Clamp the value, because it is possible for the user to go outside of the
            // widget with drag gestures.
            x = x.clamp(0.0, width);

            let mut position = (x / width) as f32;

            if obj.direction() == gtk::TextDirection::Rtl {
                position = 1.0 - position;
            }

            position
        }

        /// Emit the `seek` signal with the given new position, if it is
        /// different from the current position.
        fn emit_seek(&self, new_position: f32) {
            if (self.position.get() - new_position).abs() > 0.000_001 {
                self.obj().emit_by_name::<()>("seek", &[&new_position]);
            }
        }

        /// Initialize the event controllers on the waveform.
        fn init_event_controllers(&self) {
            let obj = self.obj();

            // Show mouse hover effect.
            let motion = gtk::EventControllerMotion::builder()
                .name("waveform-motion")
                .build();
            motion.connect_motion(clone!(
                #[weak]
                obj,
                move |_, x, _| {
                    obj.imp().hover_position.set(Some(x as f32));
                    obj.queue_draw();
                }
            ));
            motion.connect_leave(clone!(
                #[weak]
                obj,
                move |_| {
                    obj.imp().hover_position.take();
                    obj.queue_draw();
                }
            ));
            obj.add_controller(motion);

            // Handle dragging to seek. This also handles clicks because a click triggers a
            // drag begin.
            let drag = gtk::GestureDrag::builder().name("waveform-drag").build();
            drag.connect_drag_begin(clone!(
                #[weak]
                obj,
                move |gesture, x, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);

                    if !obj.has_focus() {
                        obj.grab_focus();
                    }

                    let imp = obj.imp();
                    imp.emit_seek(imp.x_coord_to_position(x));
                }
            ));
            drag.connect_drag_update(clone!(
                #[weak]
                obj,
                move |gesture, offset_x, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);

                    if !obj.has_focus() {
                        obj.grab_focus();
                    }

                    let x = gesture
                        .start_point()
                        .expect("ongoing drag should have start point")
                        .0
                        + offset_x;

                    let imp = obj.imp();
                    imp.emit_seek(imp.x_coord_to_position(x));
                }
            ));
            obj.add_controller(drag);

            // Handle left and right key presses to seek.
            let key = gtk::EventControllerKey::builder()
                .name("waveform-key")
                .build();
            key.connect_key_released(clone!(
                #[weak]
                obj,
                move |_, keyval, _, _| {
                    let mut delta = match keyval {
                        gdk::Key::Left | gdk::Key::KP_Left => -0.05,
                        gdk::Key::Right | gdk::Key::KP_Right => 0.05,
                        _ => return,
                    };

                    if obj.direction() == gtk::TextDirection::Rtl {
                        delta = -delta;
                    }

                    let imp = obj.imp();
                    let new_position = (imp.position.get() + delta).clamp(0.0, 1.0);

                    imp.emit_seek(new_position);
                }
            ));
            obj.add_controller(key);
        }
    }
}

glib::wrapper! {
    /// A widget displaying a waveform.
    ///
    /// This widget supports seeking with the keyboard and mouse.
    pub struct Waveform(ObjectSubclass<imp::Waveform>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Waveform {
    /// Create a new empty `Waveform`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set the waveform to display.
    ///
    /// The values must be normalized between 0 and 1.
    pub(crate) fn set_waveform(&self, waveform: Vec<f32>) {
        self.imp().set_waveform(waveform);
    }

    /// Connect to the signal emitted when the user seeks another position.
    pub fn connect_seek<F: Fn(&Self, f32) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "seek",
            true,
            closure_local!(move |obj: Self, position: f32| {
                f(&obj, position);
            }),
        )
    }
}

impl Default for Waveform {
    fn default() -> Self {
        Self::new()
    }
}
