// SPDX-License-Identifier: GPL-3.0-or-later
//
// DecksWindow — Presentation window with slide sidebar + Cairo canvas.
//
// Architecture (gnome-gui-spec):
//   AdwApplicationWindow
//   └── AdwToolbarView (raised)
//       ├── AdwHeaderBar [start: Open/Save/Present] [end: Menu]
//       ├── SuiteToolbar (B/I/U tools + shape/text/more)
//       ├── AdwOverlaySplitView
//       │   ├── [sidebar] GtkListBox (slide thumbnails)
//       │   ├── [content] GtkDrawingArea (Cairo slide canvas)
//       │   └── AdwBreakpoint (600sp → collapsed sidebar)
//       └── [bottom] status / transition controls

use adw::prelude::*;
use gtk4::{self as gtk, glib, prelude::*};
use gtk4::cairo;
use libadwaita as adw;
use std::cell::Cell;
use suite_common::SuiteWindow;

// ── Slide data model ─────────────────────────────────────────────────────

/// Represents a single slide in the deck.
#[derive(Clone)]
pub struct SlideData {
    pub title: String,
    pub background: String,       // hex color e.g. "#ffffff"
    pub objects: Vec<SlideObjectData>,
}

#[derive(Clone)]
pub enum SlideObjectData {
    TextBox { text: String, x: f64, y: f64, w: f64, h: f64 },
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Circle { x: f64, y: f64, r: f64 },
}

impl SlideData {
    pub fn new(title: &str) -> Self {
        SlideData {
            title: title.to_string(),
            background: "#ffffff".into(),
            objects: vec![],
        }
    }
}

// ── DecksWindow ──────────────────────────────────────────────────────────

pub struct DecksWindow {
    pub window: adw::ApplicationWindow,
    split_view: adw::OverlaySplitView,
    slide_list: gtk::ListBox,
    canvas: gtk::DrawingArea,
    slides: Vec<SlideData>,
    current_slide: Cell<usize>,
}

