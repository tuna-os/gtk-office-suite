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
use crate::undo::{AddObjectCmd, DeleteObjectCmd, AddSlideCmd, DeleteSlideCmd, ReorderSlidesCmd};

use crate::engine::{Slide, SlideObject, Deck, read_pptx, write_pptx};

// ── DecksWindow ──────────────────────────────────────────────────────────

pub struct DecksWindow {
    pub window: adw::ApplicationWindow,
    split_view: adw::OverlaySplitView,
    slide_list: gtk::ListBox,
    canvas: gtk::DrawingArea,
    slides: Rc<RefCell<Vec<Slide>>>,
    current_slide: Rc<Cell<usize>>,
    selected_object: Rc<Cell<Option<usize>>>,
    undo: Rc<RefCell<UndoManager<Vec<Slide>>>>,
}

impl DecksWindow {
    pub fn new(app: &adw::Application) -> Self {
        let slides = Rc::new(RefCell::new(vec![Slide {
            title: "Slide 1".into(),
            background: "#ffffff".into(),
            objects: vec![],
        }]));
        let current_slide = Rc::new(Cell::new(0usize));
        let selected_object = Rc::new(Cell::new(None));
        let file_path = Rc::new(RefCell::new(None::<String>));
        let undo = Rc::new(RefCell::new(UndoManager::new(slides.clone())));

        // ── Canvas ────────────────────────────────────────────────────────
        let canvas = gtk::DrawingArea::new();
        canvas.set_vexpand(true);
        canvas.set_hexpand(true);
        canvas.set_content_width(960);
        canvas.set_content_height(540);
        {
            let s = slides.clone();
            let c = current_slide.clone();
            let so = selected_object.clone();
            canvas.set_draw_func(move |_area, cr, width, height| {
                draw_slide(cr, width as f64, height as f64, &s.borrow(), c.get(), so.get());
            });
        }

        let canvas_scroll = gtk::ScrolledWindow::new();
        canvas_scroll.set_child(Some(&canvas));
        canvas_scroll.set_vexpand(true);
        canvas_scroll.set_hexpand(true);
        canvas_scroll.set_min_content_width(400);
        canvas_scroll.set_min_content_height(300);

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
        rebuild_slide_list(&slide_list, &slides.borrow(), 0);

        let sidebar_controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        sidebar_controls.set_margin_start(6);
        sidebar_controls.set_margin_end(6);
        sidebar_controls.set_margin_top(6);
        sidebar_controls.set_margin_bottom(6);

        let add_btn = gtk::Button::builder()
            .icon_name("list-add-symbolic").tooltip_text("Add Slide").build();
        let del_btn = gtk::Button::builder()
            .icon_name("list-remove-symbolic").tooltip_text("Delete Slide").build();
        let up_btn = gtk::Button::builder()
            .icon_name("go-up-symbolic").tooltip_text("Move Up").build();
        let down_btn = gtk::Button::builder()
            .icon_name("go-down-symbolic").tooltip_text("Move Down").build();

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

        // ── Breakpoint ────────────────────────────────────────────────────
        let condition = adw::BreakpointCondition::parse("max-width: 600sp").unwrap();
        let bp = adw::Breakpoint::new(condition);
        let val = glib::Value::from(&true);
        bp.add_setter(&split_view, "collapsed", Some(&val));

        // ── SuiteWindow chrome ────────────────────────────────────────────
        let suite_win = SuiteWindow::new(app, "Decks", vec![], vec![]);
        suite_win.set_content(&split_view);

        let toolbar = build_decks_toolbar();
        suite_win.add_top_bar(&toolbar);

        // ── Wire sidebar signals ──────────────────────────────────────────
        let sl = slide_list.clone();
        let cs = canvas.clone();
        let cs_ref = current_slide.clone();
        let ss = slides.clone();
        slide_list.connect_row_selected(move |list, row| {
            if let Some(r) = row {
                let idx = r.index() as usize;
                if idx < ss.borrow().len() {
                    cs_ref.set(idx);
                    cs.queue_draw();
                }
            }
        });

        // Add slide
        {
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let cs_stack = content_stack.clone();
            let undo = undo.clone();
            add_btn.connect_clicked(move |_| {
                let idx = ss.borrow().len();
                let new_slide = Slide {
                    title: format!("Slide {}", idx + 1),
                    background: "#ffffff".into(),
                    objects: vec![],
                };
                undo.borrow_mut().execute(Box::new(AddSlideCmd {
                    index: idx,
                    slide: new_slide.clone(),
                }));
                rebuild_slide_list(&sl, &ss.borrow(), idx);
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
            del_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                let mut slides = ss.borrow_mut();
                if slides.len() > 1 && idx < slides.len() {
                    let removed = slides[idx].clone();
                    let new_idx = idx.min(slides.len().saturating_sub(2));
                    undo.borrow_mut().execute(Box::new(DeleteSlideCmd {
                        index: idx,
                        slide: removed,
                    }));
                    cs_ref.set(new_idx);
                    rebuild_slide_list(&sl, &slides, new_idx);
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
            up_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                if idx > 0 {
                    undo.borrow_mut().execute(Box::new(ReorderSlidesCmd {
                        from: idx, to: idx - 1,
                    }));
                    cs_ref.set(idx - 1);
                    rebuild_slide_list(&sl, &ss.borrow(), idx - 1);
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
            down_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                let slides = ss.borrow();
                if idx + 1 < slides.len() {
                    drop(slides);
                    undo.borrow_mut().execute(Box::new(ReorderSlidesCmd {
                        from: idx, to: idx + 1,
                    }));
                    cs_ref.set(idx + 1);
                    rebuild_slide_list(&sl, &ss.borrow(), idx + 1);
                    cs.queue_draw();
                }
            });
        }

        // ── Toolbar actions ───────────────────────────────────────────────
        // "Add Text Box" button
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let undo = undo.clone();
            let tb = find_toolbar_child(&toolbar, "insert-text-symbolic");
            if let Some(btn) = tb {
                btn.connect_clicked(move |_| {
                    let idx = cs_ref.get();
                    let obj = SlideObject::TextBox {
                        text: "Text".into(), x: 200.0, y: 150.0, w: 200.0, h: 40.0,
                    };
                    undo.borrow_mut().execute(Box::new(AddObjectCmd::new(idx, obj)));
                    cs.queue_draw();
                });
            }
        }

        // "Add Shape" button — cycles through Rect → Circle
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let shape_count = Rc::new(Cell::new(0u32));
            let undo = undo.clone();
            let tb = find_toolbar_child(&toolbar, "insert-object-symbolic");
            if let Some(btn) = tb {
                btn.connect_clicked(move |_| {
                    let idx = cs_ref.get();
                    let mut slides = ss.borrow_mut();
                    if idx < slides.len() {
                        let count = shape_count.get();
                        shape_count.set(count + 1);
                        if count % 2 == 0 {
                            slides[idx].objects.push(SlideObject::Rect {
                                x: 200.0, y: 200.0, w: 200.0, h: 150.0,
                            });
                        } else {
                            slides[idx].objects.push(SlideObject::Circle {
                                x: 300.0, y: 250.0, r: 80.0,
                            });
                        }
                        cs.queue_draw();
                    }
                });
            }
        }

        // "Add Image" button
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let w = suite_win.window.clone();
            let undo = undo.clone();
            let tb = find_toolbar_child(&toolbar, "insert-image-symbolic");
            if let Some(btn) = tb {
                btn.connect_clicked(move |_| {
                    let dlg = gtk::FileDialog::new();
                    let f = gtk::FileFilter::new();
                    f.add_mime_type("image/*");
                    f.set_name(Some("Images"));
                    let fl = gio::ListStore::new::<gtk::FileFilter>();
                    fl.append(&f);
                    dlg.set_filters(Some(&fl));
                    let ss = ss.clone(); let cs = cs.clone();
                    let cs_ref = cs_ref.clone(); let w2 = w.clone();
                    let undo = undo.clone();
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
            }
        }

        // Present button
        {
            let w = suite_win.window.clone();
            let tb = find_toolbar_child(&toolbar, "view-fullscreen-symbolic");
            if let Some(btn) = tb {
                btn.connect_clicked(move |_| {
                    w.fullscreen();
                });
            }
        }

        // ── Mouse interaction on canvas ──────────────────────────────────
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let click = gtk::GestureClick::new();
            click.connect_pressed(move |_g, _n, x, y| {
                let idx = cs_ref.get();
                let slides = ss.borrow();
                if idx >= slides.len() { return; }
                // Hit test objects in reverse order (topmost first)
                let objs = &slides[idx].objects;
                let mut found = None;
                for (oi, obj) in objs.iter().enumerate().rev() {
                    match obj {
                        SlideObject::TextBox { x: ox, y: oy, w: ow, h: oh, .. } |
                        SlideObject::Rect { x: ox, y: oy, w: ow, h: oh } => {
                            if x >= *ox && x <= *ox + *ow && y >= *oy && y <= *oy + *oh {
                                found = Some(oi); break;
                            }
                        }
                        SlideObject::Circle { x: cx, y: cy, r } => {
                            let dx = x - *cx; let dy = y - *cy;
                            if dx*dx + dy*dy <= *r * *r { found = Some(oi); break; }
                        }
                        SlideObject::Image { x: ox, y: oy, w: ow, h: oh, .. } => {
                            if x >= *ox && x <= *ox + *ow && y >= *oy && y <= *oy + *oh {
                                found = Some(oi); break;
                            }
                        }
                    }
                }
                so.set(found);
                cs.queue_draw();
            });
            canvas.add_controller(click);
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
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, code, mods| {
                // Ctrl+Z: undo
                if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) && keyval == gtk::gdk::Key::z {
                    let mut u = undo.borrow_mut();
                    if u.undo() {
                        cs.queue_draw();
                        let slides = ss.borrow();
                        rebuild_slide_list(&sl, &slides, cs_ref.get());
                    }
                    return glib::Propagation::Stop;
                }
                // Ctrl+Shift+Z: redo
                if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK) && keyval == gtk::gdk::Key::z {
                    let mut u = undo.borrow_mut();
                    if u.redo() {
                        cs.queue_draw();
                        let slides = ss.borrow();
                        rebuild_slide_list(&sl, &slides, cs_ref.get());
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
                            cs_ref.set(idx - 1);
                            rebuild_slide_list(&sl, &ss.borrow(), idx - 1);
                            cs.queue_draw();
                        }
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Right | gtk::gdk::Key::Down | gtk::gdk::Key::space => {
                        let idx = cs_ref.get();
                        let slides = ss.borrow();
                        if idx + 1 < slides.len() {
                            cs_ref.set(idx + 1);
                            rebuild_slide_list(&sl, &slides, idx + 1);
                            cs.queue_draw();
                        }
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Home => {
                        cs_ref.set(0);
                        rebuild_slide_list(&sl, &ss.borrow(), 0);
                        cs.queue_draw();
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::End => {
                        let slides = ss.borrow();
                        if !slides.is_empty() {
                            cs_ref.set(slides.len() - 1);
                            rebuild_slide_list(&sl, &slides, slides.len() - 1);
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
            let cs_scroll = canvas_scroll.clone();
            let path_ref = file_path.clone();
            let act = gtk::gio::SimpleAction::new("new-document", None);
            act.connect_activate(move |_, _| {
                if cs.child_by_name("editor").is_none() {
                    cs.add_titled(&cs_scroll, Some("editor"), "Editor");
                }
                cs.set_visible_child_name("editor");
                let mut slides = ss.borrow_mut();
                *slides = vec![Slide {
                    title: "Slide 1".into(),
                    background: "#ffffff".into(),
                    objects: vec![],
                }];
                *path_ref.borrow_mut() = None;
                rebuild_slide_list(&sl, &slides, 0);
                cs.queue_draw();
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
            let cs_scroll = canvas_scroll.clone();
            let path_ref = file_path.clone();

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
                                        rebuild_slide_list(&sl, &ss.borrow(), 0);
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
            act_save.connect_activate(move |_, _| {
                let current_path = path_clone.borrow().clone();
                if let Some(path_str) = current_path {
                    let deck = Deck { slides: ss_clone.borrow().clone() };
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

                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let deck = Deck { slides: ss.borrow().clone() };
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
            current_slide,
            selected_object,
            undo,
        }
    }

    pub fn present(&self) { self.window.present(); }
}

// ── Helper: rebuild the slide list widget ────────────────────────────────

fn rebuild_slide_list(list: &gtk::ListBox, slides: &[Slide], selected: usize) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for (i, _slide) in slides.iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(&format!("Slide {}", i + 1)));
        label.set_halign(gtk::Align::Start);
        label.set_margin_start(12);
        label.set_margin_end(12);
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        row.set_child(Some(&label));
        list.append(&row);
    }
    if let Some(row) = list.row_at_index(selected as i32) {
        list.select_row(Some(&row));
    }
}

// ── Helper: find toolbar child by icon name ──────────────────────────────

fn find_toolbar_child(toolbar: &gtk::Box, icon: &str) -> Option<gtk::Button> {
    let mut iter = toolbar.first_child();
    while let Some(child) = iter {
        if let Ok(btn) = child.clone().downcast::<gtk::Button>() {
            if btn.icon_name().map(|n| n == icon).unwrap_or(false) {
                return Some(btn);
            }
        }
        iter = child.next_sibling();
    }
    None
}

// ── Decks toolbar ────────────────────────────────────────────────────────

fn build_decks_toolbar() -> gtk::Box {
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    toolbar.set_margin_start(6);
    toolbar.set_margin_end(6);
    toolbar.set_halign(gtk::Align::Center);

    let bold = gtk::ToggleButton::builder()
        .icon_name("format-text-bold-symbolic").tooltip_text("Bold").build();
    let italic = gtk::ToggleButton::builder()
        .icon_name("format-text-italic-symbolic").tooltip_text("Italic").build();
    let underline = gtk::ToggleButton::builder()
        .icon_name("format-text-underline-symbolic").tooltip_text("Underline").build();
    for btn in [&bold, &italic, &underline] {
        btn.add_css_class("flat");
        toolbar.append(btn);
    }

    let sep = gtk::Separator::new(gtk::Orientation::Vertical);
    sep.set_margin_start(6); sep.set_margin_end(6);
    toolbar.append(&sep);

    let text_box = gtk::Button::builder()
        .icon_name("insert-text-symbolic").tooltip_text("Add Text Box").build();
    text_box.add_css_class("flat");
    toolbar.append(&text_box);

    let shape = gtk::Button::builder()
        .icon_name("insert-object-symbolic").tooltip_text("Add Shape (Rect/Circle)").build();
    shape.add_css_class("flat");
    toolbar.append(&shape);

    let image = gtk::Button::builder()
        .icon_name("insert-image-symbolic").tooltip_text("Add Image").build();
    image.add_css_class("flat");
    toolbar.append(&image);

    let sep2 = gtk::Separator::new(gtk::Orientation::Vertical);
    toolbar.append(&sep2);

    let present = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic").tooltip_text("Present (F5)").build();
    present.add_css_class("flat");
    present.add_css_class("suggested-action");
    toolbar.append(&present);

    toolbar
}

// ── Cairo slide rendering ─────────────────────────────────────────────────

fn draw_slide(
    cr: &cairo::Context, width: f64, height: f64,
    slides: &[Slide], current_slide: usize, selected: Option<usize>,
) {
    cr.set_source_rgb(0.86, 0.86, 0.86);
    cr.paint().unwrap();

    let slide_w = width * 0.85;
    let slide_h = slide_w * 9.0 / 16.0;
    let ox = (width - slide_w) / 2.0;
    let oy = (height - slide_h) / 2.0;

    // Shadow
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
    cr.rectangle(ox + 3.0, oy + 3.0, slide_w, slide_h);
    cr.fill().unwrap();

    // Slide background
    if current_slide < slides.len() {
        let bg = &slides[current_slide].background;
        if bg == "#ffffff" || bg.is_empty() {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        } else if bg.starts_with('#') && bg.len() >= 7 {
            let r = u8::from_str_radix(&bg[1..3], 16).unwrap_or(255) as f64 / 255.0;
            let g = u8::from_str_radix(&bg[3..5], 16).unwrap_or(255) as f64 / 255.0;
            let b = u8::from_str_radix(&bg[5..7], 16).unwrap_or(255) as f64 / 255.0;
            cr.set_source_rgb(r, g, b);
        } else { cr.set_source_rgb(1.0, 1.0, 1.0); }
    } else { cr.set_source_rgb(1.0, 1.0, 1.0); }
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.fill().unwrap();

    // Border
    cr.set_source_rgb(0.7, 0.7, 0.7);
    cr.set_line_width(1.0);
    cr.rectangle(ox, oy, slide_w, slide_h);
    cr.stroke().unwrap();

    // Draw objects
    if current_slide < slides.len() {
        for (oi, obj) in slides[current_slide].objects.iter().enumerate() {
            let is_selected = selected == Some(oi);
            match obj {
                SlideObject::TextBox { text, x, y, w, h } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    // Selection outline
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.rectangle(sx - 2.0, sy - 2.0, sw + 4.0, sh + 4.0);
                        cr.stroke().unwrap();
                    }
                    cr.set_source_rgb(0.1, 0.1, 0.1);
                    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                    cr.set_font_size(16.0);
                    cr.move_to(sx + 4.0, sy + 20.0);
                    let display = if text.len() > 20 { &text[..20] } else { text.as_str() };
                    cr.show_text(display).unwrap();
                }
                SlideObject::Rect { x, y, w, h } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.rectangle(sx - 2.0, sy - 2.0, sw + 4.0, sh + 4.0);
                        cr.stroke().unwrap();
                    }
                    cr.set_source_rgb(0.3, 0.5, 0.9);
                    cr.rectangle(sx, sy, sw, sh);
                    cr.fill().unwrap();
                }
                SlideObject::Circle { x, y, r } => {
                    let cx = ox + (x / 960.0) * slide_w;
                    let cy = oy + (y / 540.0) * slide_h;
                    let radius = (r / 540.0) * slide_h;
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.arc(cx, cy, radius + 2.0, 0.0, 2.0 * std::f64::consts::PI);
                        cr.stroke().unwrap();
                    }
                    cr.set_source_rgb(0.9, 0.3, 0.2);
                    cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
                    cr.fill().unwrap();
                }
                SlideObject::Image { path: _, x, y, w, h } => {
                    let sx = ox + (x / 960.0) * slide_w;
                    let sy = oy + (y / 540.0) * slide_h;
                    let sw = (w / 960.0) * slide_w;
                    let sh = (h / 540.0) * slide_h;
                    if is_selected {
                        cr.set_source_rgb(0.0, 0.5, 1.0);
                        cr.set_line_width(2.0);
                        cr.rectangle(sx - 2.0, sy - 2.0, sw + 4.0, sh + 4.0);
                        cr.stroke().unwrap();
                    }
                    // Image rendering: API varies by cairo-rs version
                    // For MVP, show a placeholder rectangle
                    cr.set_source_rgb(0.9, 0.9, 0.9);
                    cr.rectangle(sx, sy, sw, sh);
                    cr.fill().unwrap();
                }
            }
        }
    }

    // Empty slide indicator
    if current_slide < slides.len() && slides[current_slide].objects.is_empty() {
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.5);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(14.0);
        let text = format!("Slide {}", current_slide + 1);
        let extents = cr.text_extents(&text).unwrap();
        cr.move_to(ox + (slide_w - extents.width()) / 2.0, oy + slide_h - 20.0);
        cr.show_text(&text).unwrap();
    }

    // Slide number badge
    if current_slide < slides.len() {
        let badge = format!("{}", current_slide + 1);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.4);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(11.0);
        cr.move_to(ox + slide_w - 30.0, oy + 20.0);
        cr.show_text(&badge).unwrap();
    }
}
