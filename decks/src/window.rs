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
                ids: Default::default(),
                layout: None,
            }],
            vec![MasterSlide {
                name: "Default".into(),
                background: "#ffffff".into(),
                default_font: MasterSlide::DEFAULT_FONT.into(),
                shapes: vec![],
                page_emu: None,
                layouts: Vec::new(),
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
            let (ctl, cs, so) = (controller.clone(), current_slide.clone(), selected_object.clone());
            let act = gio::SimpleAction::new("test-snapshot", None);
            act.connect_activate(move |_, _| {
                let Ok(path) = std::env::var("GTK_OFFICE_SNAPSHOT_PATH") else { return };
                let mut snap = decks_core::snapshot::snapshot(&ctl);
                snap.selection = Some((cs.get(), so.get()));
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
        // Keys reach the slide (navigation, Tab through objects, Delete)
        // only when it has focus: a click gives it, and so does Tab from
        // the panes around it.
        canvas.set_focusable(true);
        canvas.set_focus_on_click(true);
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
            // Headless `--export-pdf <out>` (docs/EXPORT-PARITY-SPEC.md item
            // 1): the presented slides as a Cairo PDF at the model slide's
            // size — the canvas path, not the Typst one. main.rs schedules
            // this after the window settles.
            let ctl = controller.clone();
            let act = gio::SimpleAction::new("test-export-pdf", None);
            act.connect_activate(move |_, _| {
                let Some(out) = std::env::var_os(suite_common::render_dump::EXPORT_PDF_ENV) else { return };
                let slides = ctl.slides.borrow();
                let masters = ctl.masters.borrow();
                if let Err(e) = crate::canvas::render_slides_pdf(
                    &slides,
                    &masters,
                    std::path::Path::new(&out),
                ) {
                    eprintln!("export-pdf: {e}");
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

        // Speaker notes under the slide (notes_pane.rs), the divider
        // between them dragged to size the pane; extra height goes to the
        // slide.
        let notes = crate::notes_pane::build();
        let notes_buffer = notes.buffer.clone();
        let slide_and_notes = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(&canvas_overlay)
            .end_child(&notes.widget)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();

        let editor_split = adw::OverlaySplitView::new();
        editor_split.set_sidebar_position(gtk::PackType::End);
        editor_split.set_content(Some(&slide_and_notes));
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
        let dup_btn = gtk::Button::builder()
            .icon_name("edit-copy-symbolic").tooltip_text("Duplicate slide").build();
        // slide_actions.rs does the work, so the shortcuts and the palette
        // reach the same commands.
        up_btn.set_action_name(Some("app.move-slide-up"));
        down_btn.set_action_name(Some("app.move-slide-down"));
        dup_btn.set_action_name(Some("app.duplicate-slide"));
        for (btn, name) in [(&up_btn, "Move slide up"), (&down_btn, "Move slide down"), (&dup_btn, "Duplicate slide")] {
            btn.update_property(&[gtk::accessible::Property::Label(name)]);
        }

        for btn in [&add_btn, &del_btn, &dup_btn, &up_btn, &down_btn] {
            btn.add_css_class("flat");
            btn.set_has_frame(true);
            btn.set_size_request(36, 36);
        }

        sidebar_controls.append(&add_btn);
        sidebar_controls.append(&del_btn);
        sidebar_controls.append(&dup_btn);
        sidebar_controls.append(&up_btn);
        sidebar_controls.append(&down_btn);

        let sidebar_scroll = gtk::ScrolledWindow::new();
        sidebar_scroll.set_child(Some(&slide_list));
        sidebar_scroll.set_vexpand(true);
        // Thumbnails only for the rows in view, drawn from the live deck.
        let (s, m) = (slides.clone(), masters.clone());
        crate::sidebar::set_thumbnail_source(&slide_list, &sidebar_scroll, std::rc::Rc::new(move |i| {
            crate::sidebar::slide_thumbnail(&s.borrow(), &m.borrow(), i)
        }));
        rebuild_slide_list(&slide_list, &slides.borrow(), &masters.borrow(), 0);

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
        // A dropped picture goes on the current slide. Any other file opens
        // the way one from the file manager does, through
        // GApplication::open, which asks before replacing unsaved work
        // (#1316). It used to replace the deck without asking.
        {
            let ctl = controller.clone();
            let cs_ref = current_slide.clone();
            let cs = canvas.clone();
            let refresh = refresh_hud.clone();
            suite_common::open_files_on_drop(&suite_win.window, app, move |file| {
                let Some(path) = file.path() else { return false };
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "svg") {
                    return false;
                }
                let obj = SlideObject::Image {
                    path: path.to_string_lossy().into_owned(),
                    x: 150.0,
                    y: 100.0,
                    w: 300.0,
                    h: 200.0,
                    rotation: 0.0,
                    crop: Default::default(),
                };
                ctl.add_object(cs_ref.get(), obj);
                cs.queue_draw();
                refresh();
                true
            });
        }

        // ── Window close-request: Save / Discard / Cancel guard ──────────
        // Same force_close re-entrancy pattern as Letters/Tables: a dialog
        // response can't be awaited inside close-request, so the first
        // close is stopped and the dialog's own callback re-invokes
        // .close() with the guard set so this handler lets it through.
        {
            let dirty = dirty.clone();
            let m = controller.clone();
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
                        // Through the loss question, as Save is (#1273):
                        // the close guard's Save wrote straight over a file
                        // whose content the format can't keep.
                        let deck = m.deck();
                        let w = win.clone();
                        let target = path.clone();
                        crate::loss_ui::save_after_asking(&win, Some(&target), &target, move || match save_deck(&path, Some(&path), &deck) {
                            Ok(()) => {
                                dirty.set(false);
                                slot.clear_or_report();
                                force_close.set(true);
                                w.close();
                            }
                            Err(e) => {
                                let err = adw::AlertDialog::builder()
                                    .heading(suite_common::i18n("Error saving file"))
                                    .body(&e)
                                    .build();
                                err.add_response("ok", &suite_common::i18n("OK"));
                                err.present(Some(&w));
                            }
                        });
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
                            if let Some(path) = local_path(&file, true, &win2) {
                                let path_str = path.to_string_lossy().to_string();
                                let deck = m.deck();
                                let target = path_str.clone();
                                let w = win2.clone();
                                // A deck never saved has no source to lose
                                // content from; the question still decides,
                                // as Save As's does.
                                crate::loss_ui::save_after_asking(&win2, None, &target, move || match save_deck(&path_str, None, &deck) {
                                    Ok(()) => {
                                        *path_state.borrow_mut() = Some(path_str);
                                        dirty.set(false);
                                        slot.clear_or_report();
                                        force_close.set(true);
                                        w.close();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading(suite_common::i18n("Error saving file"))
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", &suite_common::i18n("OK"));
                                        err.present(Some(&w));
                                    }
                                });
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
        // The notes stay: hiding them left a narrow window no way to read
        // or write a slide's notes at all, and the pane is a divider the
        // user drags, so it costs the slide only what they give it.

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_box.append(&split_view);
        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&main_box));
        suite_win.set_content(&toast_overlay);
        let autosave_notices = suite_common::autosave_notice::AutosaveNotifier::new(&toast_overlay);

        let toolbar = build_decks_toolbar();
        suite_win.add_top_bar(&toolbar);
        // Export as PDF, handouts and PNG, and their menu section (export_ui.rs).
        crate::export_ui::register(app, &suite_win.window, &suite_win.header_bar, &controller, &current_slide);
        let handles = crate::canvas_keys::EditorHandles {
            window: &suite_win.window,
            canvas: &canvas,
            slide_list: &slide_list,
            slides: &slides,
            masters: &masters,
            current_slide: &current_slide,
            selected_object: &selected_object,
            controller: &controller,
            refresh_hud: &refresh_hud,
            transition: &transition,
        };
        // Edit Master and its banner (master_view.rs).
        crate::master_view::register(app, |banner| suite_win.add_top_bar(banner), handles);
        // Duplicate and move slides (slide_actions.rs).
        crate::slide_actions::register(app, handles);
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

        // Back to the slide from the notes or the inspector (the palette's
        // and automation's way; a click or Tab does it too).
        {
            let canvas = canvas.clone();
            let act = gio::SimpleAction::new("focus-slide", None);
            act.connect_activate(move |_, _| {
                canvas.grab_focus();
            });
            app.add_action(&act);
            suite_common::actions::register_labels(&[("app.focus-slide", "Go to Slide")]);
        }

        // Go to Speaker Notes (Google Slides' Ctrl+Alt+Shift+S): the
        // keyboard's way into the pane.
        {
            let view = notes.view.clone();
            let act = gio::SimpleAction::new("focus-notes", None);
            act.connect_activate(move |_, _| {
                view.grab_focus();
            });
            app.add_action(&act);
            app.set_accels_for_action("app.focus-notes", &["<Primary><Alt><Shift>s"]);
        }

        // Save speaker notes on text change, through the controller: an
        // undoable edit (a word per step) that marks the deck changed.
        {
            let controller = controller.clone();
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
                let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
                controller.set_notes(cs_ref.get(), &text);
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
            ids: Default::default(),
            layout: None,
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
            let so = selected_object.clone();
            let masters = masters.clone();
            let refresh = refresh_hud.clone();
            let act = gio::SimpleAction::new(name, None);
            act.connect_activate(move |_, _| {
                let changed = if is_undo { controller.undo() } else { controller.redo() };
                if changed {
                    // An undone insert can take the current slide or the
                    // selected object away with it.
                    let (slide, object) = controller.repair_selection(cs_ref.get(), so.get());
                    cs_ref.set(slide);
                    so.set(object);
                    cs.queue_draw();
                    let snapshot = ss.borrow().clone();
                    rebuild_slide_list(&sl, &snapshot, &masters.borrow(), cs_ref.get());
                    // The inspector shows the undone state too.
                    refresh();
                }
            });
            app.add_action(&act);
        }
        // Wherever the focus is: without these, Ctrl+Z reached the deck only
        // while the canvas had focus, so an object inserted from the palette
        // or the Insert menu could not be undone from the keyboard (#1208).
        // Text being edited (notes, a text box) handles the keys first and
        // keeps its own undo, as in Letters and Tables.
        app.set_accels_for_action("app.undo", &["<Primary>z"]);
        app.set_accels_for_action("app.redo", &["<Primary>y", "<Primary><Shift>z"]);

        // Insert buttons in the header bar (insert_bar.rs): a shape from the
        // library by its index, or a 3x3 table; each selected once in.
        {
            let (cs, cs_ref, controller, refresh, so) =
                (canvas.clone(), current_slide.clone(), controller.clone(), refresh_hud.clone(), selected_object.clone());
            let insert = move |obj: SlideObject| {
                let idx = cs_ref.get();
                controller.add_object(idx, obj);
                so.set(controller.slides.borrow().get(idx).map(|s| s.objects.len().saturating_sub(1)));
                cs.queue_draw();
                refresh();
            };
            let insert = Rc::new(insert);
            let act = gio::SimpleAction::new("insert-shape", Some(glib::VariantTy::UINT32));
            let ins = insert.clone();
            act.connect_activate(move |_, p| {
                let lib = decks_core::insert::shape_library();
                if let Some(s) = p.and_then(|p| p.get::<u32>()).and_then(|i| lib.get(i as usize)) {
                    ins(decks_core::insert::shape(s.kind.clone()));
                }
            });
            app.add_action(&act);
            let act = gio::SimpleAction::new("insert-table", None);
            let ins = insert.clone();
            act.connect_activate(move |_, _| ins(decks_core::insert::table(3, 3)));
            app.add_action(&act);
            // A chart of the kind at the target's index in CHART_KINDS.
            let act = gio::SimpleAction::new("insert-chart", Some(glib::VariantTy::UINT32));
            act.connect_activate(move |_, p| {
                if let Some(kind) = p.and_then(|p| p.get::<u32>()).and_then(|i| decks_core::insert::CHART_KINDS.get(i as usize)) {
                    insert(decks_core::insert::chart(*kind));
                }
            });
            app.add_action(&act);
            crate::insert_bar::build(&suite_win.header_bar, &suite_win.narrow_breakpoint);
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
                crate::file_pick::open(&dlg, &w,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Ok(path) = suite_common::locations::open_location(&file).map_err(|e| eprintln!("{e}")) {
                                let idx = cs_ref.get();
                                let p = path.to_string_lossy().to_string();
                                let obj = SlideObject::Image {
                                    path: p, x: 200.0, y: 200.0, w: 200.0, h: 150.0, rotation: 0.0, crop: Default::default()
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
                let (m, cs_ref, app2) = (controller.clone(), current_slide.clone(), app.clone());
                let act = gio::SimpleAction::new(name, None);
                act.connect_activate(move |_, _| {
                    let deck = m.deck();
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

        // Keyboard, clipboard and in-place text editing: canvas_keys.rs.
        crate::canvas_keys::register(crate::canvas_keys::EditorHandles {
            window: &suite_win.window,
            canvas: &canvas,
            slide_list: &slide_list,
            slides: &slides,
            masters: &masters,
            current_slide: &current_slide,
            selected_object: &selected_object,
            controller: &controller,
            refresh_hud: &refresh_hud,
            transition: &transition,
        });

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

        // ── App actions ──────────────────────────────────────────────────
        {
            let cs = content_stack.clone();
            let sl = slide_list.clone();
            let ss = slides.clone();
            let cs_scroll = editor_split.clone();
            let path_ref = file_path.clone();
            let refresh = refresh_hud.clone();
            let masters = masters.clone();
            let w = suite_win.window.clone();
            let dirty = dirty.clone();
            let act = gtk::gio::SimpleAction::new("new-document", None);
            act.connect_activate(move |_, _| {
                let (cs, cs_scroll, ss, path_ref, sl, masters, refresh, dirty) = (
                    cs.clone(), cs_scroll.clone(), ss.clone(), path_ref.clone(),
                    sl.clone(), masters.clone(), refresh.clone(), dirty.clone(),
                );
                suite_common::confirm_discarding(&w, dirty.get(), "presentation", move || {
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
                        ids: Default::default(),
                        layout: None,
                    }];
                }
                *path_ref.borrow_mut() = None;
                dirty.set(false);
                rebuild_slide_list(&sl, &ss.borrow().clone(), &masters.borrow(), 0);
                cs.queue_draw();
                refresh();
                });
            });
            app.add_action(&act);
        }

        // New from Template (template_chooser.rs), in place of the suite's
        // generic template dialog: the chosen theme's deck and master.
        {
            let (cs, sl, ss, ms) = (content_stack.clone(), slide_list.clone(), slides.clone(), masters.clone());
            let (cs_scroll, path_ref, refresh) = (editor_split.clone(), file_path.clone(), refresh_hud.clone());
            let (cs_ref, so, w) = (current_slide.clone(), selected_object.clone(), suite_win.window.clone());
            let dirty = dirty.clone();
            let act = gtk::gio::SimpleAction::new("new-from-template", None);
            act.connect_activate(move |_, _| {
                let (cs, sl, ss, ms) = (cs.clone(), sl.clone(), ss.clone(), ms.clone());
                let (cs_scroll, path_ref, refresh, cs_ref, so) =
                    (cs_scroll.clone(), path_ref.clone(), refresh.clone(), cs_ref.clone(), so.clone());
                let (w2, dirty) = (w.clone(), dirty.clone());
                crate::template_chooser::present(&w, move |index| {
                    let Some((slides, masters)) = decks_core::templates::deck(index) else { return };
                    let (cs, sl, ss, ms) = (cs.clone(), sl.clone(), ss.clone(), ms.clone());
                    let (cs_scroll, path_ref, refresh, cs_ref, so, dirty) =
                        (cs_scroll.clone(), path_ref.clone(), refresh.clone(), cs_ref.clone(), so.clone(), dirty.clone());
                    suite_common::confirm_discarding(&w2, dirty.get(), "presentation", move || {
                    if cs.child_by_name("editor").is_none() {
                        cs.add_titled(&cs_scroll, Some("editor"), "Editor");
                    }
                    cs.set_visible_child_name("editor");
                    *ss.borrow_mut() = slides;
                    *ms.borrow_mut() = masters;
                    *path_ref.borrow_mut() = None;
                    dirty.set(false);
                    cs_ref.set(0);
                    so.set(None);
                    rebuild_slide_list(&sl, &ss.borrow().clone(), &ms.borrow(), 0);
                    cs.queue_draw();
                    refresh();
                    });
                });
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
            let dirty = dirty.clone();

            let act = gtk::gio::SimpleAction::new("open-file", None);
            act.connect_activate(move |_, _| {
                let (cs, sl, ss, cs_ref, so, da, w, cs_scroll, path_ref, masters, dirty) = (
                    cs.clone(), sl.clone(), ss.clone(), cs_ref.clone(), so.clone(), da.clone(),
                    w.clone(), cs_scroll.clone(), path_ref.clone(), masters.clone(), dirty.clone(),
                );
                let parent = w.clone();
                suite_common::confirm_discarding(&parent, dirty.get(), "presentation", move || {
                let dlg = gtk::FileDialog::new();
                dlg.set_filters(Some(&suite_common::file_dialogs::open_filters("Presentations (.pptx, .odp)", decks_core::FORMATS)));

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
                let dirty = dirty.clone();

                crate::file_pick::open(&dlg, &w,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = local_path(&file, false, &w2) {
                                let path_str = path.to_string_lossy().to_string();
                                match read_deck(&path_str) {
                                    Ok(deck) => {
                                        *ss.borrow_mut() = deck.slides;
                                        *masters.borrow_mut() = deck.masters;
                                        cs_ref.set(0);
                                        so.set(None);
                                        *path_ref.borrow_mut() = Some(path_str);
                                        dirty.set(false);
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
            });
            app.add_action(&act);
        }

        // Save actions
        {
            let w = suite_win.window.clone();
            let path_ref = file_path.clone();

            let act_save = gtk::gio::SimpleAction::new("save-file", None);
            let w_clone = w.clone();
            let path_clone = path_ref.clone();
            let m_save = controller.clone();
            let dirty_save = dirty.clone();
            let slot_save = autosave_slot.clone();
            act_save.connect_activate(move |_, _| {
                let current_path = path_clone.borrow().clone();
                if let Some(path_str) = current_path {
                    let deck = m_save.deck();
                    let (w_clone, dirty_save, slot_save) = (w_clone.clone(), dirty_save.clone(), slot_save.clone());
                    let target = path_str.clone();
                    crate::loss_ui::save_after_asking(&w_clone.clone(), Some(&target), &target, move || match save_deck(&path_str, Some(&path_str), &deck) {
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
                    });
                } else {
                    let _ = gtk4::prelude::WidgetExt::activate_action(&w_clone, "app.save-file-as", None);
                }
            });
            app.add_action(&act_save);

            let act_save_as = gtk::gio::SimpleAction::new("save-file-as", None);
            let m_as = controller.clone();
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

                let w2 = w.clone();
                let path_ref = path_ref.clone();
                let m_inner = m_as.clone();
                let dirty_as = dirty_as.clone();
                let slot_as = slot_as.clone();
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = local_path(&file, true, &w2) {
                                let path_str = path.to_string_lossy().to_string();
                                let deck = m_inner.deck();
                                let source = path_ref.borrow().clone();
                                let (w2, path_ref, dirty_as, slot_as) = (w2.clone(), path_ref.clone(), dirty_as.clone(), slot_as.clone());
                                let target = path_str.clone();
                                let source_for_carry = source.clone();
                                crate::loss_ui::save_after_asking(&w2.clone(), source.as_deref(), &target, move || match save_deck(&path_str, source_for_carry.as_deref(), &deck) {
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
                                });
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
            let m = controller.clone();
            let dirty = dirty.clone();
            let slot = autosave_slot.clone();
            let notices = autosave_notices.clone();
            let path_state = file_path.clone();
            let act = gtk::gio::SimpleAction::new("autosave-now", None);
            act.connect_activate(move |_, _| {
                if !dirty.get() {
                    return;
                }
                let deck = m.deck();
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
            let m = controller.clone();
            let dirty = dirty.clone();
            let slot = autosave_slot.clone();
            let notices = autosave_notices.clone();
            let path_state = file_path.clone();
            let interval = settings.int("auto-save-interval").max(10) as u32;
            let enabled = settings.boolean("auto-save");
            if enabled {
                glib::source::timeout_add_seconds_local(interval, move || {
                    if dirty.get() {
                        let deck = m.deck();
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
            let tmp = suite_common::autosave::recovery_scratch_path(&state_dir, ext);
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
    /// Whether the deck has changes a replacement would lose.
    pub fn is_dirty(&self) -> bool {
        self.controller.dirty.get()
    }

    /// A new deck nobody has touched: a file opened while it is in front
    /// takes its window rather than a new one.
    pub fn is_pristine(&self) -> bool {
        self.file_path.borrow().is_none() && !self.is_dirty()
    }

    pub fn open_path(&self, path: &str) -> Result<(), String> {
        let deck = read_deck(path)?;
        // The deck on screen becomes the file just read. (Recovery marks it
        // dirty again afterwards: recovered work is unsaved.)
        self.controller.dirty.set(false);
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

/// Write `deck` to `path`, then upload it if `path` is the staged copy of
/// a remote location (RFC-0003). Every save goes through here. `source`
/// is the file the deck came from: its safe unmodelled parts (a thumbnail,
/// custom XML) are carried into the saved file, read before the write
/// replaces it (#1274, `suite_common::carry`).
fn save_deck(path: &str, source: Option<&str>, deck: &Deck) -> Result<(), String> {
    let carried = source.map(|s| suite_common::carry::capture(std::path::Path::new(s))).unwrap_or_default();
    carried.write_with(std::path::Path::new(path), |p| {
        write_deck(p.to_str().ok_or_else(|| "save path is not UTF-8".to_string())?, deck)
    })?;
    suite_common::locations::commit_save(std::path::Path::new(path))
}

/// The local path for a location a dialog handed over: its own, or a
/// staged copy of a remote one (RFC-0003). One that can't be used is
/// reported, never silently ignored.
fn local_path(file: &gio::File, for_save: bool, parent: &adw::ApplicationWindow) -> Option<std::path::PathBuf> {
    let staged = if for_save {
        suite_common::locations::save_location(file)
    } else {
        suite_common::locations::open_location(file)
    };
    staged
        .map_err(|e| {
            let heading = if for_save { "Error saving presentation" } else { "Error opening presentation" };
            suite_common::show_error_dialog(Some(parent), &suite_common::i18n(heading), &e);
        })
        .ok()
}
