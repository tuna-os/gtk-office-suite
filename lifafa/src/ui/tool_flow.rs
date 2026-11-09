//! The composer's format bar as a wrapping row (#358): the formatting tools
//! flow left to right and wrap at the bar's width, while one end widget (the
//! host's own controls) holds the top right corner. Only the first row
//! leaves room for it, so the rows under it run the bar's full width: a
//! row beside the end widget, all the way down, left a wrapped row short
//! of room it visibly had.

use adw::prelude::*;
use gtk::{glib, subclass::prelude::*};

/// Between two tools on a row.
const SPACING: i32 = 10;
/// Between two rows.
const LINE_SPACING: i32 = 2;

/// Where flow puts one tool.
struct Placed {
    child: gtk::Widget,
    x: i32,
    w: i32,
    h: i32,
    line: usize,
}

/// A laid-out bar: each tool's place, the height of each row, and the end
/// widget's size.
struct Flow {
    placed: Vec<Placed>,
    lines: Vec<i32>,
    end: Option<(i32, i32)>,
}

impl Flow {
    fn line_top(&self, line: usize) -> i32 {
        self.lines[..line].iter().map(|h| h + LINE_SPACING).sum()
    }

    fn height(&self) -> i32 {
        let rows: i32 = self.lines.iter().sum();
        rows + LINE_SPACING * (self.lines.len() as i32 - 1).max(0)
    }
}

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub(crate) struct ToolFlow {
        pub(super) end: RefCell<Option<gtk::Widget>>,
        /// The tools hidden as one (a message written as source), the end
        /// widget staying.
        pub(super) hidden: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ToolFlow {
        const NAME: &'static str = "HylkiToolFlow";
        type Type = super::ToolFlow;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ToolFlow {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ToolFlow {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            if orientation == gtk::Orientation::Horizontal {
                // Narrowest: the widest tool, or the end widget; the tools
                // start on the second row when the first has no room for
                // them beside it. Widest: everything on one row.
                let (mut min, mut nat, mut n) = (0, 0, 0);
                for child in obj.tools() {
                    let (cmin, cnat, _, _) = child.measure(orientation, -1);
                    min = min.max(cmin);
                    nat += cnat;
                    n += 1;
                }
                nat += SPACING * (n - 1).max(0);
                if let Some(end) = obj.end_shown() {
                    let (emin, enat, _, _) = end.measure(orientation, -1);
                    min = min.max(emin);
                    nat += enat + if n > 0 { SPACING } else { 0 };
                }
                (min, nat, -1, -1)
            } else {
                let width = if for_size < 0 {
                    obj.measure(gtk::Orientation::Horizontal, -1).1
                } else {
                    for_size
                };
                let h = obj.flow(width).height();
                (h, h, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let obj = self.obj();
            let flow = obj.flow(width);
            let rtl = obj.direction() == gtk::TextDirection::Rtl;
            for p in &flow.placed {
                let top = flow.line_top(p.line) + (flow.lines[p.line] - p.h) / 2;
                let x = if rtl { width - p.x - p.w } else { p.x };
                p.child
                    .size_allocate(&gtk::Allocation::new(x, top, p.w, p.h), -1);
            }
            if let (Some(end), Some((w, h))) = (obj.end_shown(), flow.end) {
                let x = if rtl { 0 } else { width - w };
                let top = (flow.lines.first().copied().unwrap_or(h) - h) / 2;
                end.size_allocate(&gtk::Allocation::new(x, top, w, h), -1);
            }
        }
    }
}

glib::wrapper! {
    pub(crate) struct ToolFlow(ObjectSubclass<imp::ToolFlow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for ToolFlow {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolFlow {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    /// Add a tool at the end of the flow.
    pub(crate) fn append(&self, child: &impl IsA<gtk::Widget>) {
        let child = child.as_ref();
        match self.imp().end.borrow().as_ref() {
            // Tools stay ahead of the end widget among the children, so
            // focus moves through them first.
            Some(end) => child.insert_before(self, Some(end)),
            None => child.set_parent(self),
        }
        child.set_child_visible(!self.imp().hidden.get());
    }

    /// The widget that holds the top right corner (top left, right to left).
    pub(super) fn set_end(&self, end: &impl IsA<gtk::Widget>) {
        let end = end.as_ref();
        end.set_parent(self);
        self.imp().end.replace(Some(end.clone()));
    }

    /// Show or hide every tool at once; the end widget stays.
    pub(super) fn set_tools_shown(&self, on: bool) {
        self.imp().hidden.set(!on);
        let end = self.imp().end.borrow().clone();
        let mut child = self.first_child();
        while let Some(c) = child {
            if Some(&c) != end.as_ref() {
                c.set_child_visible(on);
            }
            child = c.next_sibling();
        }
        self.queue_resize();
    }

    /// The room left on `child`'s row after it, at the bar's current width.
    pub(super) fn room_after(&self, child: &gtk::Widget) -> Option<i32> {
        let width = self.width();
        let flow = self.flow(width);
        let p = flow.placed.iter().find(|p| &p.child == child)?;
        let row = width - if p.line == 0 { self.reserve() } else { 0 };
        Some(row - (p.x + p.w) - SPACING)
    }

    fn tools(&self) -> Vec<gtk::Widget> {
        let end = self.imp().end.borrow().clone();
        let mut out = Vec::new();
        let mut child = self.first_child();
        while let Some(c) = child {
            // `should_layout` does not look at child-visibility, which is
            // how the tools are hidden: counted, they kept their rows'
            // height as empty space above a source editor.
            if Some(&c) != end.as_ref() && c.should_layout() && c.is_child_visible() {
                out.push(c.clone());
            }
            child = c.next_sibling();
        }
        out
    }

    fn end_shown(&self) -> Option<gtk::Widget> {
        self.imp()
            .end
            .borrow()
            .clone()
            .filter(|e| e.should_layout())
    }

    /// What the first row leaves for the end widget, spacing included.
    fn reserve(&self) -> i32 {
        self.end_shown().map_or(0, |e| {
            e.measure(gtk::Orientation::Horizontal, -1).1 + SPACING
        })
    }

    fn flow(&self, width: i32) -> Flow {
        let end = self.end_shown().map(|e| {
            let w = e
                .measure(gtk::Orientation::Horizontal, -1)
                .1
                .min(width.max(0));
            (w, e.measure(gtk::Orientation::Vertical, w).1)
        });
        let reserve = self.reserve();
        let mut placed = Vec::new();
        let mut heights: Vec<i32> = vec![end.map_or(0, |(_, h)| h)];
        let (mut line, mut x) = (0usize, 0);
        for child in self.tools() {
            let (cmin, cnat, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            let room = |line: usize| width - if line == 0 { reserve } else { 0 };
            // A tool that does not fit after the last one takes the next
            // row; the first row may stay the end widget's alone.
            if (x > 0 && x + cnat.min(room(line)).max(cmin) > room(line))
                || (x == 0 && line == 0 && cmin > room(0))
            {
                line += 1;
                x = 0;
                heights.push(0);
            }
            let w = cnat.min(room(line)).max(cmin);
            let h = child.measure(gtk::Orientation::Vertical, w).1;
            heights[line] = heights[line].max(h);
            placed.push(Placed {
                child,
                x,
                w,
                h,
                line,
            });
            x += w + SPACING;
        }
        if heights.len() > 1 && heights[0] == 0 {
            // Nothing on the first row and no end widget: drop the row.
            heights.remove(0);
            for p in &mut placed {
                p.line -= 1;
            }
        }
        Flow {
            placed,
            lines: heights,
            end,
        }
    }
}
