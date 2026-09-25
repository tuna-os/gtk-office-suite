// SPDX-License-Identifier: GPL-3.0-or-later
//
// DecksWindow — Presentation window with slide sidebar + Cairo canvas.
// MVP: shapes, text boxes, images, present mode, fullscreen nav.

use adw::prelude::*;
use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use suite_common::SuiteWindow;
use crate::canvas::{canvas_to_slide, hit_test_object};
use crate::sidebar::rebuild_slide_list;
use crate::toolbar::{find_toolbar_child, build_decks_toolbar};
use crate::transition::{TransitionState, TransitionType, draw_transition};

use decks_core::engine::{Slide, SlideObject, MasterSlide, Deck};
use decks_core::{read_deck, write_deck, write_deck_bytes, DecksController};

/// Late-bound thumbnail-refresh callback (the slide list is built after
/// the closure that will eventually call it is created).
type ThumbUpdater = Rc<RefCell<Option<Box<dyn Fn()>>>>;

use crate::persistence::{autosave_format_hint, autosave_state_dir, next_doc_id};


// ── DecksWindow ──────────────────────────────────────────────────────────

pub struct DecksWindow {
    pub window: adw::ApplicationWindow,
    slide_list: gtk::ListBox,
    canvas: gtk::DrawingArea,
    /// Canonical slide-list state, undo history, and dirty flag — see
    /// decks_core::controller::DecksController (issue #103). window.rs
    /// no longer owns this state itself, only reads/writes through it.
    controller: Rc<DecksController>,
    current_slide: Rc<Cell<usize>>,
    selected_object: Rc<Cell<Option<usize>>>,
    content_stack: gtk::Stack,
    editor_split: adw::OverlaySplitView,
    file_path: Rc<RefCell<Option<String>>>,
    refresh_hud: Rc<dyn Fn()>,
    /// This window's own snapshot slot; recovery writes to it before
    /// clearing the orphan (see `AutosaveSlot::adopt_recovered`).
    autosave_slot: Rc<suite_common::autosave::AutosaveSlot>,
    /// Ownership of that slot, held for the window's lifetime so another
    /// launch does not offer this open deck as a crash recovery.
    _autosave_owner: Option<suite_common::autosave::SnapshotOwner>,
}

