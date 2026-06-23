// SPDX-License-Identifier: GPL-3.0-or-later
//
// Ruler — horizontal ruler widget with tick marks, margin/indent markers,
// and tab stop management. Syncs with GtkTextView indentation.
//
// Architecture:
//   - Custom GTK Widget using Cairo drawing (via snapshot + append_cairo)
//   - GestureClick for drag operations on margin/indent/tab markers
//   - EventControllerMotion for hover detection
//   - Updates from GtkTextView cursor position changes
//
// Reference: AbiWord ruler widget (concept only)

use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4::subclass::prelude::*;
use std::cell::{Cell, RefCell};

const RULER_HEIGHT: i32 = 28;
const TICK_MAJOR_INTERVAL: f64 = 72.0; // 1 inch in points
const TICK_MINOR_INTERVAL: f64 = 18.0;  // ~1/4 inch

// ── GObject subclass ───────────────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Default, Clone, Debug)]
    pub struct TabStop {
        pub position_pt: f64,
        pub alignment: TabAlignment,
    }

    #[derive(Default, Clone, Debug, PartialEq)]
    pub enum TabAlignment {
        #[default]
        Left,
        Center,
        Right,
    }

    #[derive(Default)]
    pub struct Ruler {
        // Page geometry (in points)
        pub page_width: Cell<f64>,
        pub margin_left: Cell<f64>,
        pub margin_right: Cell<f64>,
        pub first_line_indent: Cell<f64>,
        pub left_indent: Cell<f64>,
        pub tab_stops: RefCell<Vec<TabStop>>,
        // Unit system
        pub use_metric: Cell<bool>,       // true = cm, false = inches
        // Drag state
        pub dragging: Cell<Option<DragTarget>>,
        pub last_x: Cell<f64>,
        pub on_changed: RefCell<Option<Box<dyn Fn()>>>,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum DragTarget {
        MarginLeft,
        MarginRight,
        FirstLineIndent,
        LeftIndent,
        TabStop(usize),
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Ruler {
        const NAME: &'static str = "Ruler";
        type Type = super::Ruler;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("ruler");
        }
    }

    impl ObjectImpl for Ruler {
        fn constructed(&self) {
            self.parent_constructed();
            self.page_width.set(595.0); // A4 width
            self.margin_left.set(72.0);
            self.margin_right.set(72.0);
        }
    }

    impl WidgetImpl for Ruler {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let w = self.obj().width() as f64;
            let h = self.obj().height() as f64;
            if w <= 0.0 || h <= 0.0 { return; }

            let pw = self.page_width.get();
            let ml = self.margin_left.get();

            // Scale: map page width → widget width
            let scale = w / pw;

            let cr = snapshot.append_cairo(&gtk4::graphene::Rect::new(
                0.0, 0.0, w as f32, h as f32,
            ));

            let ruler_h = h;

            // ── Background ──
            cr.set_source_rgb(0.95, 0.95, 0.93);
            cr.rectangle(0.0, 0.0, w, ruler_h);
            cr.fill().unwrap();

            // ── Bottom border ──
            cr.set_source_rgb(0.7, 0.7, 0.7);
            cr.set_line_width(1.0);
            cr.move_to(0.0, ruler_h - 0.5);
            cr.line_to(w, ruler_h - 0.5);
            cr.stroke().unwrap();

            // ── Tick marks ──
            let interval = if self.use_metric.get() { 28.35 } else { TICK_MAJOR_INTERVAL }; // 1cm in pts
            let minor = interval / 4.0;

            let mut pos = 0.0;
            let mut tick_idx = 0;

            while pos <= pw {
                let x = (pos - ml) * scale;
                if x < -20.0 || x > w + 20.0 { pos += minor; tick_idx += 1; continue; }

                let is_major = tick_idx % 4 == 0;
                let tick_top = if is_major { 4.0 } else { 12.0 };
                let tick_bottom = ruler_h - 4.0;

                cr.set_source_rgb(0.5, 0.5, 0.5);
                cr.set_line_width(if is_major { 1.0 } else { 0.5 });
                cr.move_to(x, tick_top);
                cr.line_to(x, tick_bottom);
                cr.stroke().unwrap();

                // Major tick labels
                if is_major {
                    let label = if self.use_metric.get() {
                        format!("{}", tick_idx / 4 + 1)
                    } else {
                        format!("{}", tick_idx / 4 + 1)
                    };
                    cr.set_source_rgb(0.3, 0.3, 0.3);
                    cr.set_font_size(9.0);
                    cr.move_to(x - 6.0, 10.0);
                    let _ = cr.show_text(&label);
                }

                pos += minor;
                tick_idx += 1;
            }

            // ── Margin shading (outside page margins) ──
            cr.set_source_rgba(0.85, 0.85, 0.85, 0.4);
            // Left margin area
            let left_margin_x = 0.0;
            cr.rectangle(left_margin_x, 0.0, (ml * scale).max(0.0), ruler_h);
            cr.fill().unwrap();
            // Right margin area
            let mr = self.margin_right.get();
            let right_margin_x = (pw - mr) * scale;
            cr.rectangle(right_margin_x, 0.0, w - right_margin_x, ruler_h);
            cr.fill().unwrap();

            // ── Right margin marker (triangle at right edge) ──
            {
                let rx = (pw - mr) * scale;
                cr.set_source_rgb(0.4, 0.4, 0.4);
                cr.move_to(rx, ruler_h);
                cr.line_to(rx - 5.0, ruler_h - 10.0);
                cr.line_to(rx + 5.0, ruler_h - 10.0);
                cr.close_path();
                cr.fill().unwrap();
            }

            // ── Indent markers (triangles) ──
            let fl = self.first_line_indent.get();
            let li = self.left_indent.get();

            // First-line indent (top triangle pointing down)
            {
                let ix = (fl - ml) * scale;
                cr.set_source_rgb(0.3, 0.3, 0.3);
                cr.move_to(ix, 0.0);
                cr.line_to(ix - 5.0, 10.0);
                cr.line_to(ix + 5.0, 10.0);
                cr.close_path();
                cr.fill().unwrap();
            }

            // Left indent (bottom triangle pointing up)
            {
                let ix = (li - ml) * scale;
                cr.set_source_rgb(0.3, 0.3, 0.3);
                cr.move_to(ix, ruler_h);
                cr.line_to(ix - 5.0, ruler_h - 10.0);
                cr.line_to(ix + 5.0, ruler_h - 10.0);
                cr.close_path();
                cr.fill().unwrap();
            }

            // ── Tab stops (L-shaped) ──
            for (i, ts) in self.tab_stops.borrow().iter().enumerate() {
                let tx = (ts.position_pt - ml) * scale;
                if tx < 0.0 || tx > w { continue; }
                cr.set_source_rgb(0.4, 0.4, 0.4);
                cr.set_line_width(1.0);
                cr.move_to(tx, ruler_h - 12.0);
                cr.line_to(tx, ruler_h - 4.0);
                cr.stroke().unwrap();
                cr.move_to(tx, ruler_h - 12.0);
                cr.line_to(tx + 4.0, ruler_h - 12.0);
                cr.stroke().unwrap();
            }
        }

        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (RULER_HEIGHT, RULER_HEIGHT, -1, -1)
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            // Force height to RULER_HEIGHT
            // Width comes from parent allocation
        }
    }
}

