use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, graphene, gsk};

mod imp {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::CropCircle)]
    pub struct CropCircle {
        /// The child widget to crop.
        #[property(get, set = Self::set_child, explicit_notify, nullable)]
        child: RefCell<Option<gtk::Widget>>,
        /// Whether the child should be cropped.
        #[property(get, set = Self::set_is_cropped, explicit_notify)]
        is_cropped: Cell<bool>,
        /// The width that should be cropped.
        ///
        /// This is the number of pixels from the right edge of the child
        /// widget.
        #[property(get, set = Self::set_cropped_width, explicit_notify)]
        cropped_width: Cell<u32>,
        mask: adw::Bin,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CropCircle {
        const NAME: &'static str = "CropCircle";
        type Type = super::CropCircle;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("crop-circle");
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CropCircle {
        fn constructed(&self) {
            self.parent_constructed();

            self.mask.set_parent(&*self.obj());
            self.mask.add_css_class("mask");
            self.mask
                .set_accessible_role(gtk::AccessibleRole::Presentation);
        }

        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }

            self.mask.unparent();
        }
    }

    impl WidgetImpl for CropCircle {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if let Some(child) = self.child.borrow().as_ref() {
                return child.measure(orientation, for_size);
            }

            (0, 0, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.child.borrow().clone() else {
                return;
            };

            child.allocate(width, height, baseline, None);

            let (_, child_size) = child.preferred_size();

            // The x position at the right edge of the child.
            let mut x = width.midpoint(child_size.width());

            if self.is_cropped.get() {
                let cropped_width = self
                    .cropped_width
                    .get()
                    .try_into()
                    .expect("width fits into an i32");
                x = x.saturating_sub(cropped_width);
            }

            let transform = gsk::Transform::new().translate(&graphene::Point::new(x as f32, 0.0));
            self.mask.allocate(width, height, baseline, Some(transform));
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let borrow = self.child.borrow();
            let Some(child) = borrow.as_ref() else {
                return;
            };

            let obj = self.obj();

            if !self.is_cropped.get() || self.cropped_width.get() == 0 {
                obj.snapshot_child(child, snapshot);
                return;
            }

            snapshot.push_mask(gsk::MaskMode::InvertedAlpha);

            obj.snapshot_child(&self.mask, snapshot);
            snapshot.pop();

            obj.snapshot_child(child, snapshot);
            snapshot.pop();
        }
    }

    impl CropCircle {
        /// Set the child widget to crop.
        fn set_child(&self, child: Option<gtk::Widget>) {
            let prev_child = self.child.borrow().clone();

            if prev_child == child {
                return;
            }
            let obj = self.obj();

            if let Some(child) = prev_child {
                child.unparent();
            }

            if let Some(child) = &child {
                child.set_parent(&*obj);
            }

            self.child.replace(child);

            obj.queue_resize();
            obj.notify_child();
        }

        /// Set whether the child widget should be cropped.
        fn set_is_cropped(&self, is_cropped: bool) {
            if self.is_cropped.get() == is_cropped {
                return;
            }
            let obj = self.obj();

            self.is_cropped.set(is_cropped);

            obj.queue_allocate();
            obj.notify_is_cropped();
        }

        /// Set the width that should be cropped.
        fn set_cropped_width(&self, width: u32) {
            if self.cropped_width.get() == width {
                return;
            }
            let obj = self.obj();

            self.cropped_width.set(width);

            obj.queue_allocate();
            obj.notify_cropped_width();
        }
    }
}

glib::wrapper! {
    /// A widget that crops its child with a circle.
    pub struct CropCircle(ObjectSubclass<imp::CropCircle>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CropCircle {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
