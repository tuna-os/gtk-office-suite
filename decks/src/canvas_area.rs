// canvas_area.rs — DrawingArea subclass exposing slide objects as
// virtual AT-SPI children (issue #87, deep half — Decks side).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Same pattern as Tables' grid_area.rs: plain GObjects implementing
// GtkAccessible, registered with set_accessible_parent so the AT-SPI
// bridge notices dynamic children.

use gtk4::{self as gtk, glib, prelude::*, subclass::prelude::*};
use std::cell::RefCell;

use crate::canvas::slide_geometry;
use decks_core::engine::SlideObject;

// ── ObjectAccessible: one slide object ───────────────────────────────

mod imp_obj {
    use super::*;
    use std::cell::{Cell, OnceCell};

    #[derive(Default)]
    pub struct ObjectAccessible {
        pub canvas: OnceCell<glib::WeakRef<super::CanvasArea>>,
        pub index: Cell<usize>,
        /// Slide-coordinate rect (x, y, w, h).
        pub rect: Cell<(f64, f64, f64, f64)>,
        pub context: RefCell<Option<gtk::ATContext>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ObjectAccessible {
        const NAME: &'static str = "DecksObjectAccessible";
        type Type = super::ObjectAccessible;
        type ParentType = glib::Object;
        type Interfaces = (gtk::Accessible,);
    }

    impl ObjectImpl for ObjectAccessible {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPS: std::sync::OnceLock<Vec<glib::ParamSpec>> = std::sync::OnceLock::new();
            PROPS.get_or_init(|| {
                vec![glib::ParamSpecOverride::for_interface::<gtk::Accessible>(
                    "accessible-role",
                )]
            })
        }
        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "accessible-role" => gtk::AccessibleRole::ListItem.to_value(),
                _ => unimplemented!(),
            }
        }
        fn set_property(&self, _id: usize, _value: &glib::Value, _pspec: &glib::ParamSpec) {
            // The role is fixed at ListItem.
        }
    }

    impl AccessibleImpl for ObjectAccessible {
        fn at_context(&self) -> Option<gtk::ATContext> {
            if let Some(ctx) = self.context.borrow().as_ref() {
                return Some(ctx.clone());
            }
            let canvas = self.canvas.get()?.upgrade()?;
            let ctx = gtk::ATContext::create(
                gtk::AccessibleRole::ListItem,
                self.obj().upcast_ref::<gtk::Accessible>(),
                &canvas.display(),
            )?;
            *self.context.borrow_mut() = Some(ctx.clone());
            Some(ctx)
        }

        fn accessible_parent(&self) -> Option<gtk::Accessible> {
            self.canvas
                .get()
                .and_then(|w| w.upgrade())
                .map(|c| c.upcast::<gtk::Accessible>())
        }

        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            None
        }

        fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
            let canvas = self.canvas.get()?.upgrade()?;
            let (ox, oy, sw, sh) = slide_geometry(canvas.width() as f64, canvas.height() as f64);
            let (x, y, w, h) = self.rect.get();
            Some((
                (ox + x / 960.0 * sw) as i32,
                (oy + y / 540.0 * sh) as i32,
                (w / 960.0 * sw) as i32,
                (h / 540.0 * sh) as i32,
            ))
        }

        fn platform_state(&self, _state: gtk::AccessiblePlatformState) -> bool {
            false
        }
    }
}

glib::wrapper! {
    pub struct ObjectAccessible(ObjectSubclass<imp_obj::ObjectAccessible>)
        @implements gtk::Accessible;
}

impl ObjectAccessible {
    fn new(canvas: &CanvasArea, index: usize) -> Self {
        let obj: Self = glib::Object::builder().build();
        let weak = glib::WeakRef::new();
        weak.set(Some(canvas));
        obj.imp().canvas.set(weak).ok();
        obj.imp().index.set(index);
        obj
    }

    fn update(&self, object: &SlideObject, selected: bool) {
        let (name, rect) = match object {
            SlideObject::TextBox { text, x, y, w, h, .. } => {
                let label = if text.trim().is_empty() {
                    "Text box, empty".to_string()
                } else {
                    format!("Text box: {}", text.replace('\n', " "))
                };
                (label, (*x, *y, *w, *h))
            }
            SlideObject::Rect { x, y, w, h } => ("Rectangle".to_string(), (*x, *y, *w, *h)),
            SlideObject::Circle { x, y, r } => {
                ("Circle".to_string(), (x - r, y - r, r * 2.0, r * 2.0))
            }
            SlideObject::Image { path, x, y, w, h } => {
                let file = std::path::Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                (format!("Image: {file}"), (*x, *y, *w, *h))
            }
        };
        self.imp().rect.set(rect);
        self.update_property(&[gtk::accessible::Property::Label(&name)]);
        self.update_state(&[gtk::accessible::State::Selected(Some(selected))]);
    }
}

// ── CanvasArea: the slide canvas exposing its objects ────────────────

mod imp_canvas {
    use super::*;

    #[derive(Default)]
    pub struct CanvasArea {
        pub objects: RefCell<Vec<ObjectAccessible>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CanvasArea {
        const NAME: &'static str = "DecksCanvasArea";
        type Type = super::CanvasArea;
        type ParentType = gtk::DrawingArea;
        // Required to register the AccessibleImpl override below.
        type Interfaces = (gtk::Accessible,);
    }

    impl ObjectImpl for CanvasArea {}
    impl WidgetImpl for CanvasArea {}
    impl DrawingAreaImpl for CanvasArea {}

    impl AccessibleImpl for CanvasArea {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            self.objects
                .borrow()
                .first()
                .map(|o| o.clone().upcast::<gtk::Accessible>())
        }
    }
}

glib::wrapper! {
    pub struct CanvasArea(ObjectSubclass<imp_canvas::CanvasArea>)
        @extends gtk::DrawingArea, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for CanvasArea {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}

impl CanvasArea {
    /// Mirror the current slide's objects as virtual a11y children.
    /// Accessibles are persistent: appended children are linked in with
    /// update_next_accessible_sibling (the dynamic-children protocol —
    /// rebuilding the whole chain leaves the bridge holding stale
    /// references and it reports no children at all); surplus children
    /// are hidden, never destroyed.
    pub fn sync_objects(&self, objects: &[SlideObject], selected: Option<usize>) {
        loop {
            let (len, prev) = {
                let list = self.imp().objects.borrow();
                (list.len(), list.last().cloned())
            };
            if len >= objects.len() {
                break;
            }
            let acc = ObjectAccessible::new(self, len);
            self.imp().objects.borrow_mut().push(acc.clone());
            acc.set_accessible_parent(Some(self), None::<&ObjectAccessible>);
            if let Some(prev) = prev {
                prev.update_next_accessible_sibling(Some(&acc));
            }
        }
        let list = self.imp().objects.borrow();
        for (i, acc) in list.iter().enumerate() {
            match objects.get(i) {
                Some(obj) => {
                    acc.update_state(&[gtk::accessible::State::Hidden(false)]);
                    acc.update(obj, selected == Some(i));
                }
                None => acc.update_state(&[gtk::accessible::State::Hidden(true)]),
            }
        }
    }
}
