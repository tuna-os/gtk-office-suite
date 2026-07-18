// SPDX-License-Identifier: GPL-3.0-or-later
//
// DecksWindow — Presentation window with slide sidebar + Cairo canvas.
// MVP: shapes, text boxes, images, present mode, fullscreen nav.

use adw::prelude::*;
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4::cairo;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use suite_common::undo::UndoManager;
use suite_common::SuiteWindow;
use decks_core::undo::{AddObjectCmd, DeleteObjectCmd, AddSlideCmd, DeleteSlideCmd, ReorderSlidesCmd, MoveObjectCmd, set_obj_position};
use crate::canvas::{draw_slide, canvas_to_slide, slide_to_canvas, hit_test_object, snap_to_grid, GRID_SPACING};
use crate::sidebar::rebuild_slide_list;
use crate::toolbar::{find_toolbar_child, build_decks_toolbar};
use crate::transition::{TransitionState, TransitionType, draw_transition};

use decks_core::engine::{Slide, SlideObject, MasterSlide, Deck, read_pptx, write_pptx};

// ── DecksWindow ──────────────────────────────────────────────────────────

pub struct DecksWindow {
    pub window: adw::ApplicationWindow,
    split_view: adw::OverlaySplitView,
    slide_list: gtk::ListBox,
    canvas: gtk::DrawingArea,
    slides: Rc<RefCell<Vec<Slide>>>,
    masters: Rc<RefCell<Vec<MasterSlide>>>,
    current_slide: Rc<Cell<usize>>,
    selected_object: Rc<Cell<Option<usize>>>,
    transition: Rc<RefCell<TransitionState>>,
    undo: Rc<RefCell<UndoManager<Vec<Slide>>>>,
    content_stack: gtk::Stack,
    editor_split: adw::OverlaySplitView,
    file_path: Rc<RefCell<Option<String>>>,
    refresh_hud: Rc<dyn Fn()>,
}