impl DecksWindow {
    pub fn new(app: &adw::Application) -> Self {
        let controller = Rc::new(DecksController::new(
            vec![Slide {
                title: "Slide 1".into(),
                background: "#ffffff".into(),
                objects: vec![],
                notes: String::new(),
                master_idx: Some(0),
                transition: Default::default(),
                builds: Vec::new(),
            }],
            vec![MasterSlide {
                name: "Default".into(),
                background: "#ffffff".into(),
                default_font: MasterSlide::DEFAULT_FONT.into(),
                shapes: vec![],
            }],
        ));
        let slides = controller.slides.clone();
        let masters = controller.masters.clone();
        let dirty = controller.dirty.clone();
        let current_slide = Rc::new(Cell::new(0usize));
        let selected_object = Rc::new(Cell::new(None));
        let file_path = controller.file_path.clone();

        // Test-only state snapshot (#104): only registered when
        // GTK_OFFICE_TEST_MODE is set — see tables/src/window.rs for the
        // identical pattern and rationale.
        if std::env::var_os("GTK_OFFICE_TEST_MODE").is_some() {
            let ctl = controller.clone();
            let act = gio::SimpleAction::new("test-snapshot", None);
            act.connect_activate(move |_, _| {
                let Ok(path) = std::env::var("GTK_OFFICE_SNAPSHOT_PATH") else { return };
                let snap = decks_core::snapshot::snapshot(&ctl);
                let _ = std::fs::write(path, snap.to_json());
            });
            app.add_action(&act);
        }
        let settings = gio::Settings::new("org.tunaos.decks");
        let autosave_slot = Rc::new(suite_common::autosave::AutosaveSlot::new(
            autosave_state_dir(), next_doc_id(),
        ));
        let snap_enabled = Rc::new(Cell::new(settings.boolean("snap-to-grid")));
        // Smart guides shown while an object is dragged (canvas_input.rs).
        let guides: Rc<RefCell<Vec<decks_core::guides::Guide>>> = Rc::default();
        {
            let se = snap_enabled.clone();
            settings.connect_changed(Some("snap-to-grid"), move |s, _| {
                se.set(s.boolean("snap-to-grid"));
            });
        }
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
        if std::env::var_os("GTK_OFFICE_TEST_MODE").is_some() {
            // Render lab Tier A (docs/RENDER-PARITY-ROADMAP.md): every
            // slide as the canvas draws it, 1280 px wide (13.33 in at 96
            // DPI, the size LibreOffice's reference PNGs come out at).
            let ctl = controller.clone();
            let dump_canvas = canvas.clone();
            let act = gio::SimpleAction::new("test-render-dump", None);
            act.connect_activate(move |_, _| {
                let Some(dir) = suite_common::render_dump::dump_dir() else { return };
                // Tier B can only see the slide on the canvas (slide 1).
                let slide = crate::canvas::slide_geometry(dump_canvas.width() as f64, dump_canvas.height() as f64);
                suite_common::render_dump::write_geometry(&dump_canvas, &[slide]);
                let screen = suite_common::render_dump::screen_path(&dir, 0);
                if let Err(e) = suite_common::render_dump::widget_to_png(&dump_canvas, Some(slide), &screen) {
                    eprintln!("render-dump: on-screen slide: {e}");
                }
                let slides = ctl.slides.borrow();
                let masters = ctl.masters.borrow();
                for i in 0..slides.len() {
                    let path = suite_common::render_dump::page_path(&dir, i);
                    if let Err(e) = crate::canvas::render_slide_png(&slides, &masters, i, 1280, &path) {
                        eprintln!("render-dump: slide {}: {e}", i + 1);
                    }
                }
            });
            app.add_action(&act);
        }
        // No fixed content size: the canvas fills the viewport and the
        // slide scales to fit (slide_geometry) — a fixed 960px minimum
        // made the scrolled window clip the slide at narrow widths.
        {
            let s = slides.clone();
            let c = current_slide.clone();
            let so = selected_object.clone();
            let ts = transition.clone();
            let m = masters.clone();
            let gd = guides.clone();
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
                // Also embed the canvas's position relative to the
                // toplevel window so GUI tests can compute click
                // coordinates: AT-SPI Component.position is broken for
                // widgets nested in box containers (upstream GTK4
                // bridge gap, tracked as #132), but GTK's own
                // compute_point works correctly.
                let desc = {
                    let n_objs = slides.get(cur).map(|sl| sl.objects.len()).unwrap_or(0);
                    let base = match so.get() {
                        Some(oi) => format!(
                            "slide {} of {}, {} objects, object {} selected",
                            cur + 1, slides.len(), n_objs, oi + 1
                        ),
                        None => format!(
                            "slide {} of {}, {} objects",
                            cur + 1, slides.len(), n_objs
                        ),
                    };
                    let pos_hint = area
                        .root()
                        .and_then(|root| area.compute_point(&root, &gtk::graphene::Point::new(0.0, 0.0)))
                        .map(|pt| format!(" canvas_at={},{}", pt.x() as i32, pt.y() as i32))
                        .unwrap_or_default();
                    format!("{}{}", base, pos_hint)
                };
                area.update_property(&[gtk::accessible::Property::Description(&desc)]);
                let accent = crate::canvas::accent_rgb(area);
                let selected_set: std::collections::HashSet<usize> = so.get().into_iter().collect();
                crate::canvas::draw_slide_multi(cr, width as f64, height as f64, &slides, cur, &selected_set, None, &m.borrow(), accent);
                crate::canvas::draw_guides(cr, width as f64, height as f64, &gd.borrow(), accent);
            });
        }

        let canvas_scroll = gtk::ScrolledWindow::new();
        canvas_scroll.set_child(Some(&canvas));
        canvas_scroll.set_vexpand(true);
        canvas_scroll.set_hexpand(true);
        canvas_scroll.set_min_content_width(400);
        canvas_scroll.set_min_content_height(300);

        // The Format inspector (right sidebar) is built once the HUD
        // refresh exists; it and the HUD refresh each other.
        let inspector_sync: crate::format_inspector::SyncSlot = Rc::default();

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
        prev_btn.set_tooltip_text(Some(&suite_common::i18n("Previous slide")));
        let present_btn = gtk::Button::from_icon_name("media-playback-start-symbolic");
        present_btn.set_tooltip_text(Some(&suite_common::i18n("Present (F5)")));
        present_btn.set_action_name(Some("app.present"));
        let next_btn = gtk::Button::from_icon_name("go-next-symbolic");
        next_btn.set_tooltip_text(Some(&suite_common::i18n("Next slide")));
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
        editor_split.set_content(Some(&canvas_overlay));
        editor_split.set_min_sidebar_width(280.0);
        editor_split.set_max_sidebar_width(340.0);

        // Central HUD refresh: status text + inspector fields.
        // The thumbnail updater is late-bound (the slide list is built
        // after this closure).
        let thumb_updater: ThumbUpdater = Rc::new(RefCell::new(None));
        let refresh_hud: Rc<dyn Fn()> = {
            let ss = slides.clone();
            let cs_ref = current_slide.clone();
            let so = selected_object.clone();
            let insp = inspector_sync.clone();
            let status = status_label.clone();
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
                drop(slides);
                if let Some(sync) = insp.borrow().as_ref() {
                    sync();
                }
            })
        };
        refresh_hud();

        // ── Format inspector ─────────────────────────────────────────────
        let format_toggle = {
            let refresh = refresh_hud.clone();
            let da = canvas.clone();
            let changed: Rc<dyn Fn()> = Rc::new(move || {
                refresh();
                da.queue_draw();
            });
            let inspector = crate::format_inspector::build(&controller, &current_slide, &selected_object, changed);
            editor_split.set_sidebar(Some(&inspector.sidebar));
            *inspector_sync.borrow_mut() = Some(inspector.sync.clone());
            (inspector.sync)();
            let show = gtk::ToggleButton::builder()
                .icon_name("sidebar-show-right-symbolic")
                .tooltip_text(suite_common::i18n("Format"))
                .build();
            show.update_property(&[gtk::accessible::Property::Label("Format")]);
            show.bind_property("active", &editor_split, "show-sidebar").bidirectional().sync_create().build();
            show
        };


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

        // ── SuiteWindow chrome ────────────────────────────────────────────
        let suite_win = SuiteWindow::new(app, "Decks", vec![], vec![]);
        suite_win.header_bar.pack_end(&format_toggle);
        suite_common::bind_window_geometry(&suite_win.window, &settings);

        // ── File Drag and Drop Support ────────────────────────────────────
        {
            let win_weak = suite_win.window.downgrade();
            let ctl = controller.clone();
            let cs_ref = current_slide.clone();
            let cs = canvas.clone();
            let refresh = refresh_hud.clone();
            let sl = slide_list.clone();
            let stk = content_stack.clone();
            let ed_split = editor_split.clone();
            let fp = file_path.clone();
            suite_common::attach_file_drop_target(&suite_win.window, move |paths| {
                for path in paths {
                    let path_str = path.to_string_lossy().to_string();
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    if ext == "pptx" || ext == "odp" || ext == "deck" {
                        if let Ok(deck) = read_deck(&path_str) {
                            *ctl.slides.borrow_mut() = deck.slides;
                            *ctl.masters.borrow_mut() = deck.masters;
                            cs_ref.set(0);
                            *fp.borrow_mut() = Some(path_str.clone());
                            if stk.child_by_name("editor").is_none() {
                                stk.add_titled(&ed_split, Some("editor"), "Editor");
                            }
                            stk.set_visible_child_name("editor");
                            rebuild_slide_list(&sl, &ctl.slides.borrow().clone(), &ctl.masters.borrow(), 0);
                            if let Some(name) = path.file_name() {
                                if let Some(w) = win_weak.upgrade() {
                                    w.set_title(Some(&format!("{} — Decks", name.to_string_lossy())));
                                }
                            }
                            let settings = gio::Settings::new("org.tunaos.decks");
                            suite_common::push_recent_file(&settings, &path_str);
                            cs.queue_draw();
                            refresh();
                        }
                    } else if ext == "png" || ext == "jpg" || ext == "jpeg" || ext == "svg" {
                        let idx = cs_ref.get();
                        let obj = SlideObject::Image {
                            path: path_str,
                            x: 150.0,
                            y: 100.0,
                            w: 300.0,
                            h: 200.0,
                            rotation: 0.0,
                        };
                        ctl.add_object(idx, obj);
                        cs.queue_draw();
                        refresh();
                    }
                }
            });
        }

        // ── Window close-request: Save / Discard / Cancel guard ──────────
        // Same force_close re-entrancy pattern as Letters/Tables: a dialog
        // response can't be awaited inside close-request, so the first
        // close is stopped and the dialog's own callback re-invokes
        // .close() with the guard set so this handler lets it through.
        {
            let dirty = dirty.clone();
            let ss = slides.clone();
            let m = masters.clone();
            let path_state = file_path.clone();
            let slot = autosave_slot.clone();
            let force_close = Rc::new(Cell::new(false));
            suite_win.window.connect_close_request(move |win| {
                if force_close.get() {
                    return glib::Propagation::Proceed;
                }
                if !dirty.get() {
                    return glib::Propagation::Proceed;
                }
                let dialog = adw::AlertDialog::builder()
                    .heading("Save changes?")
                    .body("This presentation has unsaved changes. If you close without saving, they will be lost.")
                    .build();
                dialog.add_responses(&[
                    ("cancel", "_Cancel"),
                    ("discard", "_Discard"),
                    ("save", "_Save"),
                ]);
                dialog.set_close_response("cancel");
                dialog.set_default_response(Some("save"));
                dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

                let win_weak = win.downgrade();
                let force_close = force_close.clone();
                let dirty = dirty.clone();
                let ss = ss.clone();
                let m = m.clone();
                let path_state = path_state.clone();
                let slot = slot.clone();
                dialog.choose(Some(win), None::<&gio::Cancellable>, move |response: glib::GString| {
                    let Some(win) = win_weak.upgrade() else { return };
                    if response == "discard" {
                        slot.clear_or_report();
                        force_close.set(true);
                        win.close();
                        return;
                    }
                    if response != "save" {
                        return;
                    }
                    let existing_path = path_state.borrow().clone();
                    if let Some(path) = existing_path {
                        let deck = Deck { slides: ss.borrow().clone(), masters: m.borrow().clone() };
                        match write_deck(&path, &deck) {
                            Ok(()) => {
                                dirty.set(false);
                                slot.clear_or_report();
                                force_close.set(true);
                                win.close();
                            }
                            Err(e) => {
                                let err = adw::AlertDialog::builder()
                                    .heading(suite_common::i18n("Error saving file"))
                                    .body(&e)
                                    .build();
                                err.add_response("ok", &suite_common::i18n("OK"));
                                err.present(Some(&win));
                            }
                        }
                        return;
                    }
                    // Never saved: prompt for a destination, then close only
                    // once that save actually succeeds.
                    let dlg = gtk::FileDialog::new();
                    let f = gtk::FileFilter::new();
                    f.add_pattern("*.pptx");
                    f.set_name(Some("PowerPoint Presentations (.pptx)"));
                    let odp = gtk::FileFilter::new();
                    odp.add_pattern("*.odp");
                    odp.set_name(Some("OpenDocument Presentations (.odp)"));
                    let fl = gio::ListStore::new::<gtk::FileFilter>();
                    fl.append(&f);
                    fl.append(&odp);
                    dlg.set_filters(Some(&fl));
                    dlg.set_initial_name(Some("Untitled.pptx"));
                    let win2 = win.clone();
                    let slot = slot.clone();
                    dlg.save(Some(&win), None::<&gio::Cancellable>, move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let deck = Deck { slides: ss.borrow().clone(), masters: m.borrow().clone() };
                                match write_deck(&path_str, &deck) {
                                    Ok(()) => {
                                        *path_state.borrow_mut() = Some(path_str);
                                        dirty.set(false);
                                        slot.clear_or_report();
                                        force_close.set(true);
                                        win2.close();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading(suite_common::i18n("Error saving file"))
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", &suite_common::i18n("OK"));
                                        err.present(Some(&win2));
                                    }
                                }
                            }
                        }
                    });
                });
                glib::Propagation::Stop
            });
        }

        // Medium breakpoint (≤ 800sp): both sidebars become hidden
        // overlays and the canvas gives up its fixed minimum — otherwise
        // the editor demands ~770px and the header bar's menu and window
        // controls get clipped off-screen (fixes #79).
        let t = glib::Value::from(&true);
        let f = glib::Value::from(&false);
        let mbp = &suite_win.medium_breakpoint;
        mbp.add_setter(&split_view, "collapsed", Some(&t));
        mbp.add_setter(&split_view, "show-sidebar", Some(&f));
        mbp.add_setter(&editor_split, "collapsed", Some(&t));
        mbp.add_setter(&editor_split, "show-sidebar", Some(&f));
        mbp.add_setter(&canvas_scroll, "min-content-width", Some(&glib::Value::from(&240i32)));
        // The status caption collides with the centered presenter pill.
        mbp.add_setter(&status_label, "visible", Some(&f));

        // Narrow breakpoint (≤ 500sp): hide sidebars and reduce
        // minimum canvas width further.  The toolbar is hidden below
        // once it has been created.
        let nbp = &suite_win.narrow_breakpoint;
        nbp.add_setter(&split_view, "collapsed", Some(&t));
        nbp.add_setter(&split_view, "show-sidebar", Some(&f));
        nbp.add_setter(&editor_split, "collapsed", Some(&t));
        nbp.add_setter(&editor_split, "show-sidebar", Some(&f));
        nbp.add_setter(&canvas_scroll, "min-content-width", Some(&glib::Value::from(&180i32)));
        nbp.add_setter(&status_label, "visible", Some(&f));

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
        let autosave_notices = suite_common::autosave_notice::AutosaveNotifier::new(&toast_overlay);

        let toolbar = build_decks_toolbar();
        suite_win.add_top_bar(&toolbar);
        // Narrow breakpoint: hide the editing toolbar entirely —
        // only the header bar, canvas, and pill survive.
        suite_win.narrow_breakpoint.add_setter(&toolbar, "visible", Some(&f));

        // ── Wire sidebar signals ──────────────────────────────────────────
        let _sl = slide_list.clone();
        let cs = canvas.clone();
        let cs_ref = current_slide.clone();
        let ss = slides.clone();
        let notes_skip = Rc::new(Cell::new(false));
        let notes_skip2 = notes_skip.clone();
        let nb = notes_buffer.clone();
        slide_list.connect_row_selected(move |_list, row| {
            if let Some(r) = row {
                let idx = r.index() as usize;
                if idx < ss.borrow().len() {
                    cs_ref.set(idx);
                    cs.queue_draw();
                    let slides = ss.borrow();
                    if let Some(slide) = slides.get(idx) {
                        let notes = slide.notes.clone();
                        // Don't hold ss's Ref across nb.set_text() below --
                        // GTK's set_text does a delete-then-insert and can
                        // emit `changed` twice, and the second emission's
                        // handler takes ss.borrow_mut() (see notes_skip
                        // comment below).
                        drop(slides);
                        notes_skip2.set(true);
                        nb.set_text(&notes);
                        notes_skip2.set(false);
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
                // `skip` is cleared by the row-selected handler once its
                // set_text() call fully returns, not here -- set_text can
                // emit `changed` twice (delete, then insert) and clearing
                // it after the first emission left the second free to
                // reach ss.borrow_mut() while that handler's ss.borrow()
                // was still alive (the exact Tables reentrancy bug class).
                if skip.get() { return; }
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
            let controller = controller.clone();
            let masters = masters.clone();
            add_btn.connect_clicked(move |_| {
                let idx = ss.borrow().len();
                let new_slide = Slide {
                    title: format!("Slide {}", idx + 1),
                    background: "#ffffff".into(),
                    objects: vec![],
                    notes: String::new(),
            master_idx: Some(0),
            transition: Default::default(),
            builds: Vec::new(),
                };
                let idx = controller.add_slide(idx, new_slide);
                rebuild_slide_list(&sl, &ss.borrow().clone(), &masters.borrow(), idx);
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
            let controller = controller.clone();
            let masters = masters.clone();
            del_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                if let Some(new_idx) = controller.delete_slide(idx) {
                    cs_ref.set(new_idx);
                    rebuild_slide_list(&sl, &ss.borrow().clone(), &masters.borrow(), new_idx);
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
            let controller = controller.clone();
            let masters = masters.clone();
            up_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                if let Some(new_idx) = controller.move_slide_up(idx) {
                    cs_ref.set(new_idx);
                    rebuild_slide_list(&sl, &ss.borrow().clone(), &masters.borrow(), new_idx);
                    cs.queue_draw();
                }
            });
        }
        {
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let controller = controller.clone();
            let masters = masters.clone();
            down_btn.connect_clicked(move |_| {
                let idx = cs_ref.get();
                if let Some(new_idx) = controller.move_slide_down(idx) {
                    cs_ref.set(new_idx);
                    rebuild_slide_list(&sl, &ss.borrow().clone(), &masters.borrow(), new_idx);
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
            ("app.rehearse", "Rehearse Presentation"),
            ("app.undo", "Undo"),
            ("app.redo", "Redo"),
        ]);

        // Expose the canonical controller history to Gio automation as well
        // as the keyboard path below.  Keeping both routes on the same
        // controller prevents tests and accessibility tools from exercising a
        // different, stale document state.
        for (name, is_undo) in [("undo", true), ("redo", false)] {
            let controller = controller.clone();
            let cs = canvas.clone();
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs_ref = current_slide.clone();
            let masters = masters.clone();
            let refresh = refresh_hud.clone();
            let act = gio::SimpleAction::new(name, None);
            act.connect_activate(move |_, _| {
                let changed = if is_undo { controller.undo() } else { controller.redo() };
                if changed {
                    cs.queue_draw();
                    let snapshot = ss.borrow().clone();
                    rebuild_slide_list(&sl, &snapshot, &masters.borrow(), cs_ref.get());
                    // The inspector shows the undone state too.
                    refresh();
                }
            });
            app.add_action(&act);
        }

        // "Add Text Box"
        {
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let controller = controller.clone();
            let refresh = refresh_hud.clone();
            let so = selected_object.clone();
            let act = gio::SimpleAction::new("add-text-box", None);
            act.connect_activate(move |_, _| {
                let idx = cs_ref.get();
                let obj = SlideObject::TextBox {
                    text: "Text".into(), x: 200.0, y: 150.0, w: 200.0, h: 40.0,
                    rotation: 0.0,
                    runs: vec![],
                    body: Default::default(),
                };
                controller.add_object(idx, obj);
                // What was just inserted is selected, ready to format.
                so.set(controller.slides.borrow().get(idx).map(|s| s.objects.len().saturating_sub(1)));
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
            let controller = controller.clone();
            let refresh = refresh_hud.clone();
            let so = selected_object.clone();
            let act = gio::SimpleAction::new("add-shape", None);
            act.connect_activate(move |_, _| {
                let idx = cs_ref.get();
                let ss_snap = ss.borrow();
                if idx >= ss_snap.len() { return; }
                let count = shape_count.get();
                shape_count.set(count + 1);
                // A styled shape in the theme's default look (accent fill,
                // darker outline), which the file then carries: not the old
                // unstyled Rect/Circle that the canvas painted blue/red.
                use decks_core::engine::shape::{ShapeKind, ShapeStyle};
                let obj = if count.is_multiple_of(2) {
                    SlideObject::Shape { kind: ShapeKind::Rect, x: 200.0, y: 200.0, w: 200.0, h: 150.0, rotation: 0.0, style: ShapeStyle::default() }
                } else {
                    SlideObject::Shape { kind: ShapeKind::Ellipse, x: 220.0, y: 170.0, w: 160.0, h: 160.0, rotation: 0.0, style: ShapeStyle::default() }
                };
                drop(ss_snap);
                controller.add_object(idx, obj);
                so.set(controller.slides.borrow().get(idx).map(|s| s.objects.len().saturating_sub(1)));
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
            let controller = controller.clone();
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
                let controller = controller.clone();
                let _refresh = refresh.clone();
                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let idx = cs_ref.get();
                                let p = path.to_string_lossy().to_string();
                                let obj = SlideObject::Image {
                                    path: p, x: 200.0, y: 200.0, w: 200.0, h: 150.0, rotation: 0.0,
                                };
                                controller.add_object(idx, obj);
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
            // A show from the slide being edited: the audience fullscreen
            // (on the second monitor if there is one) and, with two
            // monitors, the presenter display (presenter_window.rs).
            for (name, rehearse) in [("present", false), ("rehearse", true)] {
                let (ss, m, cs_ref, app2) = (slides.clone(), masters.clone(), current_slide.clone(), app.clone());
                let act = gio::SimpleAction::new(name, None);
                act.connect_activate(move |_, _| {
                    let deck = Deck { slides: ss.borrow().clone(), masters: m.borrow().clone() };
                    crate::presenter_window::start(&app2, deck, cs_ref.get(), rehearse);
                });
                app.add_action(&act);
            }
            app.set_accels_for_action("app.present", &["F5"]);
            if let Some(btn) = find_toolbar_child(&toolbar, "view-fullscreen-symbolic") {
                btn.set_action_name(Some("app.present"));
            }
        }

        // "Go to slide N" (0-based), for the command line and automation:
        // `gapplication action org.tunaos.decks go-to-slide "uint32 1"`.
        {
            let (ss, cs_ref, cs, sl, m, so) =
                (slides.clone(), current_slide.clone(), canvas.clone(), slide_list.clone(), masters.clone(), selected_object.clone());
            let refresh = refresh_hud.clone();
            let act = gio::SimpleAction::new("go-to-slide", Some(glib::VariantTy::UINT32));
            act.connect_activate(move |_, param| {
                let Some(i) = param.and_then(|p| p.get::<u32>()) else { return };
                let snap = ss.borrow().clone();
                let i = i as usize;
                if i >= snap.len() {
                    return;
                }
                cs_ref.set(i);
                so.set(None);
                rebuild_slide_list(&sl, &snap, &m.borrow(), i);
                cs.queue_draw();
                refresh();
            });
            app.add_action(&act);
        }

        // "Preview transition": play how the current slide arrives, from
        // the one before it, on the canvas (the inspector's Slide page).
        {
            let (ss, cs_ref, cs, ts, m) = (slides.clone(), current_slide.clone(), canvas.clone(), transition.clone(), masters.clone());
            let act = gio::SimpleAction::new("preview-transition", None);
            act.connect_activate(move |_, _| {
                let slides = ss.borrow();
                let idx = cs_ref.get();
                let Some(to) = slides.get(idx) else { return };
                let blank = Slide { objects: vec![], ..to.clone() };
                let from = idx.checked_sub(1).and_then(|i| slides.get(i)).unwrap_or(&blank);
                let kind = TransitionType::of(to.transition);
                TransitionState::start(&ts, kind, from, to, &m.borrow(), &cs);
                crate::transition::dump_midpoint(&ts.borrow(), &cs);
            });
            app.add_action(&act);
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
            let controller2 = controller.clone();
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
                let controller3 = controller2.clone();
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
                            controller3.add_object(idx, obj);
                            cs2.queue_draw();
                            refresh2();
                        }
                    },
                );
                glib::Propagation::Stop
            });
            suite_win.window.add_controller(key);
        }

        // Pointer interaction on the canvas lives in canvas_input.rs.
        crate::canvas_input::register_canvas_pointer_input(
            &canvas,
            &slides,
            &current_slide,
            &selected_object,
            &controller,
            &refresh_hud,
            &snap_enabled,
            &guides,
        );

        // ── Double-click: inline text edit on TextBox ───────────────────
        {
            let ss = slides.clone();
            let cs = canvas.clone();
            let cs_ref = current_slide.clone();
            let _so = selected_object.clone();
            let controller = controller.clone();
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
                        if let Some(p) = cs2.parent() {
                            if let Ok(fixed) = p.downcast::<gtk::Fixed>() {
                                fixed.put(&overlay, 0.0, 0.0);
                            }
                        }
                        text_view.grab_focus();
                        // Commit on Enter via EventControllerKey
                        let key_ctrl = gtk::EventControllerKey::new();
                        let _ss2 = ss.clone();
                        let cs3 = cs.clone();
                        let controller2 = controller.clone();
                        let tv2 = text_view.clone();
                        let ov2 = overlay.clone();
                        let cs_ref2 = cs_ref.clone();
                        key_ctrl.connect_key_pressed(move |_, key, _code, _mod| {
                            if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
                                let buf = tv2.buffer();
                                let new_text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                                if new_text != old_text {
                                    controller2.change_text(cs_ref2.get(), oi, old_text.clone(), new_text);
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
                        let _ov3 = overlay.clone();
                        fc.connect_leave(move |_| {
                            if let Some(p) = tv3.parent() { p.unparent(); }
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
            let controller = controller.clone();
            let ts = transition.clone();
            let m = masters.clone();
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _code, mods| {
                // Ctrl+Z: undo
                if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) && keyval == gtk::gdk::Key::z {
                    if controller.undo() {
                        cs.queue_draw();
                        // Snapshot instead of holding ss.borrow() across the
                        // call -- rebuild_slide_list()'s select_row() fires
                        // row-selected synchronously, which reaches ss.borrow_mut().
                        let slides = ss.borrow().clone();
                        rebuild_slide_list(&sl, &slides, &m.borrow(), cs_ref.get());
                    }
                    return glib::Propagation::Stop;
                }
                // Ctrl+Shift+Z: redo
                if mods.contains(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK) && keyval == gtk::gdk::Key::z {
                    if controller.redo() {
                        cs.queue_draw();
                        let slides = ss.borrow().clone();
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
                                // Backwards, the slide being left plays
                                // its own transition in reverse.
                                TransitionState::start(&ts, TransitionType::of(sls[idx].transition),
                                    &sls[idx], &sls[idx - 1], &m.borrow(), &cs);
                            }
                            // Snapshot + drop before rebuild_slide_list(),
                            // whose select_row() reaches ss.borrow_mut().
                            let snap = sls.clone();
                            drop(sls);
                            cs_ref.set(idx - 1);
                            rebuild_slide_list(&sl, &snap, &m.borrow(), idx - 1);
                            cs.queue_draw();
                        }
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Right | gtk::gdk::Key::Down | gtk::gdk::Key::space => {
                        let idx = cs_ref.get();
                        let slides = ss.borrow();
                        if idx + 1 < slides.len() {
                            TransitionState::start(&ts, TransitionType::of(slides[idx + 1].transition),
                                &slides[idx], &slides[idx + 1], &m.borrow(), &cs);
                            let snap = slides.clone();
                            drop(slides);
                            cs_ref.set(idx + 1);
                            rebuild_slide_list(&sl, &snap, &m.borrow(), idx + 1);
                            cs.queue_draw();
                        }
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Home => {
                        cs_ref.set(0);
                        let snap = ss.borrow().clone();
                        rebuild_slide_list(&sl, &snap, &m.borrow(), 0);
                        cs.queue_draw();
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::End => {
                        let slides = ss.borrow();
                        if !slides.is_empty() {
                            let snap = slides.clone();
                            let last = snap.len() - 1;
                            drop(slides);
                            cs_ref.set(last);
                            rebuild_slide_list(&sl, &snap, &m.borrow(), last);
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
                                    controller.delete_object(idx, oi, obj);
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
                        transition: Default::default(),
                        builds: Vec::new(),
                    }];
                }
                *path_ref.borrow_mut() = None;
                rebuild_slide_list(&sl, &ss.borrow().clone(), &masters.borrow(), 0);
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
                let all = gtk::FileFilter::new();
                all.add_pattern("*.pptx");
                all.add_pattern("*.odp");
                all.set_name(Some("Presentations (.pptx, .odp)"));
                let f = gtk::FileFilter::new();
                f.add_pattern("*.pptx");
                f.set_name(Some("PowerPoint Presentations (.pptx)"));
                let odp = gtk::FileFilter::new();
                odp.add_pattern("*.odp");
                odp.set_name(Some("OpenDocument Presentations (.odp)"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&all);
                fl.append(&f);
                fl.append(&odp);
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
                                match read_deck(&path_str) {
                                    Ok(deck) => {
                                        *ss.borrow_mut() = deck.slides;
                                        *masters.borrow_mut() = deck.masters;
                                        cs_ref.set(0);
                                        so.set(None);
                                        *path_ref.borrow_mut() = Some(path_str);
                                        if cs.child_by_name("editor").is_none() {
                                            cs.add_titled(&cs_scroll, Some("editor"), "Editor");
                                        }
                                        cs.set_visible_child_name("editor");
                                        rebuild_slide_list(&sl, &ss.borrow().clone(), &masters.borrow(), 0);
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
                                            .heading(suite_common::i18n("Error opening presentation"))
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", &suite_common::i18n("OK"));
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
            let dirty_save = dirty.clone();
            let slot_save = autosave_slot.clone();
            act_save.connect_activate(move |_, _| {
                let current_path = path_clone.borrow().clone();
                if let Some(path_str) = current_path {
                    let deck = Deck { slides: ss_clone.borrow().clone(), masters: m_save.borrow().clone() };
                    match write_deck(&path_str, &deck) {
                        Ok(()) => {
                            let settings = gio::Settings::new("org.tunaos.decks");
                            suite_common::push_recent_file(&settings, &path_str);
                            dirty_save.set(false);
                            slot_save.clear_or_report();
                        }
                        Err(e) => {
                            let err = adw::AlertDialog::builder()
                                .heading(suite_common::i18n("Error saving presentation"))
                                .body(&e)
                                .build();
                            err.add_response("ok", &suite_common::i18n("OK"));
                            err.set_default_response(Some("ok"));
                            err.present(Some(&w_clone));
                        }
                    }
                } else {
                    let _ = gtk4::prelude::WidgetExt::activate_action(&w_clone, "app.save-file-as", None);
                }
            });
            app.add_action(&act_save);

            let act_save_as = gtk::gio::SimpleAction::new("save-file-as", None);
            let m_as = masters.clone();
            let dirty_as = dirty.clone();
            let slot_as = autosave_slot.clone();
            act_save_as.connect_activate(move |_, _| {
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.pptx");
                f.set_name(Some("PowerPoint Presentations (.pptx)"));
                let odp = gtk::FileFilter::new();
                odp.add_pattern("*.odp");
                odp.set_name(Some("OpenDocument Presentations (.odp)"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                fl.append(&odp);
                dlg.set_filters(Some(&fl));
                dlg.set_initial_name(Some("Untitled.pptx"));

                let ss = ss.clone();
                let w2 = w.clone();
                let path_ref = path_ref.clone();
                let m_inner = m_as.clone();
                let dirty_as = dirty_as.clone();
                let slot_as = slot_as.clone();
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let deck = Deck { slides: ss.borrow().clone(), masters: m_inner.borrow().clone() };
                                match write_deck(&path_str, &deck) {
                                    Ok(()) => {
                                        let settings = gio::Settings::new("org.tunaos.decks");
                                        suite_common::push_recent_file(&settings, &path_str);
                                        *path_ref.borrow_mut() = Some(path_str);
                                        dirty_as.set(false);
                                        slot_as.clear_or_report();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading(suite_common::i18n("Error saving presentation"))
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", &suite_common::i18n("OK"));
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

        // ── Autosave: periodic crash-recovery snapshot ──────────────────
        // Mirrors Tables: writes to the state-dir slot, never to the deck's
        // own path, and never clears `dirty` — a snapshot is not a save,
        // the close guard still needs to fire.
        {
            let ss = slides.clone();
            let m = masters.clone();
            let dirty = dirty.clone();
            let slot = autosave_slot.clone();
            let notices = autosave_notices.clone();
            let path_state = file_path.clone();
            let act = gtk::gio::SimpleAction::new("autosave-now", None);
            act.connect_activate(move |_, _| {
                if !dirty.get() {
                    return;
                }
                let deck = Deck { slides: ss.borrow().clone(), masters: m.borrow().clone() };
                let path = path_state.borrow().clone();
                let kind = autosave_format_hint(&path);
                if let Ok(bytes) = write_deck_bytes(&kind, &deck) {
                    let meta = suite_common::autosave::SnapshotMeta {
                        original_path: path.map(std::path::PathBuf::from),
                        kind,
                    };
                    notices.record(slot.write(&bytes, &meta));
                }
            });
            app.add_action(&act);
        }
        {
            let ss = slides.clone();
            let m = masters.clone();
            let dirty = dirty.clone();
            let slot = autosave_slot.clone();
            let notices = autosave_notices.clone();
            let path_state = file_path.clone();
            let interval = settings.int("auto-save-interval").max(10) as u32;
            let enabled = settings.boolean("auto-save");
            if enabled {
                glib::source::timeout_add_seconds_local(interval, move || {
                    if dirty.get() {
                        let deck = Deck { slides: ss.borrow().clone(), masters: m.borrow().clone() };
                        let path = path_state.borrow().clone();
                        let kind = autosave_format_hint(&path);
                        if let Ok(bytes) = write_deck_bytes(&kind, &deck) {
                            let meta = suite_common::autosave::SnapshotMeta {
                                original_path: path.map(std::path::PathBuf::from),
                                kind,
                            };
                            notices.record(slot.write(&bytes, &meta));
                        }
                    }
                    glib::ControlFlow::Continue
                });
            }
        }

        Self {
            _autosave_owner: autosave_slot.claim(),
            autosave_slot: autosave_slot.clone(),
            window: suite_win.window,
            slide_list,
            canvas,
            controller,
            current_slide,
            selected_object,
            content_stack,
            editor_split,
            file_path,
            refresh_hud,
        }
    }

    pub fn present(&self) { self.window.present(); }

    /// Check for a snapshot orphaned by a crash and load it if found. Call
    /// once, right after construction, before any explicit CLI-open — an
    /// explicit open target should win over recovering an unrelated
    /// document. Returns true if a snapshot was recovered.
    ///
    /// A window holds one deck, so this recovers the newest snapshot —
    /// `find_orphaned_snapshots` orders them — and tries each candidate in
    /// turn, because reading back whole and *loading* are different things
    /// and giving up on the first failure left a snapshot that never loads
    /// burying the rest on every launch. The ones not recovered here stay on
    /// disk for the next launch. Same shape as Tables; see that one.
    pub fn recover_from_snapshot(&self) -> bool {
        let state_dir = autosave_state_dir();
        for orphan_id in suite_common::autosave::find_orphaned_snapshots(&state_dir) {
            let orphan = suite_common::autosave::AutosaveSlot::new(state_dir.clone(), orphan_id);
            let Some((bytes, meta)) = orphan.read() else { continue };
            let ext = if meta.kind == "odp" { "odp" } else { "pptx" };
            let tmp = std::env::temp_dir().join(format!("decks-recovery-{}.{ext}", std::process::id()));
            if std::fs::write(&tmp, &bytes).is_err() {
                continue;
            }
            let recovered = self.open_path(&tmp.to_string_lossy()).is_ok();
            let _ = std::fs::remove_file(&tmp);
            if !recovered {
                continue;
            }
            // open_path() pointed file_path at the temp recovery file and left
            // `dirty` untouched (it's a plain field write, not an undo-stack
            // mutation) — recovered content targets the *original* path (or
            // none) and must count as dirty so the close guard offers to
            // save it.
            *self.file_path.borrow_mut() = meta.original_path.as_ref().map(|p| p.to_string_lossy().to_string());
            self.controller.dirty.set(true);
            let name = meta.original_path.as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string());
            self.window.set_title(Some(&format!("{name} (Recovered) — Decks")));
            // Order and failure handling: AutosaveSlot::adopt_recovered.
            if self.autosave_slot.adopt_recovered(&bytes, &meta) {
                orphan.clear_or_report();
            }
            return true;
        }
        false
    }

    /// Open a .pptx or .odp directly (CLI / file-manager open). Mirrors the
    /// open-file dialog success path.
    pub fn open_path(&self, path: &str) -> Result<(), String> {
        let deck = read_deck(path)?;
        *self.controller.slides.borrow_mut() = deck.slides;
        *self.controller.masters.borrow_mut() = deck.masters;
        self.current_slide.set(0);
        self.selected_object.set(None);
        *self.file_path.borrow_mut() = Some(path.to_string());
        if self.content_stack.child_by_name("editor").is_none() {
            self.content_stack.add_titled(&self.editor_split, Some("editor"), "Editor");
        }
        self.content_stack.set_visible_child_name("editor");
        rebuild_slide_list(&self.slide_list, &self.controller.slides.borrow().clone(), &self.controller.masters.borrow(), 0);
        if let Some(name) = std::path::Path::new(path).file_name() {
            self.window
                .set_title(Some(&format!("{} — Decks", name.to_string_lossy())));
        }
        self.canvas.queue_draw();
        (self.refresh_hud)();
        let settings = gio::Settings::new("org.tunaos.decks");
        suite_common::push_recent_file(&settings, path);
        Ok(())
    }
}

// ── Helper: rebuild the slide list widget ────────────────────────────────
// force rebuild
