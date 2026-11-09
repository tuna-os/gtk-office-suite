//! `PillFlow`, the layout that wraps pills and the text into lines.

use super::{SPACING, TEXT_MIN};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};

mod flow_imp {
    use super::*;

    /// Lays the pills out left to right, wrapping, with the text after the
    /// last one taking what is left of its line.
    #[derive(Default)]
    pub(crate) struct PillFlow {
        pub(super) text: RefCell<Option<gtk::Widget>>,
        /// The line showing where a dragged pill would go, outside the
        /// layout.
        pub(super) marker: gtk::Box,
        /// The pill the marker stands before; the number of pills puts it
        /// after the last.
        pub(super) marker_at: Cell<Option<usize>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PillFlow {
        const NAME: &'static str = "HylkiPillFlow";
        type Type = super::PillFlow;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for PillFlow {
        fn constructed(&self) {
            self.parent_constructed();
            self.marker.add_css_class("pill-drop-marker");
            self.marker.set_can_target(false);
            self.marker.set_visible(false);
            self.marker.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            while let Some(c) = self.obj().first_child() {
                c.unparent();
            }
        }
    }

    impl WidgetImpl for PillFlow {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            if orientation == gtk::Orientation::Horizontal {
                let mut min = TEXT_MIN;
                let mut nat = 0;
                for child in obj.children() {
                    let (cmin, cnat, _, _) = child.measure(orientation, -1);
                    min = min.max(cmin);
                    nat += cnat.max(if obj.is_text(&child) { TEXT_MIN } else { 0 }) + SPACING;
                }
                (min, (nat - SPACING).max(min), -1, -1)
            } else {
                let width = if for_size < 0 { i32::MAX / 2 } else { for_size };
                let lines = obj.lines(width);
                let h = lines.last().map_or(0, |l| l.y + l.h);
                // Wrapped lines get room under the last one, which a single
                // line has from the row's own height.
                let h = if lines.len() > 1 { h + SPACING } else { h };
                (h, h, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let obj = self.obj();
            let rtl = obj.direction() == gtk::TextDirection::Rtl;
            // Each child's place before mirroring, for the marker.
            let mut placed = Vec::new();
            for line in obj.lines(width) {
                for (child, x, w, h) in line.items {
                    let y = line.y + (line.h - h) / 2;
                    placed.push((obj.is_text(&child), x, y, w, h));
                    let x = if rtl { width - x - w } else { x };
                    child.size_allocate(&gtk::Allocation::new(x, y, w, h), -1);
                }
            }
            if let Some(at) = self.marker_at.get() {
                let pills: Vec<_> = placed.iter().filter(|p| !p.0).collect();
                let (w, _, _, _) = self.marker.measure(gtk::Orientation::Horizontal, -1);
                let w = w.max(2);
                // In the gap before the pill, or after the last one, or at
                // the start of the text when there is none.
                let (x, y, h) = match (pills.get(at), pills.last(), placed.iter().find(|p| p.0)) {
                    (Some(p), _, _) => (p.1 - (SPACING + w) / 2, p.2, p.4),
                    (None, Some(p), _) => (p.1 + p.3 + (SPACING - w) / 2, p.2, p.4),
                    (None, None, Some(t)) => (t.1, t.2, t.4),
                    _ => (0, 0, 0),
                };
                let x = x.clamp(0, (width - w).max(0));
                let x = if rtl { width - x - w } else { x };
                self.marker
                    .size_allocate(&gtk::Allocation::new(x, y, w, h), -1);
            }
        }

        /// Tab goes to the text and on out of the field, never through the
        /// pills, which the arrow keys reach from the text.
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            match direction {
                gtk::DirectionType::TabForward | gtk::DirectionType::TabBackward => {
                    if self.obj().focus_child().is_some() {
                        return false;
                    }
                    self.text.borrow().as_ref().is_some_and(|t| t.grab_focus())
                }
                _ => self.parent_focus(direction),
            }
        }
    }
}

glib::wrapper! {
    pub(crate) struct PillFlow(ObjectSubclass<flow_imp::PillFlow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for PillFlow {
    fn default() -> Self {
        glib::Object::new()
    }
}

/// One line of the flow: each child with its x, width and height.
struct Line {
    y: i32,
    h: i32,
    items: Vec<(gtk::Widget, i32, i32, i32)>,
}

impl PillFlow {
    /// The text, always the last child.
    pub(super) fn set_text_child(&self, text: &impl IsA<gtk::Widget>) {
        text.set_parent(self);
        self.imp().text.replace(Some(text.clone().upcast()));
    }

    /// Put a pill before `next`, or after the others; a pill already here
    /// moves.
    pub(super) fn place(&self, pill: &impl IsA<gtk::Widget>, next: Option<&gtk::Widget>) {
        let text = self.imp().text.borrow().clone();
        pill.insert_before(self, next.or(text.as_ref()));
        self.queue_resize();
    }

    /// Show the drop marker before pill `at`, or hide it.
    pub(super) fn set_marker(&self, at: Option<usize>) {
        let imp = self.imp();
        if imp.marker_at.get() == at {
            return;
        }
        imp.marker_at.set(at);
        imp.marker.set_visible(at.is_some());
        self.queue_allocate();
    }

    pub(super) fn remove(&self, child: &impl IsA<gtk::Widget>) {
        if child.parent().as_ref() == Some(self.upcast_ref()) {
            child.unparent();
            self.queue_resize();
        }
    }

    fn is_text(&self, child: &gtk::Widget) -> bool {
        self.imp().text.borrow().as_ref() == Some(child)
    }

    fn children(&self) -> Vec<gtk::Widget> {
        let mut out = Vec::new();
        let mut child = self.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c.is_visible() && c != *self.imp().marker.upcast_ref::<gtk::Widget>() {
                out.push(c);
            }
        }
        out
    }

    fn lines(&self, width: i32) -> Vec<Line> {
        let mut lines: Vec<Line> = Vec::new();
        let mut cur = Line {
            y: 0,
            h: 0,
            items: Vec::new(),
        };
        let mut x = 0;
        for child in self.children() {
            let (_, nat, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            let text = self.is_text(&child);
            // The text needs room to type in; a pill as much as its name,
            // cut short at the full width.
            let need = if text { TEXT_MIN } else { nat.min(width) };
            if x > 0 && x + need > width {
                let y = cur.y + cur.h + SPACING;
                lines.push(std::mem::replace(
                    &mut cur,
                    Line {
                        y,
                        h: 0,
                        items: Vec::new(),
                    },
                ));
                x = 0;
            }
            let w = if text { (width - x).max(need) } else { need };
            let (_, h, _, _) = child.measure(gtk::Orientation::Vertical, w);
            cur.h = cur.h.max(h);
            cur.items.push((child, x, w, h));
            x += w + SPACING;
        }
        lines.push(cur);
        lines
    }
}