impl DecksWindow {
    pub fn new(app: &adw::Application) -> Self {
        let slides = Rc::new(RefCell::new(vec![Slide {
            title: "Slide 1".into(),
            background: "#ffffff".into(),
            objects: vec![],
            notes: String::new(),
            master_idx: Some(0),
        }]));
        let masters = Rc::new(RefCell::new(vec![MasterSlide {
            name: "Default".into(),
            background: "#ffffff".into(),
            default_font: "Sans".into(),
            shapes: vec![],
        }]));
        let current_slide = Rc::new(Cell::new(0usize));
        let selected_object = Rc::new(Cell::new(None));
        let file_path = Rc::new(RefCell::new(None::<String>));
        let undo = Rc::new(RefCell::new(UndoManager::new(slides.clone())));
        let transition = Rc::new(RefCell::new(TransitionState::new()));

        // ── Canvas ────────────────────────────────────────────────────────
        // CanvasArea exposes each slide object as a virtual AT-SPI child
        // (canvas_area.rs, issue #87); it IS a DrawingArea otherwise.
        let canvas_area = crate::canvas_area::CanvasArea::default();
        let canvas = canvas_area.clone().upcast::<gtk::DrawingArea>();
        canvas.set_vexpand(true);
        canvas.set_hexpand(true);
        canvas.set_accessible_role(gtk::AccessibleRole::List);
        canvas.update_property(&[gtk::accessible::Property::Label("Slide canvas")]);
        // No fixed content size: the canvas fills the viewport and the
        // slide scales to fit (slide_geometry) — a fixed 960px minimum
        // made the scrolled window clip the slide at narrow widths.
        {
            let s = slides.clone();
            let c = current_slide.clone();
            let so = selected_object.clone();
            let ts = transition.clone();
            let m = masters.clone();
            canvas.set_draw_func(move |area, cr, width, height| {
                let t = ts.borrow();
                if draw_transition(cr, &t, width as f64, height as f64) {
                    return; // transition is active, skip normal rendering
                }
                drop(t);
                let slides = s.borrow();
                let cur = c.get();
                // A11y: every state change redraws, so the accessible
                // description tracks slide/selection here (issue #87).
                let desc = {
                    let n_objs = slides.get(cur).map(|sl| sl.objects.len()).unwrap_or(0);
                    match so.get() {
                        Some(oi) => format!(
                            "slide {} of {}, {} objects, object {} selected",
                            cur + 1, slides.len(), n_objs, oi + 1
                        ),
                        None => format!(
                            "slide {} of {}, {} objects",
                            cur + 1, slides.len(), n_objs
                        ),
                    }
                };
                area.update_property(&[gtk::accessible::Property::Description(&desc)]);
                draw_slide(cr, width as f64, height as f64, &slides, cur, so.get(), &m.borrow());
            });
        }

        let canvas_scroll = gtk::ScrolledWindow::new();
        canvas_scroll.set_child(Some(&canvas));
        canvas_scroll.set_vexpand(true);
        canvas_scroll.set_hexpand(true);
        canvas_scroll.set_min_content_width(400);
        canvas_scroll.set_min_content_height(300);

        // ── Object inspector (right sidebar) ─────────────────────────────
        // Visible twin of the a11y descriptions: position/size of the
        // selected object, two-way bound to the model (DESIGN-UI §Decks).
        let insp_grid = gtk::Grid::new();
        insp_grid.set_row_spacing(6);
        insp_grid.set_column_spacing(6);
        let mk_spin = |label: &str, row: i32, grid: &gtk::Grid| -> gtk::SpinButton {
            let l = gtk::Label::new(Some(label));
            l.add_css_class("dim-label");
            l.set_halign(gtk::Align::Start);
            let sb = gtk::SpinButton::with_range(-2000.0, 4000.0, 1.0);
            sb.set_hexpand(true);
            sb.update_property(&[gtk::accessible::Property::Label(&format!("Object {label}"))]);
            grid.attach(&l, 0, row, 1, 1);
            grid.attach(&sb, 1, row, 1, 1);
            sb
        };
        let spin_x = mk_spin("X", 0, &insp_grid);
        let spin_y = mk_spin("Y", 1, &insp_grid);
        let spin_w = mk_spin("W", 2, &insp_grid);
        let spin_h = mk_spin("H", 3, &insp_grid);

        let insp_title = gtk::Label::new(Some("Object"));
        insp_title.add_css_class("heading");
        insp_title.set_halign(gtk::Align::Start);
        let insp_hint = gtk::Label::new(Some("Select an object on the slide"));
        insp_hint.add_css_class("dim-label");
        insp_hint.add_css_class("caption");
        insp_hint.set_halign(gtk::Align::Start);
        insp_hint.set_wrap(true);

        let inspector = gtk::Box::new(gtk::Orientation::Vertical, 12);
        inspector.set_margin_start(12);
        inspector.set_margin_end(12);
        inspector.set_margin_top(12);
        inspector.set_margin_bottom(12);
        inspector.append(&insp_title);
        inspector.append(&insp_grid);
        inspector.append(&insp_hint);
        insp_grid.set_sensitive(false);

        // Status readout: slide x/y + object count (same source as the
        // a11y description).
        let status_label = gtk::Label::new(None);
        status_label.add_css_class("caption");
        status_label.add_css_class("dim-label");

        // Presenter pill: bottom-center prev / present / next.
        let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        pill.add_css_class("linked");
        pill.add_css_class("osd");
        pill.add_css_class("toolbar");
        pill.set_halign(gtk::Align::Center);
        pill.set_valign(gtk::Align::End);
        pill.set_margin_bottom(12);
        let prev_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        prev_btn.set_tooltip_text(Some("Previous slide"));
        let present_btn = gtk::Button::from_icon_name("media-playback-start-symbolic");
        present_btn.set_tooltip_text(Some("Present (F5)"));
        present_btn.set_action_name(Some("app.present"));
        let next_btn = gtk::Button::from_icon_name("go-next-symbolic");
        next_btn.set_tooltip_text(Some("Next slide"));
        pill.append(&prev_btn);
        pill.append(&present_btn);
        pill.append(&next_btn);

        let canvas_overlay = gtk::Overlay::new();
        canvas_overlay.set_child(Some(&canvas_scroll));
        canvas_overlay.add_overlay(&pill);
        status_label.set_halign(gtk::Align::End);
        status_label.set_valign(gtk::Align::End);
        status_label.set_margin_end(12);
        status_label.set_margin_bottom(12);
        canvas_overlay.add_overlay(&status_label);

        let editor_split = adw::OverlaySplitView::new();
        editor_split.set_sidebar_position(gtk::PackType::End);
        editor_split.set_sidebar(Some(&inspector));
        editor_split.set_content(Some(&canvas_overlay));
        editor_split.set_min_sidebar_width(170.0);
        editor_split.set_max_sidebar_width(220.0);

        // Central HUD refresh: status text + inspector fields.
        // The thumbnail updater is late-bound (the slide list is built
        // after this closure).
        let thumb_updater: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        let insp_guard = Rc::new(Cell::new(false));
        let refresh_hud: Rc<dyn Fn()> = {
            let ss = slides.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let (sx, sy, sw, sh) = (spin_x.clone(), spin_y.clone(), spin_w.clone(), spin_h.clone());
            let grid = insp_grid.clone();
            let status = status_label.clone();
            let guard = insp_guard.clone();
            let ca = canvas_area.clone();
            let tu = thumb_updater.clone();
            Rc::new(move || {
                let idx = cs_ref.get();
                let slides = ss.borrow();
                if let Some(slide) = slides.get(idx) {
                    ca.sync_objects(&slide.objects, so.get());
                }
                drop(slides);
                if let Some(update) = tu.borrow().as_ref() {
                    update();
                }
                let slides = ss.borrow();
                let n_objects = slides.get(idx).map(|s| s.objects.len()).unwrap_or(0);
                status.set_text(&format!(
                    "Slide {}/{}  ·  {} object{}",
                    idx + 1,
                    slides.len().max(1),
                    n_objects,
                    if n_objects == 1 { "" } else { "s" }
                ));
                let obj = so
                    .get()
                    .and_then(|oi| slides.get(idx).and_then(|s| s.objects.get(oi)));
                match obj {
                    Some(o) => {
                        guard.set(true);
                        let (x, y, w, h) = match o {
                            SlideObject::TextBox { x, y, w, h, .. }
                            | SlideObject::Rect { x, y, w, h }
                            | SlideObject::Image { x, y, w, h, .. } => (*x, *y, *w, *h),
                            SlideObject::Circle { x, y, r } => (*x, *y, r * 2.0, r * 2.0),
                        };
                        sx.set_value(x);
                        sy.set_value(y);
                        sw.set_value(w);
                        sh.set_value(h);
                        grid.set_sensitive(true);
                        guard.set(false);
                    }
                    None => {
                        guard.set(true);
                        for sb in [&sx, &sy, &sw, &sh] {
                            sb.set_value(0.0);
                        }
                        guard.set(false);
                        grid.set_sensitive(false);
                    }
                }
            })
        };
        refresh_hud();

        // Inspector edits write back to the model.
        {
            enum Field { X, Y, W, H }
            for (spin, field) in [
                (&spin_x, Field::X),
                (&spin_y, Field::Y),
                (&spin_w, Field::W),
                (&spin_h, Field::H),
            ] {
                let ss = slides.clone();
                let cs_ref = current_slide.clone();
                let so = selected_object.clone();
                let da = canvas.clone();
                let guard = insp_guard.clone();
                spin.connect_value_changed(move |sb| {
                    if guard.get() {
                        return;
                    }
                    let Some(oi) = so.get() else { return };
                    let idx = cs_ref.get();
                    let mut slides = ss.borrow_mut();
                    let Some(obj) = slides.get_mut(idx).and_then(|s| s.objects.get_mut(oi))
                    else {
                        return;
                    };
                    let v = sb.value();
                    match obj {
                        SlideObject::TextBox { x, y, w, h, .. }
                        | SlideObject::Rect { x, y, w, h }
                        | SlideObject::Image { x, y, w, h, .. } => match field {
                            Field::X => *x = v,
                            Field::Y => *y = v,
                            Field::W => *w = v.max(1.0),
                            Field::H => *h = v.max(1.0),
                        },
                        SlideObject::Circle { x, y, r } => match field {
                            Field::X => *x = v,
                            Field::Y => *y = v,
                            Field::W | Field::H => *r = (v / 2.0).max(1.0),
                        },
                    }
                    drop(slides);
                    da.queue_draw();
                });
            }
        }


        // ── Content stack ─────────────────────────────────────────────────
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

        // We'll add the canvas to the stack when the user creates/opens a deck
        // For now, it starts with just the empty state

        // ── Slide sidebar ─────────────────────────────────────────────────
        let slide_list = gtk::ListBox::new();
        slide_list.add_css_class("navigation-sidebar");
        slide_list.set_selection_mode(gtk::SelectionMode::Single);
        slide_list.set_activate_on_single_click(false); // we handle selection manually

        // Populate initial slide list
        rebuild_slide_list(&slide_list, &slides.borrow(), &masters.borrow(), 0);

        let sidebar_controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        sidebar_controls.set_margin_start(6);
        sidebar_controls.set_margin_end(6);
        sidebar_controls.set_margin_top(6);
        sidebar_controls.set_margin_bottom(6);

        let add_btn = gtk::Button::builder()
            .icon_name("list-add-symbolic").tooltip_text("Add slide").build();
        let del_btn = gtk::Button::builder()
            .icon_name("list-remove-symbolic").tooltip_text("Delete slide").build();
        let up_btn = gtk::Button::builder()
            .icon_name("go-up-symbolic").tooltip_text("Move up").build();
        let down_btn = gtk::Button::builder()
            .icon_name("go-down-symbolic").tooltip_text("Move down").build();

        for btn in [&add_btn, &del_btn, &up_btn, &down_btn] {
            btn.add_css_class("flat");
            btn.set_has_frame(true);
            btn.set_size_request(36, 36);
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

        // ── Breakpoint ────────────────────────────────────────────────────
        let condition = adw::BreakpointCondition::parse("max-width: 600sp").unwrap();
        let bp = adw::Breakpoint::new(condition);
        let val = glib::Value::from(&true);
        bp.add_setter(&split_view, "collapsed", Some(&val));

        // ── SuiteWindow chrome ────────────────────────────────────────────
        let suite_win = SuiteWindow::new(app, "Decks", vec![], vec![]);

        // Speaker notes pane (collapsible, below the canvas)
        let notes_expander = gtk::Expander::new(Some("Speaker Notes"));
        let notes_buffer = gtk::TextBuffer::new(None);
        let notes_view = gtk::TextView::with_buffer(&notes_buffer);
        notes_view.set_wrap_mode(gtk::WrapMode::Word);
        notes_view.set_size_request(-1, 80);
        notes_view.set_vexpand(false);
        notes_expander.set_child(Some(&notes_view));

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_box.append(&split_view);
        main_box.append(&notes_expander);
        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&main_box));
        suite_win.set_content(&toast_overlay);