impl DecksWindow {
    pub fn new(app: &adw::Application) -> Self {
        // ── Slide data ────────────────────────────────────────────────────
        let slides = vec![SlideData::new("Slide 1")];
        let current_slide = Cell::new(0usize);

        // ── Canvas (DrawingArea with Cairo) ────────────────────────────────
        let canvas = gtk::DrawingArea::new();
        canvas.set_vexpand(true);
        canvas.set_hexpand(true);
        canvas.set_content_width(960);
        canvas.set_content_height(540);
        canvas.set_draw_func({
            let slides = slides.clone();
            let current = current_slide.clone();
            move |_area, cr, width, height| {
                draw_slide(cr, width as f64, height as f64, &slides, current.get());
            }
        });

        let canvas_scroll = gtk::ScrolledWindow::new();
        canvas_scroll.set_child(Some(&canvas));
        canvas_scroll.set_vexpand(true);
        canvas_scroll.set_hexpand(true);
        canvas_scroll.set_min_content_width(400);
        canvas_scroll.set_min_content_height(300);

        // ── Content stack: empty state <-> editor ─────────────────────────
        let content_stack = gtk::Stack::new();
        content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        content_stack.set_transition_duration(200);

        let empty_page = suite_common::make_empty_state(
            "Decks",
            "Create a new presentation or open an existing one",
            "x-office-presentation-symbolic",
            "Open File\u{2026}",
        );
        content_stack.add_titled(&empty_page, Some("empty"), "Empty");
        content_stack.set_visible_child_name("empty");

        // ── Slide sidebar ─────────────────────────────────────────────────
        let slide_list = gtk::ListBox::new();
        slide_list.add_css_class("navigation-sidebar");
        slide_list.set_selection_mode(gtk::SelectionMode::Single);
        slide_list.set_activate_on_single_click(true);

        // Populate sidebar
        for (i, _slide) in slides.iter().enumerate() {
            let label = gtk::Label::new(Some(&format!("Slide {}", i + 1)));
            label.set_halign(gtk::Align::Start);
            label.set_margin_start(12);
            label.set_margin_end(12);
            label.set_margin_top(8);
            label.set_margin_bottom(8);
            slide_list.append(&label);
        }
        // Select first
        if let Some(row) = slide_list.row_at_index(0) {
            slide_list.select_row(Some(&row));
        }

        // Sidebar controls: Add / Delete / Move Up / Move Down
        let sidebar_controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        sidebar_controls.set_margin_start(6);
        sidebar_controls.set_margin_end(6);
        sidebar_controls.set_margin_top(6);
        sidebar_controls.set_margin_bottom(6);

        let add_btn = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add Slide")
            .build();
        let del_btn = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .tooltip_text("Delete Slide")
            .build();
        let up_btn = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Move Up")
            .build();
        let down_btn = gtk::Button::builder()
            .icon_name("go-down-symbolic")
            .tooltip_text("Move Down")
            .build();

        for btn in [&add_btn, &del_btn, &up_btn, &down_btn] {
            btn.add_css_class("flat");
            btn.set_has_frame(false);
        }

        sidebar_controls.append(&add_btn);
        sidebar_controls.append(&del_btn);
        sidebar_controls.append(&up_btn);
        sidebar_controls.append(&down_btn);

        let sidebar_scroll = gtk::ScrolledWindow::new();
        sidebar_scroll.set_child(Some(&slide_list));
        sidebar_scroll.set_vexpand(true);

        let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_box.append(&sidebar_scroll);
        sidebar_box.append(&sidebar_controls);
        sidebar_box.set_size_request(200, -1);

        // ── OverlaySplitView ──────────────────────────────────────────────
        let split_view = adw::OverlaySplitView::new();
        split_view.set_sidebar(Some(&sidebar_box));
        split_view.set_content(Some(&content_stack));
        split_view.set_max_sidebar_width(260.0);
        split_view.set_min_sidebar_width(180.0);

        // ── Breakpoint: collapse sidebar at 600sp ─────────────────────────
        let condition = adw::BreakpointCondition::parse("max-width: 600sp").unwrap();
        let bp = adw::Breakpoint::new(condition);
        let val = glib::Value::from(&true);
        bp.add_setter(&split_view, "collapsed", Some(&val));

        // ── SuiteWindow chrome ────────────────────────────────────────────
        let suite_win = SuiteWindow::new(app, "Decks", vec![], vec![]);
        suite_win.set_content(&split_view);

        // Rebuild the toolbar for decks-specific tools
        let toolbar = build_decks_toolbar();
        suite_win.add_top_bar(&toolbar);

        // ── Wire sidebar signals ──────────────────────────────────────────
        let canvas_clone = canvas.clone();
        let current_ref = current_slide.clone();
        let cs = content_stack.clone();
        slide_list.connect_row_selected(move |_list, row| {
            if let Some(r) = row {
                let idx = r.index() as usize;
                if idx != current_ref.get() {
                    current_ref.set(idx);
                    canvas_clone.queue_draw();
                }
            }
        });

        // Add slide
        let cs2 = content_stack.clone();
        add_btn.connect_clicked(move |_| {
            cs2.set_visible_child_name("editor");
        });

        // Delete slide
        del_btn.connect_clicked(|_| {});

        // Move up/down
        up_btn.connect_clicked(|_| {});
        down_btn.connect_clicked(|_| {});

        // ── Header bar: add Decks-specific buttons ────────────────────────
        // We add them to the existing header bar from SuiteWindow's make_header_bar()
        // Since SuiteWindow owns the header_bar, we need to access it.
        // For now, the header is handled by SuiteWindow's built-in menu.

        // ── Register app actions ──────────────────────────────────────────
        let cs = content_stack.clone();
        let act = gtk::gio::SimpleAction::new("new-document", None);
        act.connect_activate(move |_, _| {
            cs.set_visible_child_name("editor");
        });
        app.add_action(&act);

        let cs = content_stack.clone();
        let act = gtk::gio::SimpleAction::new("open-file", None);
        act.connect_activate(move |_, _| {
            cs.set_visible_child_name("editor");
        });
        app.add_action(&act);

        // Present mode action
        let act = gtk::gio::SimpleAction::new("present", None);
        act.connect_activate(|_, _| {
            // fullscreen the window
        });
        app.add_action(&act);

        // ── Add breakpoint to window ──────────────────────────────────────
        suite_win.window.add_breakpoint(bp);

        Self {
            window: suite_win.window,
            split_view,
            slide_list,
            canvas,
            slides,
            current_slide,
        }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

// ── Decks-specific toolbar ───────────────────────────────────────────────

fn build_decks_toolbar() -> gtk::Box {
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    toolbar.set_margin_start(6);
    toolbar.set_margin_end(6);
    toolbar.set_halign(gtk::Align::Center);

    // Drawing tools (primary)
    let bold = gtk::ToggleButton::builder()
        .icon_name("format-text-bold-symbolic")
        .tooltip_text("Bold")
        .build();
    let italic = gtk::ToggleButton::builder()
        .icon_name("format-text-italic-symbolic")
        .tooltip_text("Italic")
        .build();
    let underline = gtk::ToggleButton::builder()
        .icon_name("format-text-underline-symbolic")
        .tooltip_text("Underline")
        .build();
    for btn in [&bold, &italic, &underline] {
        btn.add_css_class("flat");
        toolbar.append(btn);
    }

    let sep = gtk::Separator::new(gtk::Orientation::Vertical);
    sep.set_margin_start(6);
    sep.set_margin_end(6);
    toolbar.append(&sep);

    // Object tools
    let text_box = gtk::Button::builder()
        .icon_name("insert-text-symbolic")
        .tooltip_text("Add Text Box")
        .build();
    text_box.add_css_class("flat");
    toolbar.append(&text_box);

    let shape = gtk::Button::builder()
        .icon_name("insert-object-symbolic")
        .tooltip_text("Add Shape")
        .build();
    shape.add_css_class("flat");
    toolbar.append(&shape);

    let image = gtk::Button::builder()
        .icon_name("insert-image-symbolic")
        .tooltip_text("Add Image")
        .build();
    image.add_css_class("flat");
    toolbar.append(&image);

    let sep2 = gtk::Separator::new(gtk::Orientation::Vertical);
    toolbar.append(&sep2);

    // Present
    let present = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic")
        .tooltip_text("Present (F5)")
        .build();
    present.add_css_class("flat");
    present.add_css_class("suggested-action");
    toolbar.append(&present);

    toolbar
}

// ── Cairo slide rendering ─────────────────────────────────────────────────

/// Draw the current slide onto the Cairo context.
fn draw_slide(
    cr: &cairo::Context,
    width: f64,
    height: f64,
    slides: &[SlideData],
    current_slide: usize,
) {
    let idx = current_slide;
    // Background
    cr.set_source_rgb(0.86, 0.86, 0.86); // #dcdcdc canvas area
    cr.paint().unwrap();

    // Slide area (16:9 centered)
    let slide_w = width * 0.85;
    let slide_h = slide_w * 9.0 / 16.0;
    let ox = (width - slide_w) / 2.0;
    let oy = (height - slide_h) / 2.0;

    // Slide shadow
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
    cr.rectangle(ox + 3.0, oy + 3.0, slide_w, slide_h);
    cr.fill().unwrap();

    // Slide background
    if idx < slides.len() {
        let bg = &slides[idx].background;
        if bg == "#ffffff" || bg.is_empty() {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        } else if bg.starts_with('#') && bg.len() >= 7 {
            let r = u8::from_str_radix(&bg[1..3], 16).unwrap_or(255) as f64 / 255.0;
            let g = u8::from_str_radix(&bg[3..5], 16).unwrap_or(255) as f64 / 255.0;
            let b = u8::from_str_radix(&bg[5..7], 16).unwrap_or(255) as f64 / 255.0;
            cr.set_source_rgb(r, g, b);
        } else {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        }
    } else {
        cr.set_source_rgb(1.0, 1.0, 1.0);
    }
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.fill().unwrap();

    // Slide border
    cr.set_source_rgb(0.7, 0.7, 0.7);
    cr.set_line_width(1.0);
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.stroke().unwrap();

    // Draw objects
    if idx < slides.len() {
        for obj in &slides[idx].objects {
            match obj {
                SlideObjectData::TextBox { text, x, y, w, h } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    cr.set_source_rgb(0.1, 0.1, 0.1);
                    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                    cr.set_font_size(16.0);
                    cr.move_to(sx, sy + 16.0);
                    cr.show_text(text).unwrap();
                }
                SlideObjectData::Rect { x, y, w, h } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    cr.set_source_rgb(0.3, 0.5, 0.9);
                    cr.rectangle(sx, sy, sw, sh);
                    cr.fill().unwrap();
                }
                SlideObjectData::Circle { x, y, r } => {
                    let cx = ox + (x / 960.0) * slide_w;
                    let cy = oy + (y / 540.0) * slide_h;
                    let radius = (r / 540.0) * slide_h;
                    cr.set_source_rgb(0.9, 0.3, 0.2);
                    cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
                    cr.fill().unwrap();
                }
            }
        }
    }

    // Empty slide indicator
    if idx < slides.len() && slides[idx].objects.is_empty() {
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.5);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(14.0);
        let text = format!("Slide {}", idx + 1);
        let extents = cr.text_extents(&text).unwrap();
        cr.move_to(ox + (slide_w - extents.width()) / 2.0, oy + slide_h - 20.0);
        cr.show_text(&text).unwrap();
    }

    // Slide number badge (top-right)
    if idx < slides.len() {
        let badge = format!("{}", idx + 1);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.4);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(11.0);
        cr.move_to(ox + slide_w - 30.0, oy + 20.0);
        cr.show_text(&badge).unwrap();
    }
}
