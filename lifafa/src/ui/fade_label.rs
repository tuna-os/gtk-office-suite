//! A one-line label that fades out where it is cut off instead of ending in
//! an ellipsis, and slides to show the rest while the pointer is over it
//! (#299, the composer's attachment chips).
//!
//! The fade is a mask over the text rather than a gradient painted on top,
//! so it reads the same on any background, hover highlights included. While
//! the text slides, the start fades on the left the way the end did on the
//! right.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use gtk::{gdk, glib, graphene, gsk, subclass::prelude::*};

/// Width of the fade at a cut edge.
const FADE: f32 = 24.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub(crate) struct FadeLabel {
        pub(super) label: RefCell<Option<gtk::Label>>,
        /// How far the text is slid to the left, in pixels.
        pub(super) offset: Cell<f64>,
        /// The natural width asked of the parent at most: past it the text
        /// fades rather than widening its row.
        pub(super) max_natural: Cell<i32>,
        pub(super) anim: RefCell<Option<adw::TimedAnimation>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FadeLabel {
        const NAME: &'static str = "HylkiFadeLabel";
        type Type = super::FadeLabel;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for FadeLabel {
        fn dispose(&self) {
            if let Some(a) = self.anim.take() {
                a.pause();
            }
            if let Some(l) = self.label.take() {
                l.unparent();
            }
        }
    }

    impl WidgetImpl for FadeLabel {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(label) = self.label.borrow().clone() else {
                return (0, 0, -1, -1);
            };
            let (min, nat, base_min, base_nat) = label.measure(orientation, for_size);
            if orientation == gtk::Orientation::Horizontal {
                // Any width will do: whatever does not fit fades.
                (0, nat.min(self.max_natural.get()), -1, -1)
            } else {
                (min, nat, base_min, base_nat)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(label) = self.label.borrow().clone() else {
                return;
            };
            let (_, nat, _, _) = label.measure(gtk::Orientation::Horizontal, -1);
            let full = nat.max(width);
            let overflow = (full - width).max(0) as f64;
            // A resize can leave the slide past the new end.
            if self.offset.get() > overflow {
                self.offset.set(overflow);
            }
            let x = -self.offset.get().round() as f32;
            let t = gsk::Transform::new().translate(&graphene::Point::new(x, 0.0));
            label.allocate(full, height, baseline, Some(t));
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(label) = self.label.borrow().clone() else {
                return;
            };
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            let overflow = obj.overflow_px();
            if overflow <= 0.0 || w <= 0.0 {
                obj.snapshot_child(&label, snapshot);
                return;
            }
            let offset = self.offset.get() as f32;
            // Each fade is as deep as the text hidden past its edge, up to
            // FADE, so it comes and goes smoothly as the text slides.
            let left = (offset / FADE).clamp(0.0, 1.0);
            let right = ((overflow - offset) / FADE).clamp(0.0, 1.0);
            let edge = (FADE / w).min(0.5);
            let stop =
                |at: f32, alpha: f32| gsk::ColorStop::new(at, gdk::RGBA::new(0.0, 0.0, 0.0, alpha));
            let bounds = graphene::Rect::new(0.0, 0.0, w, h);
            snapshot.push_clip(&bounds);
            snapshot.push_mask(gsk::MaskMode::Alpha);
            snapshot.append_linear_gradient(
                &bounds,
                &graphene::Point::new(0.0, 0.0),
                &graphene::Point::new(w, 0.0),
                &[
                    stop(0.0, 1.0 - left),
                    stop(edge, 1.0),
                    stop(1.0 - edge, 1.0),
                    stop(1.0, 1.0 - right),
                ],
            );
            snapshot.pop();
            obj.snapshot_child(&label, snapshot);
            snapshot.pop();
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub(crate) struct FadeLabel(ObjectSubclass<imp::FadeLabel>)
        @extends gtk::Widget;
}

impl FadeLabel {
    /// `max_natural` caps the width the label asks for; it takes any width
    /// it is given.
    pub(crate) fn new(text: &str, max_natural: i32) -> Self {
        let obj: Self = glib::Object::new();
        let label = gtk::Label::new(Some(text));
        label.set_xalign(0.0);
        label.set_single_line_mode(true);
        label.set_parent(&obj);
        obj.imp().label.replace(Some(label));
        obj.imp().max_natural.set(max_natural);
        obj.set_overflow(gtk::Overflow::Hidden);
        obj
    }

    /// How much of the text is out of sight at rest, in pixels.
    fn overflow_px(&self) -> f32 {
        let Some(label) = self.imp().label.borrow().clone() else {
            return 0.0;
        };
        (label.width() - self.width()).max(0) as f32
    }

    /// Slide the text to show its end (`true`) or back to its start. Speed
    /// is steady, so a long name takes longer than one a few letters over.
    pub(crate) fn reveal(&self, on: bool) {
        let imp = self.imp();
        if let Some(a) = imp.anim.take() {
            a.pause();
        }
        let overflow = self.overflow_px() as f64;
        let from = imp.offset.get();
        let to = if on { overflow } else { 0.0 };
        if (to - from).abs() < 0.5 {
            return;
        }
        // About 70 px a second out, faster back; never a crawl or a jump.
        let ms = if on {
            (to - from).abs() * 14.0
        } else {
            (to - from).abs() * 5.0
        };
        let weak = self.downgrade();
        let target = adw::CallbackAnimationTarget::new(move |v| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().offset.set(v);
                obj.queue_allocate();
                obj.queue_draw();
            }
        });
        let anim = adw::TimedAnimation::new(self, from, to, ms.clamp(250.0, 4000.0) as u32, target);
        anim.set_easing(adw::Easing::EaseInOutCubic);
        // Hold the start for a moment, so a pointer passing over does not set
        // every name it crosses sliding.
        if on {
            let weak = anim.downgrade();
            glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
                if let Some(a) = weak.upgrade() {
                    a.play();
                }
            });
        } else {
            anim.play();
        }
        imp.anim.replace(Some(anim));
    }
}