        let toolbar = build_decks_toolbar();
        suite_win.add_top_bar(&toolbar);

        // ── Wire sidebar signals ──────────────────────────────────────────
        let sl = slide_list.clone();
        let cs = canvas.clone();
        let cs_ref = current_slide.clone();
        let ss = slides.clone();
        let notes_skip = Rc::new(Cell::new(false));
        let notes_skip2 = notes_skip.clone();
        let nb = notes_buffer.clone();
        slide_list.connect_row_selected(move |list, row| {
            if let Some(r) = row {
                let idx = r.index() as usize;
                if idx < ss.borrow().len() {
                    cs_ref.set(idx);
                    cs.queue_draw();
                    let slides = ss.borrow();
                    if let Some(slide) = slides.get(idx) {
                        notes_skip2.set(true);
                        nb.set_text(&slide.notes);
                    }
                }
            }
        });

        // Late-bind the thumbnail updater now that the list exists.
        {
            let sl = slide_list.clone();
            let ss = slides.clone();
            let m = masters.clone();
            let cs_ref = current_slide.clone();
            *thumb_updater.borrow_mut() = Some(Box::new(move || {
                crate::sidebar::update_thumbnail(&sl, &ss.borrow(), &m.borrow(), cs_ref.get());
            }));
        }

        // Second row-selected handler: HUD follows slide switches.
        {
            let refresh = refresh_hud.clone();
            let so = selected_object.clone();
            slide_list.connect_row_selected(move |_, _| {
                so.set(None);
                refresh();
            });
        }