glib::wrapper! {
    pub struct Ruler(ObjectSubclass<imp::Ruler>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Ruler {
    pub fn new() -> Self {
        let obj: Self = glib::Object::builder().build();

        // ── Click handler: detect hit targets ──
        {
            let click = gtk::GestureClick::new();
            let ruler_weak = obj.downgrade();
            click.connect_pressed(move |gesture, _n_press, x, y| {
                let ruler = ruler_weak.upgrade().unwrap();
                ruler.handle_press(gesture, x, y);
            });
            obj.add_controller(click);
        }

        // ── Motion handler: update during drag ──
        {
            let motion = gtk::EventControllerMotion::new();
            let ruler_weak = obj.downgrade();
            motion.connect_motion(move |_ctrl, x, y| {
                if let Some(ruler) = ruler_weak.upgrade() {
                    ruler.handle_motion(x, y);
                }
            });
            obj.add_controller(motion);
        }

        // ── Right-click handler: remove tab stop ──
        {
            let click = gtk::GestureClick::new();
            click.set_button(3); // right button
            let ruler_weak = obj.downgrade();
            click.connect_pressed(move |_gesture, _n_press, x, _y| {
                if let Some(ruler) = ruler_weak.upgrade() {
                    ruler.handle_right_click(x);
                }
            });
            obj.add_controller(click);
        }

        // ── Release handler: end drag ──
        {
            let ruler_weak = obj.downgrade();
            let release = gtk::GestureClick::new();
            release.connect_released(move |_gesture, _n_press, _x, _y| {
                if let Some(ruler) = ruler_weak.upgrade() {
                    ruler.imp().dragging.set(None);
                    if let Some(ref cb) = *ruler.imp().on_changed.borrow() {
                        cb();
                    }
                }
            });
            obj.add_controller(release);
        }

        obj
    }

    /// Set page width in points.
    pub fn set_page_width(&self, pt: f64) {
        self.imp().page_width.set(pt);
        self.queue_draw();
    }

    /// Set margin positions in points.
    pub fn set_margins(&self, left: f64, right: f64) {
        let imp = self.imp();
        imp.margin_left.set(left);
        imp.margin_right.set(right);
        self.queue_draw();
    }

    /// Set indent positions in points.
    pub fn set_indents(&self, first_line: f64, left: f64) {
        let imp = self.imp();
        imp.first_line_indent.set(first_line);
        imp.left_indent.set(left);
        self.queue_draw();
    }

    /// Set tab stops.
    pub fn set_tab_stops(&self, tabs: &[f64]) {
        let imp = self.imp();
        imp.tab_stops.replace(
            tabs.iter().map(|p| imp::TabStop { position_pt: *p, alignment: imp::TabAlignment::Left }).collect()
        );
        self.queue_draw();
    }

    /// Get current margin values for syncing.
    pub fn margin_left(&self) -> f64 { self.imp().margin_left.get() }
    pub fn margin_right(&self) -> f64 { self.imp().margin_right.get() }
    pub fn first_line_indent(&self) -> f64 { self.imp().first_line_indent.get() }
    pub fn left_indent(&self) -> f64 { self.imp().left_indent.get() }

    /// Set unit system (true = metric/cm, false = imperial/inches).
    pub fn set_metric(&self, metric: bool) {
        self.imp().use_metric.set(metric);
        self.queue_draw();
    }

    /// Load initial values from GSettings.
    pub fn load_from_settings(&self, settings: &gio::Settings) {
        self.set_page_width(settings.double("page-width-pt"));
        self.set_margins(
            settings.double("page-margin-left"),
            settings.double("page-margin-right"),
        );
        self.set_indents(
            settings.double("page-margin-left"),  // first-line defaults to left margin
            settings.double("page-margin-left"),  // left indent defaults to left margin
        );
        self.set_metric(settings.boolean("ruler-metric"));
    }

    /// Connect a callback that fires when any drag operation finishes.
    /// Use this to sync ruler state back to GSettings or the editor.
    pub fn connect_changed<F: Fn() + 'static>(&self, f: F) {
        self.imp().on_changed.replace(Some(Box::new(f)));
    }

    // ── Internal hit testing & drag ─────────────────────────────────

    fn pt_to_x(&self, pt: f64) -> f64 {
        let imp = self.imp();
        let w = self.width() as f64;
        let pw = imp.page_width.get();
        if pw <= 0.0 { return 0.0; }
        let scale = w / pw;
        let ml = imp.margin_left.get();
        (pt - ml) * scale
    }

    fn x_to_pt(&self, x: f64) -> f64 {
        let imp = self.imp();
        let w = self.width() as f64;
        let pw = imp.page_width.get();
        if pw <= 0.0 { return 0.0; }
        let scale = w / pw;
        let ml = imp.margin_left.get();
        (x / scale) + ml
    }

    fn handle_press(&self, _gesture: &gtk::GestureClick, x: f64, y: f64) {
        let imp = self.imp();
        let h = self.height() as f64;
        let pt = self.x_to_pt(x);
        let ml = imp.margin_left.get();
        let mr = imp.margin_right.get();
        let fl = imp.first_line_indent.get();
        let li = imp.left_indent.get();
        let pw = imp.page_width.get();

        // Check left margin (left edge of page)
        let mlx = self.pt_to_x(ml);
        if y > h - 20.0 && (x - mlx).abs() < 6.0 {
            imp.dragging.set(Some(DragTarget::MarginLeft));
            imp.last_x.set(pt);
            return;
        }

        // Check right margin
        let mrx = self.pt_to_x(pw - mr);
        if y > h - 20.0 && (x - mrx).abs() < 6.0 {
            imp.dragging.set(Some(DragTarget::MarginRight));
            imp.last_x.set(pt);
            return;
        }

        // Check first-line indent (top triangle)
        let flx = self.pt_to_x(fl);
        if y < 12.0 && (x - flx).abs() < 8.0 {
            imp.dragging.set(Some(DragTarget::FirstLineIndent));
            imp.last_x.set(pt);
            return;
        }

        // Check left indent (bottom triangle)
        let lix = self.pt_to_x(li);
        if y > h - 12.0 && (x - lix).abs() < 8.0 {
            imp.dragging.set(Some(DragTarget::LeftIndent));
            imp.last_x.set(pt);
            return;
        }

        // Check tab stops
        for (i, ts) in imp.tab_stops.borrow().iter().enumerate() {
            let tx = self.pt_to_x(ts.position_pt);
            if y > h - 16.0 && (x - tx).abs() < 6.0 {
                imp.dragging.set(Some(DragTarget::TabStop(i)));
                imp.last_x.set(pt);
                return;
            }
        }

        // Click on empty space: add tab stop
        if y > h - 20.0 {
            let mut tabs = imp.tab_stops.borrow_mut();
            tabs.push(imp::TabStop { position_pt: pt, alignment: imp::TabAlignment::Left });
            tabs.sort_by(|a, b| a.position_pt.partial_cmp(&b.position_pt).unwrap());
            self.queue_draw();
        }
    }

    fn handle_motion(&self, x: f64, _y: f64) {
        let imp = self.imp();
        if let Some(target) = imp.dragging.get() {
            let pt = self.x_to_pt(x);
            let min_pt = 0.0;
            let max_pt = imp.page_width.get();

            match target {
                DragTarget::MarginLeft => {
                    let mr = imp.margin_right.get();
                    let val = pt.clamp(min_pt, max_pt - mr - 10.0);
                    imp.margin_left.set(val);
                    imp.first_line_indent.set(val);
                    imp.left_indent.set(val);
                }
                DragTarget::MarginRight => {
                    let ml = imp.margin_left.get();
                    let val = (imp.page_width.get() - pt).clamp(min_pt, max_pt - ml - 10.0);
                    imp.margin_right.set(val);
                }
                DragTarget::FirstLineIndent => {
                    imp.first_line_indent.set(pt.clamp(min_pt, max_pt));
                }
                DragTarget::LeftIndent => {
                    imp.left_indent.set(pt.clamp(min_pt, max_pt));
                }
                DragTarget::TabStop(i) => {
                    let mut tabs = imp.tab_stops.borrow_mut();
                    if i < tabs.len() {
                        tabs[i].position_pt = pt.clamp(min_pt, max_pt);
                    }
                }
            }
            self.queue_draw();
        }
    }

    fn handle_right_click(&self, x: f64) {
        let imp = self.imp();
        let pt = self.x_to_pt(x);
        let mut tabs = imp.tab_stops.borrow_mut();
        if let Some(idx) = tabs.iter().position(|t| (t.position_pt - pt).abs() < 8.0) {
            tabs.remove(idx);
            self.queue_draw();
        }
    }
}

// Re-export
pub use imp::{DragTarget, TabStop, TabAlignment};

impl Default for Ruler {
    fn default() -> Self {
        Self::new()
    }
}