        // Pill prev/next drive the slide-list selection (the single
        // source of truth for the current slide).
        {
            let sl = slide_list.clone();
            prev_btn.connect_clicked(move |_| {
                let idx = sl.selected_row().map(|r| r.index()).unwrap_or(0);
                if idx > 0 {
                    if let Some(row) = sl.row_at_index(idx - 1) {
                        sl.select_row(Some(&row));
                    }
                }
            });
            let sl = slide_list.clone();
            next_btn.connect_clicked(move |_| {
                let idx = sl.selected_row().map(|r| r.index()).unwrap_or(0);
                if let Some(row) = sl.row_at_index(idx + 1) {
                    sl.select_row(Some(&row));
                }
            });
        }

        // Save speaker notes on text change
        {
            let ss = slides.clone();
            let cs_ref = current_slide.clone();
            let skip = notes_skip.clone();
            notes_buffer.connect_changed(move |buf| {
                if skip.get() { skip.set(false); return; }
                let idx = cs_ref.get();
                let mut slides = ss.borrow_mut();
                if let Some(slide) = slides.get_mut(idx) {
                    slide.notes = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                }
            });
        }

        // Add slide
        {
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let cs_stack = content_stack.clone();
            let undo = undo.clone();
            let masters = masters.clone();
            add_btn.connect_clicked(move |_| {
                let idx = ss.borrow().len();
                let new_slide = Slide {
                    title: format!("Slide {}", idx + 1),
                    background: "#ffffff".into(),
                    objects: vec![],
                    notes: String::new(),
            master_idx: Some(0),
                };
                undo.borrow_mut().execute(Box::new(AddSlideCmd {
                    index: idx,
                    slide: new_slide.clone(),
                }));
                rebuild_slide_list(&sl, &ss.borrow(), &masters.borrow(), idx);
                cs_ref.set(idx);
                cs.queue_draw();
                cs_stack.set_visible_child_name("editor");
            });
        }

        // Delete slide
        {
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let undo = undo.clone();
            let masters = masters.clone();
            del_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                let has_slides = {
                    let slides = ss.borrow();
                    slides.len() > 1 && idx < slides.len()
                };
                if has_slides {
                    let removed = {
                        let slides = ss.borrow();
                        slides[idx].clone()
                    };
                    let new_idx = {
                        let slides = ss.borrow();
                        idx.min(slides.len().saturating_sub(2))
                    };
                    undo.borrow_mut().execute(Box::new(DeleteSlideCmd {
                        index: idx,
                        slide: removed,
                    }));
                    cs_ref.set(new_idx);
                    rebuild_slide_list(&sl, &ss.borrow(), &masters.borrow(), new_idx);
                    cs.queue_draw();
                }
            });
        }

        // Move up/down
        {
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let undo = undo.clone();
            let masters = masters.clone();
            up_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                if idx > 0 {
                    undo.borrow_mut().execute(Box::new(ReorderSlidesCmd {
                        from: idx, to: idx - 1,
                    }));
                    cs_ref.set(idx - 1);
                    rebuild_slide_list(&sl, &ss.borrow(), &masters.borrow(), idx - 1);
                    cs.queue_draw();
                }
            });
        }
        {
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let undo = undo.clone();
            let masters = masters.clone();
            down_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                let slides = ss.borrow();
                if idx + 1 < slides.len() {
                    drop(slides);
                    undo.borrow_mut().execute(Box::new(ReorderSlidesCmd {
                        from: idx, to: idx + 1,
                    }));
                    cs_ref.set(idx + 1);
                    rebuild_slide_list(&sl, &ss.borrow(), &masters.borrow(), idx + 1);
                    cs.queue_draw();
                }
            });
        }

        // ── Toolbar actions ───────────────────────────────────────────────
        // Each operation is a named GioAction; toolbar buttons bind to the
        // action so the palette/shortcuts dialog see them too.
        suite_common::actions::register_labels(&[
            ("app.add-text-box", "Add Text Box"),
            ("app.add-shape", "Add Shape"),
            ("app.add-image", "Add Image…"),
            ("app.present", "Present"),
        ]);

        // "Add Text Box"
        {
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let undo = undo.clone();
            let refresh = refresh_hud.clone();
            let act = gio::SimpleAction::new("add-text-box", None);
            act.connect_activate(move |_, _| {
                let idx = cs_ref.get();
                let obj = SlideObject::TextBox {
                    text: "Text".into(), x: 200.0, y: 150.0, w: 200.0, h: 40.0,
                    runs: vec![],
                };
                undo.borrow_mut().execute(Box::new(AddObjectCmd::new(idx, obj)));
                cs.queue_draw();
                refresh();
            });
            app.add_action(&act);
            if let Some(btn) = find_toolbar_child(&toolbar, "insert-text-symbolic") {
                btn.set_action_name(Some("app.add-text-box"));
            }
        }

        // "Add Shape" — cycles through Rect → Circle
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let shape_count = Rc::new(Cell::new(0u32));
            let undo = undo.clone();
            let refresh = refresh_hud.clone();
            let act = gio::SimpleAction::new("add-shape", None);
            act.connect_activate(move |_, _| {
                let idx = cs_ref.get();
                let ss_snap = ss.borrow();
                if idx >= ss_snap.len() { return; }
                let count = shape_count.get();
                shape_count.set(count + 1);
                let obj = if count % 2 == 0 {
                    SlideObject::Rect { x: 200.0, y: 200.0, w: 200.0, h: 150.0 }
                } else {
                    SlideObject::Circle { x: 300.0, y: 250.0, r: 80.0 }
                };
                drop(ss_snap);
                undo.borrow_mut().execute(Box::new(AddObjectCmd::new(idx, obj)));
                cs.queue_draw();
                refresh();
            });
            app.add_action(&act);
            if let Some(btn) = find_toolbar_child(&toolbar, "insert-object-symbolic") {
                btn.set_action_name(Some("app.add-shape"));
            }
        }

        // "Add Image"
        {
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let w = suite_win.window.clone();
            let undo = undo.clone();
            let refresh = refresh_hud.clone();
            let act = gio::SimpleAction::new("add-image", None);
            act.connect_activate(move |_, _| {
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_mime_type("image/*");
                f.set_name(Some("Images"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                let cs = cs.clone();
                let cs_ref = cs_ref.clone();
                let undo = undo.clone();
                let refresh = refresh.clone();
                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let idx = cs_ref.get();
                                let p = path.to_string_lossy().to_string();
                                let obj = SlideObject::Image {
                                    path: p, x: 200.0, y: 200.0, w: 200.0, h: 150.0,
                                };
                                undo.borrow_mut().execute(Box::new(AddObjectCmd::new(idx, obj)));
                                cs.queue_draw();
                            }
                        }
                    },
                );
            });
            app.add_action(&act);
            if let Some(btn) = find_toolbar_child(&toolbar, "insert-image-symbolic") {
                btn.set_action_name(Some("app.add-image"));
            }
        }

        // Present
        {
            let w = suite_win.window.clone();
            let act = gio::SimpleAction::new("present", None);
            act.connect_activate(move |_, _| { w.fullscreen(); });
            app.add_action(&act);
            app.set_accels_for_action("app.present", &["F5"]);
            if let Some(btn) = find_toolbar_child(&toolbar, "view-fullscreen-symbolic") {
                btn.set_action_name(Some("app.present"));
            }
        }

        // ── Cross-app clipboard (DESIGN-UI): Ctrl+C copies the selected
        // text box as a styled fragment; Ctrl+V pastes a fragment as a
        // new text box. Window-level capture, skipped while an entry or
        // the notes view has focus.
        {
            let ss = slides.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let cs = canvas.clone();
            let undo2 = undo.clone();
            let refresh = refresh_hud.clone();
            let win = suite_win.window.clone();
            let key = gtk::EventControllerKey::new();
            key.set_propagation_phase(gtk::PropagationPhase::Capture);
            key.connect_key_pressed(move |_, keyval, _code, mods| {
                let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                if !ctrl || (keyval != gtk::gdk::Key::c && keyval != gtk::gdk::Key::v) {
                    return glib::Propagation::Proceed;
                }
                // Text widgets keep their own clipboard behavior.
                if gtk::prelude::GtkWindowExt::focus(&win)
                    .map(|w| w.is::<gtk::Text>() || w.is::<gtk::TextView>() || w.is::<gtk::Entry>())
                    .unwrap_or(false)
                {
                    return glib::Propagation::Proceed;
                }
                if keyval == gtk::gdk::Key::c {
                    let idx = cs_ref.get();
                    let slides = ss.borrow();
                    let frag = so
                        .get()
                        .and_then(|oi| slides.get(idx).and_then(|s| s.objects.get(oi)))
                        .and_then(decks_core::fragment::copy_object);
                    if let Some(frag) = frag {
                        let provider = suite_common::clipboard::provider(
                            decks_core::fragment::MIME,
                            &frag.to_json(),
                            &frag.to_html(),
                            &frag.to_plain(),
                        );
                        let _ = cs.clipboard().set_content(Some(&provider));
                        return glib::Propagation::Stop;
                    }
                    return glib::Propagation::Proceed;
                }
                // Paste
                let clipboard = cs.clipboard();
                if !suite_common::clipboard::offers(&clipboard, decks_core::fragment::MIME) {
                    return glib::Propagation::Proceed;
                }
                let cs2 = cs.clone();
                let cs_ref2 = cs_ref.clone();
                let undo3 = undo2.clone();
                let refresh2 = refresh.clone();
                suite_common::clipboard::read_string(
                    &clipboard,
                    decks_core::fragment::MIME,
                    move |json| {
                        if let Some(frag) = json
                            .as_deref()
                            .and_then(decks_core::fragment::Fragment::from_json)
                        {
                            let obj = decks_core::fragment::paste_as_text_box(&frag, 240.0, 200.0);
                            let idx = cs_ref2.get();
                            undo3.borrow_mut().execute(Box::new(AddObjectCmd::new(idx, obj)));
                            cs2.queue_draw();
                            refresh2();
                        }
                    },
                );
                glib::Propagation::Stop
            });
            suite_win.window.add_controller(key);
        }

        // ── Mouse interaction on canvas ──────────────────────────────────
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let refresh = refresh_hud.clone();
            let click = gtk::GestureClick::new();
            click.connect_pressed(move |_g, _n, x, y| {
                let idx = cs_ref.get();
                let slides = ss.borrow();
                if idx >= slides.len() { return; }
                let (sx, sy) = canvas_to_slide(x, y, cs.width() as f64, cs.height() as f64);
                let found = hit_test_object(&slides[idx].objects, sx, sy);
                so.set(found);
                drop(slides);
                cs.queue_draw();
                refresh();
            });
            canvas.add_controller(click);
        }

        // ── Object drag: move selected object on canvas ────────────────
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let undo = undo.clone();
            let drag_state: Rc<Cell<Option<(usize, f64, f64)>>> = Rc::new(Cell::new(None));
            let drag = gtk::GestureDrag::new();
            drag.set_button(1);
            let ds2 = drag_state.clone();
            let ds3 = drag_state.clone();
            let ds4 = drag_state.clone();
            let ss2 = ss.clone();
            let cs2 = cs.clone();
            let cs_ref2 = cs_ref.clone();
            let cs_ref3 = cs_ref.clone();
            let cs_ref4 = cs_ref.clone();
            let cs3 = cs.clone();
            let so2 = so.clone();
            let cvb = cs.clone();
            let cve = cs.clone();
            drag.connect_drag_begin(move |_g, x, y| {
                let idx = cs_ref2.get();
                let sl = ss2.borrow();
                if idx >= sl.len() { return; }
                let (sx, sy) =
                    canvas_to_slide(x, y, cvb.width() as f64, cvb.height() as f64);
                if let Some(oi) = hit_test_object(&sl[idx].objects, sx, sy) {
                    let (ox, oy) = decks_core::undo::obj_position(&sl[idx].objects[oi]);
                    so2.set(Some(oi));
                    ds2.set(Some((oi, ox, oy)));
                }
            });
            drag.connect_drag_update(move |_g, dx, dy| {
                if let Some((oi, orig_x, orig_y)) = ds3.get() {
                    let idx = cs_ref3.get();
                    let mut sl = ss.borrow_mut();
                    if idx < sl.len() && oi < sl[idx].objects.len() {
                        let (_, _, sw, _) = crate::canvas::slide_geometry(
                            cs3.width() as f64, cs3.height() as f64);
                        let k = 960.0 / sw.max(1.0);
                        let nx = snap_to_grid(orig_x + dx * k, GRID_SPACING);
                        let ny = snap_to_grid(orig_y + dy * k, GRID_SPACING);
                        set_obj_position(&mut sl[idx].objects[oi], nx, ny);
                        cs3.queue_draw();
                    }
                }
            });
            drag.connect_drag_end(move |_g, dx, dy| {
                if let Some((oi, orig_x, orig_y)) = ds4.get() {
                    let (_, _, sw, _) = crate::canvas::slide_geometry(
                        cve.width() as f64, cve.height() as f64);
                    let k = 960.0 / sw.max(1.0);
                    let snapped_x = snap_to_grid(orig_x + dx * k, GRID_SPACING);
                    let snapped_y = snap_to_grid(orig_y + dy * k, GRID_SPACING);
                    let net_dx = snapped_x - orig_x;
                    let net_dy = snapped_y - orig_y;
                    if net_dx != 0.0 || net_dy != 0.0 {
                        undo.borrow_mut().execute(Box::new(
                            MoveObjectCmd {
                                slide_idx: cs_ref4.get(), index: oi,
                                dx: net_dx, dy: net_dy,
                            }
                        ));
                    }
                }
                ds4.set(None);
            });
            canvas.add_controller(drag);
        }

        // ── Double-click: inline text edit on TextBox ───────────────────
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let undo = undo.clone();
            let dbl = gtk::GestureClick::new();
            dbl.set_button(1);
            let cs2 = cs.clone();
            dbl.connect_pressed(move |_g, n, x, y| {
                if n < 2 { return; }
                let idx = cs_ref.get();
                let slides = ss.borrow();
                if idx >= slides.len() { return; }
                let (hx, hy) =
                    canvas_to_slide(x, y, cs2.width() as f64, cs2.height() as f64);
                if let Some(oi) = hit_test_object(&slides[idx].objects, hx, hy) {
                    let obj = slides[idx].objects[oi].clone();
                    if let SlideObject::TextBox { text, x: ox, y: oy, w: ow, h: oh, .. } = obj {
                        let old_text = text.clone();
                        drop(slides);
                        let text_view = gtk::TextView::new();
                        text_view.buffer().set_text(&old_text);
                        text_view.set_wrap_mode(gtk::WrapMode::Word);
                        text_view.set_size_request((ow / 960.0 * 800.0) as i32, (oh / 540.0 * 450.0) as i32);
                        let overlay = gtk::Fixed::new();
                        let (cvx, cvy) = crate::canvas::slide_to_canvas(ox, oy, 800.0, 450.0);
                        overlay.put(&text_view, cvx, cvy);
                        // Add overlay to window via a stack or popover — put on Fixed overlay
                        // For now, add as child of the canvas parent scrolled window area
                        cs2.parent().map(|p| {
                            if let Ok(fixed) = p.downcast::<gtk::Fixed>() {
                                fixed.put(&overlay, 0.0, 0.0);
                            }
                        });
                        text_view.grab_focus();
                        // Commit on Enter via EventControllerKey
                        let key_ctrl = gtk::EventControllerKey::new();
                        let ss2 = ss.clone();
                        let cs3 = cs.clone();
                        let undo2 = undo.clone();
                        let tv2 = text_view.clone();
                        let ov2 = overlay.clone();
                        let cs_ref2 = cs_ref.clone();
                        key_ctrl.connect_key_pressed(move |_, key, _code, _mod| {
                            if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
                                let buf = tv2.buffer();
                                let new_text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                                if new_text != old_text {
                                    undo2.borrow_mut().execute(Box::new(
                                        decks_core::undo::ChangeTextCmd {
                                            slide_idx: cs_ref2.get(), index: oi,
                                            old_text: old_text.clone(), new_text,
                                        }
                                    ));
                                }
                                ov2.unparent();
                                cs3.queue_draw();
                                glib::Propagation::Stop
                            } else {
                                glib::Propagation::Proceed
                            }
                        });
                        text_view.add_controller(key_ctrl);
                        // Commit on focus loss
                        let fc = gtk::EventControllerFocus::new();
                        let tv3 = text_view.clone();
                        let ov3 = overlay.clone();
                        fc.connect_leave(move |_| {
                            tv3.parent().map(|p| { p.unparent(); });
                        });
                        text_view.add_controller(fc);
                    }
                }
            });
            canvas.add_controller(dbl);
        }

        // ── Keyboard: navigation, delete, undo/redo ─────────────────────
        {
            let w = suite_win.window.clone();
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let undo = undo.clone();
            let ts = transition.clone();
            let m = masters.clone();
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, code, mods| {
                // Ctrl+Z: undo
                if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) && keyval == gtk::gdk::Key::z {
                    let mut u = undo.borrow_mut();
                    if u.undo() {
                        cs.queue_draw();
                        let slides = ss.borrow();
                        rebuild_slide_list(&sl, &slides, &m.borrow(), cs_ref.get());
                    }
                    return glib::Propagation::Stop;
                }
                // Ctrl+Shift+Z: redo
                if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK) && keyval == gtk::gdk::Key::z {
                    let mut u = undo.borrow_mut();
                    if u.redo() {
                        cs.queue_draw();
                        let slides = ss.borrow();
                        rebuild_slide_list(&sl, &slides, &m.borrow(), cs_ref.get());
                    }
                    return glib::Propagation::Stop;
                }
                match keyval {
                    gtk::gdk::Key::Escape => {
                        w.unfullscreen();
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Left | gtk::gdk::Key::Up => {
                        let idx = cs_ref.get();
                        if idx > 0 {
                            let sls = ss.borrow();
                            if idx < sls.len() && idx > 0 {
                                ts.borrow_mut().start(TransitionType::PushLeft,
                                    &sls[idx], &sls[idx - 1], &cs);
                            }
                            cs_ref.set(idx - 1);
                            rebuild_slide_list(&sl, &sls, &m.borrow(), idx - 1);
                            cs.queue_draw();
                        }
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Right | gtk::gdk::Key::Down | gtk::gdk::Key::space => {
                        let idx = cs_ref.get();
                        let slides = ss.borrow();
                        if idx + 1 < slides.len() {
                            ts.borrow_mut().start(TransitionType::PushLeft,
                                &slides[idx], &slides[idx + 1], &cs);
                            cs_ref.set(idx + 1);
                            rebuild_slide_list(&sl, &slides, &m.borrow(), idx + 1);
                            cs.queue_draw();
                        }
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Home => {
                        cs_ref.set(0);
                        rebuild_slide_list(&sl, &ss.borrow(), &m.borrow(), 0);
                        cs.queue_draw();
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::End => {
                        let slides = ss.borrow();
                        if !slides.is_empty() {
                            cs_ref.set(slides.len() - 1);
                            rebuild_slide_list(&sl, &slides, &m.borrow(), slides.len() - 1);
                            cs.queue_draw();
                        }
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Delete | gtk::gdk::Key::BackSpace => {
                        let idx = cs_ref.get();
                        let slides = ss.borrow();
                        if idx < slides.len() {
                            if let Some(oi) = so.get() {
                                if oi < slides[idx].objects.len() {
                                    let obj = slides[idx].objects[oi].clone();
                                    drop(slides);
                                    undo.borrow_mut().execute(Box::new(
                                        DeleteObjectCmd::new(idx, oi, obj)
                                    ));
                                    so.set(None);
                                    cs.queue_draw();
                                }
                            }
                        }
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
            canvas.add_controller(key);
        }

        // ── App actions ──────────────────────────────────────────────────
        {
            let cs = content_stack.clone();
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs_scroll = editor_split.clone();
            let path_ref = file_path.clone();
            let refresh = refresh_hud.clone();
            let masters = masters.clone();
            let act = gtk::gio::SimpleAction::new("new-document", None);
            act.connect_activate(move |_, _| {
                if cs.child_by_name("editor").is_none() {
                    cs.add_titled(&cs_scroll, Some("editor"), "Editor");
                }
                cs.set_visible_child_name("editor");
                {
                    let mut slides = ss.borrow_mut();
                    *slides = vec![Slide {
                        title: "Slide 1".into(),
                        background: "#ffffff".into(),
                        objects: vec![],
                        notes: String::new(),
                        master_idx: Some(0),
                    }];
                }
                *path_ref.borrow_mut() = None;
                rebuild_slide_list(&sl, &ss.borrow(), &masters.borrow(), 0);
                cs.queue_draw();
                refresh();
            });
            app.add_action(&act);
        }

        {
            let cs = content_stack.clone();
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let da = canvas.clone();
            let w = suite_win.window.clone();
            let cs_scroll = editor_split.clone();
            let path_ref = file_path.clone();
            let masters = masters.clone();

            let act = gtk::gio::SimpleAction::new("open-file", None);
            act.connect_activate(move |_, _| {
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.pptx");
                f.set_name(Some("PowerPoint Presentations (.pptx)"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));

                let cs = cs.clone();
                let sl = sl.clone();
                let ss = ss.clone();
                let cs_ref = cs_ref.clone();
                let so = so.clone();
                let da = da.clone();
                let w2 = w.clone();
                let cs_scroll = cs_scroll.clone();
                let path_ref = path_ref.clone();
                let masters = masters.clone();

                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                match read_pptx(&path_str) {
                                    Ok(deck) => {
                                        *ss.borrow_mut() = deck.slides;
                                        cs_ref.set(0);
                                        so.set(None);
                                        *path_ref.borrow_mut() = Some(path_str);
                                        if cs.child_by_name("editor").is_none() {
                                            cs.add_titled(&cs_scroll, Some("editor"), "Editor");
                                        }
                                        cs.set_visible_child_name("editor");
                                        rebuild_slide_list(&sl, &ss.borrow(), &masters.borrow(), 0);
                                        if let Some(name) = path_ref
                                            .borrow()
                                            .as_deref()
                                            .and_then(|p| std::path::Path::new(p).file_name())
                                        {
                                            w2.set_title(Some(&format!(
                                                "{} — Decks", name.to_string_lossy())));
                                        }
                                        da.queue_draw();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading("Error opening presentation")
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", "OK");
                                        err.set_default_response(Some("ok"));
                                        err.present(Some(&w2));
                                    }
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&act);
        }

        // Save actions
        {
            let ss = slides.clone();
            let w = suite_win.window.clone();
            let path_ref = file_path.clone();

            let act_save = gtk::gio::SimpleAction::new("save-file", None);
            let ss_clone = ss.clone();
            let w_clone = w.clone();
            let path_clone = path_ref.clone();
            let m_save = masters.clone();
            act_save.connect_activate(move |_, _| {
                let current_path = path_clone.borrow().clone();
                if let Some(path_str) = current_path {
                    let deck = Deck { slides: ss_clone.borrow().clone(), masters: m_save.borrow().clone() };
                    if let Err(e) = write_pptx(&path_str, &deck) {
                        let err = adw::AlertDialog::builder()
                            .heading("Error saving presentation")
                            .body(&e)
                            .build();
                        err.add_response("ok", "OK");
                        err.set_default_response(Some("ok"));
                        err.present(Some(&w_clone));
                    }
                } else {
                    let _ = gtk4::prelude::WidgetExt::activate_action(&w_clone, "app.save-file-as", None);
                }
            });
            app.add_action(&act_save);

            let act_save_as = gtk::gio::SimpleAction::new("save-file-as", None);
            let m_as = masters.clone();
            act_save_as.connect_activate(move |_, _| {
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.pptx");
                f.set_name(Some("PowerPoint Presentations (.pptx)"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.set_initial_name(Some("Untitled.pptx"));

                let ss = ss.clone();
                let w2 = w.clone();
                let path_ref = path_ref.clone();
                let m_inner = m_as.clone();
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let deck = Deck { slides: ss.borrow().clone(), masters: m_inner.borrow().clone() };
                                match write_pptx(&path_str, &deck) {
                                    Ok(()) => {
                                        *path_ref.borrow_mut() = Some(path_str);
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading("Error saving presentation")
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", "OK");
                                        err.set_default_response(Some("ok"));
                                        err.present(Some(&w2));
                                    }
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&act_save_as);
        }

        // ── Add breakpoint to window ──────────────────────────────────────
        suite_win.window.add_breakpoint(bp);

        Self {
            window: suite_win.window,
            split_view,
            slide_list,
            canvas,
            slides,
            masters: masters.clone(),
            current_slide,
            selected_object,
            transition,
            undo,
            content_stack,
            editor_split,
            file_path,
            refresh_hud,
        }
    }

    pub fn present(&self) { self.window.present(); }

    /// Open a .pptx directly (CLI / file-manager open). Mirrors the
    /// open-file dialog success path.
    pub fn open_path(&self, path: &str) -> Result<(), String> {
        let deck = read_pptx(path)?;
        *self.slides.borrow_mut() = deck.slides;
        self.current_slide.set(0);
        self.selected_object.set(None);
        *self.file_path.borrow_mut() = Some(path.to_string());
        if self.content_stack.child_by_name("editor").is_none() {
            self.content_stack.add_titled(&self.editor_split, Some("editor"), "Editor");
        }
        self.content_stack.set_visible_child_name("editor");
        rebuild_slide_list(&self.slide_list, &self.slides.borrow(), &self.masters.borrow(), 0);
        if let Some(name) = std::path::Path::new(path).file_name() {
            self.window
                .set_title(Some(&format!("{} — Decks", name.to_string_lossy())));
        }
        self.canvas.queue_draw();
        (self.refresh_hud)();
        Ok(())
    }
}

// ── Helper: rebuild the slide list widget ────────────────────────────────
// force rebuild
