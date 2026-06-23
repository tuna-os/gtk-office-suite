// SPDX-License-Identifier: GPL-3.0-or-later
//
// LettersWindow — Tabbed word processor following gnome-gui-spec AdwTabView pattern.

use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::{AlertDialogExt, AlertDialogExtManual, AdwDialogExt};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::page_container::PageContainer;

// ── Per-tab state via widget Qdata ─────────────────────────────────────

#[derive(Clone)]
struct TabData(Rc<RefCell<TabDataInner>>);
struct TabDataInner { file: Option<PathBuf>, closing_after_save: bool }
impl TabData {
    fn new() -> Self { TabData(Rc::new(RefCell::new(TabDataInner { file: None, closing_after_save: false }))) }
}
fn tab_data_set(w: &impl IsA<gtk::Widget>, d: TabData) { unsafe { w.upcast_ref::<gtk::Widget>().set_data("tab-data", d); } }
fn tab_data_get(w: &gtk::Widget) -> Option<TabData> { unsafe { w.data::<TabData>("tab-data").map(|p| p.as_ref().clone()) } }

// ── Make a tab's document widget ──────────────────────────────────────

fn make_doc_widget(settings: Option<&gio::Settings>) -> (PageContainer, gtk::TextBuffer) {
    let buffer = gtk::TextBuffer::new(None);
    register_formatting_tags(&buffer);
    let editor = gtk::TextView::with_buffer(&buffer);
    connect_list_continuation(&editor, &buffer);
    connect_markdown_macros(&buffer);
    editor.set_wrap_mode(gtk::WrapMode::Word);
    editor.set_left_margin(24); editor.set_right_margin(24);
    editor.set_top_margin(16); editor.set_bottom_margin(16);
    editor.set_vexpand(true); editor.set_hexpand(true);
    // Spell-check via zspell (hunspell-compatible, pure Rust).
    // Applies red wavy underline to misspelled words, re-checks on edits.
    let spell_enabled = settings.map(|s| s.boolean("spell-check-enabled")).unwrap_or(true);
    if spell_enabled {
        crate::spell::SpellChecker::new(&buffer).start();
    }
    // Drag-and-drop for images from file manager
    {
        let buf = buffer.clone();
        let drop = gtk::DropTarget::new(gio::File::static_type(), gtk4::gdk::DragAction::COPY);
        drop.connect_drop(move |_target, value, _x, _y| {
            if let Ok(file) = value.get::<gio::File>() {
                if let Some(path) = file.path() {
                    let name = path.file_name()
                        .and_then(|n| n.to_str()).unwrap_or("image");
                    let path_str = path.to_string_lossy();
                    let md = format!("![{}]({})", name, path_str);
                    let ins = buf.selection_bounds()
                        .map(|(i,_)| i).unwrap_or_else(|| buf.start_iter());
                    let mut pos = ins;
                    buf.insert(&mut pos, &md);
                }
            }
            true
        });
        editor.add_controller(drop);
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&editor));
    scroll.set_vexpand(true); scroll.set_hexpand(true);
    let container = PageContainer::new();
    if let Some(s) = settings {
        container.load_from_settings(s);
    }
    scroll.set_parent(&container);
    container.set_vexpand(true); container.set_hexpand(true);
    (container, buffer)
}

// ── LettersWindow ───────────────────────────────────────────────────────

pub struct LettersWindow {
    pub window: adw::ApplicationWindow,
    tab_view: adw::TabView,
    stack: gtk4::Stack,
    word_count_label: gtk4::Label,
    settings: gio::Settings,
}

impl LettersWindow {
    pub fn new(app: &adw::Application, settings: gio::Settings) -> Self {
        let tab_view = adw::TabView::new();
        tab_view.set_menu_model(Some(&make_tab_menu()));
        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&tab_view));

        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        stack.set_transition_duration(200);
        let empty_page = suite_common::make_empty_state(
            "Letters",
            "Create a new document (Ctrl+N), or open an existing one (Ctrl+O)",
            "accessories-text-editor-symbolic",
            "Open File\u{2026}",
        );
        stack.add_titled(&empty_page, Some("empty"), "Empty");
        stack.add_titled(&tab_view, Some("editor"), "Editor");
        stack.set_visible_child_name("empty");

        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&stack));
        let (status_bar, word_count_label) = suite_common::make_status_bar();

        let app_clone = app.clone();
        let primary_toolbar: Vec<(&'static str, &'static str, Box<dyn Fn(bool) + 'static>)> = vec![
            ("format-text-bold-symbolic", "Bold (<Control>b)", Box::new({
                let app = app_clone.clone();
                move |_| { app.activate_action("bold", None); }
            })),
            ("format-text-italic-symbolic", "Italic (<Control>i)", Box::new({
                let app = app_clone.clone();
                move |_| { app.activate_action("italic", None); }
            })),
            ("format-text-underline-symbolic", "Underline (<Control>u)", Box::new({
                let app = app_clone.clone();
                move |_| { app.activate_action("underline", None); }
            })),
        ];

        let app_clone = app.clone();
        let extended_toolbar: Vec<(&'static str, &'static str, Box<dyn Fn() + 'static>)> = vec![
            ("format-text-strikethrough-symbolic", "Strikethrough (<Control><Shift>s)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("strikethrough", None); }
            })),
            ("format-text-highlight-symbolic", "Highlight (<Control><Shift>h)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("highlight", None); }
            })),
            ("format-list-bullet-symbolic", "Bullet List (<Control><Shift>8)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("bullet-list", None); }
            })),
            ("format-list-ordered-symbolic", "Numbered List (<Control><Shift>7)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("numbered-list", None); }
            })),
            ("format-justify-left-symbolic", "Align Left (<Control>l)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("align-left", None); }
            })),
            ("format-justify-center-symbolic", "Align Center (<Control>e)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("align-center", None); }
            })),
            ("format-justify-right-symbolic", "Align Right (<Control>r)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("align-right", None); }
            })),
            ("format-justify-fill-symbolic", "Justify (<Control>j)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("align-justify", None); }
            })),
            ("insert-link-symbolic", "Insert Link (<Control>k)", Box::new({
                let app = app_clone.clone();
                move || { app.activate_action("insertlink", None); }
            })),
        ];

        let suite_win = suite_common::SuiteWindow::new(app, "Letters", primary_toolbar, extended_toolbar);
        suite_win.add_top_bar(&tab_bar);
        suite_win.set_content(&toast_overlay);
        suite_win.add_bottom_bar(&status_bar);

        // ── Ruler ──────────────────────────────────────────────────
        let ruler_widget = crate::ruler::Ruler::new();
        ruler_widget.load_from_settings(&settings);
        {
            let s = settings.clone();
            let rw = ruler_widget.downgrade();
            ruler_widget.connect_changed(move || {
                if let Some(r) = rw.upgrade() {
                    let _ = s.set_double("page-margin-left", r.margin_left());
                    let _ = s.set_double("page-margin-right", r.margin_right());
                }
            });
        }
        suite_win.add_top_bar(&ruler_widget);

        // ── Style dropdown ────────────────────────────────────────
        let style_sheet = std::rc::Rc::new(std::cell::RefCell::new(
            crate::styles::StyleSheet::default_styles()
        ));
        let all_names: Vec<&str> = crate::styles::style_names();
        let style_model = gtk4::StringList::new(&all_names);
        let model = style_model.clone();
        let style_dropdown = gtk4::DropDown::new(Some(style_model), None::<&gtk4::Expression>);
        {
            let tv = tab_view.clone();
            let ss = style_sheet.clone();
            style_dropdown.connect_selected_notify(move |dd| {
                let idx = dd.selected();
                if idx != gtk4::INVALID_LIST_POSITION {
                    if let Some(obj) = model.item(idx) {
                        if let Ok(so) = obj.downcast::<gtk4::StringObject>() {
                            let name = so.string();
                            if let Some(buf) = active_buffer(&tv) {
                                if let Ok(sheet) = ss.try_borrow() {
                                    crate::styles::ensure_tags_synced(&sheet, &buf.tag_table());
                                    crate::styles::apply_style(&buf, &sheet, &name);
                                }
                            }
                        }
                    }
                }
            });
        }
        suite_win.add_top_bar(&style_dropdown);

        let win = suite_win.window.clone();

        // ── Tab: selected-page ──────────────────────────────────────
        let st = stack.clone();
        tab_view.connect_selected_page_notify(move |_| { st.set_visible_child_name("editor"); });

        // ── Tab: close-page with unsaved confirmation ───────────────
        {
            let tv = tab_view.clone();
            let st = stack.clone();
            tab_view.connect_close_page(move |tv, page| {
                if !page.needs_attention() {
                    tv.close_page_finish(page, true);
                    if tv.n_pages() == 0 { st.set_visible_child_name("empty"); }
                    return glib::Propagation::Stop;
                }
                let title = page.title().to_string();
                let tv2 = tv.clone();
                let st2 = st.clone();
                let child = page.child().clone();
                let body = format!("{}\nThis document has not been saved.", title);
                let dialog = adw::AlertDialog::new(Some("Save document?"), Some(body.as_str()));
                dialog.add_responses(&[("cancel", "_Cancel"), ("discard", "_Discard"), ("save", "_Save")]);
                dialog.set_close_response("cancel");
                dialog.set_default_response(Some("save"));
                dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
                dialog.set_response_appearance("cancel", adw::ResponseAppearance::Destructive);
                dialog.choose(None::<&gtk::Window>, None::<&gio::Cancellable>,
                    move |response: glib::GString| {
                        match response.as_str() {
                            "save" => {
                                if let Some(td) = tab_data_get(&child) { td.0.borrow_mut().closing_after_save = true; }
                                do_save(&tv2, &st2);
                            }
                            "discard" => {
                                tv2.close_page_finish(&tv2.page(&child), true);
                                if tv2.n_pages() == 0 { st2.set_visible_child_name("empty"); }
                            }
                            _ => { tv2.close_page_finish(&tv2.page(&child), false); }
                        }
                    },
                );
                glib::Propagation::Stop
            });
        }

        // ── Tab: create-window (drag to new window) ────────────────
        {
            let app_weak = app.downgrade();
            let s = settings.clone();
            tab_view.connect_create_window(move |_| {
                app_weak.upgrade().map(|app| {
                    let nw = LettersWindow::new(&app, s.clone());
                    nw.present();
                    nw.tab_view
                })
            });
        }

        // ── Window: close-request with dirty check ──────────────────
        {
            let tv = tab_view.clone();
            let force_close = std::rc::Rc::new(std::cell::Cell::new(false));
            win.connect_close_request(move |win| {
                if force_close.get() {
                    return glib::Propagation::Proceed;
                }
                let n = tv.n_pages();
                let mut dirty: Vec<String> = Vec::new();
                for i in 0..n {
                    let p = tv.nth_page(i);
                    if p.needs_attention() { dirty.push(p.title().to_string()); }
                }
                if dirty.is_empty() { return glib::Propagation::Proceed; }
                let body = format!(
                    "The following documents have unsaved changes:\n• {}\nAll unsaved changes will be discarded if you close Letters now.",
                    dirty.join("\n• ")
                );
                let dialog = adw::AlertDialog::new(Some("Unsaved changes"), Some(body.as_str()));
                dialog.add_responses(&[("cancel", "_Cancel"), ("discard", "_Discard All")]);
                dialog.set_close_response("cancel");
                dialog.set_default_response(Some("cancel"));
                dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                let win_weak = win.downgrade();
                let force_close_clone = force_close.clone();
                dialog.choose(Some(win), None::<&gio::Cancellable>,
                    move |response: glib::GString| {
                        if response == "discard" {
                            force_close_clone.set(true);
                            if let Some(w) = win_weak.upgrade() { w.close(); }
                        }
                    },
                );
                glib::Propagation::Stop
            });
        }

        // ── Find/Replace revealer ──────────────────────────────────
        let (find_revealer, find_entry) = make_find_replace_widget(&tab_view);
        suite_win.add_top_bar(&find_revealer);

        // ── Find/Replace actions ────────────────────────────────────
        let fe = find_entry.clone();
        let fr = find_revealer.clone();
        let a = gtk::gio::SimpleAction::new("find", None);
        a.connect_activate(move |_, _| {
            fr.set_reveal_child(true);
            fe.grab_focus();
        });
        app.add_action(&a);
        app.set_accels_for_action("app.find", &["<Primary>f"]);

        // ── Page Setup action ─────────────────────────────────────
        {
            let w = win.clone();
            let s = settings.clone();
            let tv = tab_view.clone();
            let a = gtk::gio::SimpleAction::new("page-setup", None);
            a.connect_activate(move |_, _| {
                let dialog = gtk::PageSetupUnixDialog::new(Some("Page Setup"), Some(&w));
                // Load current page setup from GSettings
                if let Some(ps) = load_page_setup_from_settings(&s) {
                    dialog.set_page_setup(&ps);
                }
                let s2 = s.clone();
                let tv2 = tv.clone();
                dialog.connect_response(move |dlg, _response| {
                    let ps = dlg.page_setup();
                    save_page_setup_to_settings(&s2, &ps);
                    for i in 0..tv2.n_pages() {
                        let page = tv2.nth_page(i);
                        if let Some(pc) = page.child().first_child()
                            .and_then(|c| c.downcast::<crate::page_container::PageContainer>().ok())
                        {
                            pc.reload_settings(&s2);
                        }
                    }
                    dlg.close();
                });
                dialog.show();
            });
            app.add_action(&a);
            app.set_accels_for_action("app.page-setup", &["<Primary><Shift>p"]);
        }

        // ── Actions ────────────────────────────────────────────────
        Self::register_actions(&tab_view, &stack, &word_count_label, &win, app, &settings);

        // ── Print action ──────────────────────────────────────────
        {
            let tv = tab_view.clone();
            let w = win.clone();
            let s = settings.clone();
            let a = gtk::gio::SimpleAction::new("print", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    let op = gtk::PrintOperation::new();
                    let buf_clone = buf.clone();
                    let s2 = s.clone();
                    op.connect_draw_page(move |_op, ctx, _nth| {
                        let cr = ctx.cairo_context();
                        let text = buf_clone.text(&buf_clone.start_iter(), &buf_clone.end_iter(), false);
                        let layout = pangocairo::functions::create_layout(&cr);
                        layout.set_text(&text);
                        // Read page size and margins from GSettings
                        let pw = s2.double("page-width-pt");
                        let ml = s2.double("page-margin-left");
                        let mr = s2.double("page-margin-right");
                        let mt = s2.double("page-margin-top");
                        let content_w = (pw - ml - mr).max(10.0);
                        layout.set_width((content_w * (pango::SCALE as f64)) as i32);
                        cr.move_to(ml, mt);
                        pangocairo::functions::show_layout(&cr, &layout);
                    });
                    op.set_export_filename("output.pdf");
                    let _ = op.run(gtk::PrintOperationAction::PrintDialog, Some(&w));
                }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.print", &["<Primary>p"]);
        }

        // ── Formatting actions ────────────────────────────────────
        Self::register_formatting_actions(&tab_view, app);

        // Undo/Redo (GtkTextBuffer built-in)
        {
            let tv = tab_view.clone();
            let a = gtk::gio::SimpleAction::new("undo", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) { buf.undo(); }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.undo", &["<Primary>z"]);
        }
        {
            let tv = tab_view.clone();
            let a = gtk::gio::SimpleAction::new("redo", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) { buf.redo(); }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.redo", &["<Primary>y", "<Primary><Shift>z"]);
        }

        // ── Insert actions ────────────────────────────────────────────
        // Insert Image
        {
            let tv = tab_view.clone();
            let w = win.clone();
            let a = gtk::gio::SimpleAction::new("insertimage", None);
            a.connect_activate(move |_, _| {
                let tv = tv.clone();
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_mime_type("image/*");
                f.set_name(Some("Images"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                if let Some(buf) = active_buffer(&tv) {
                                    let path_str = path.to_string_lossy();
                                    let name = path.file_name()
                                        .and_then(|n| n.to_str()).unwrap_or("image");
                                    let md = format!("![{}]({})", name, path_str);
                                    let ins = buf.selection_bounds()
                                        .map(|(i,_)| i).unwrap_or_else(|| buf.start_iter());
                                    let mut pos = ins;
                                    buf.insert(&mut pos, &md);
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&a);
        }

        // Insert Link
        {
            let tv = tab_view.clone();
            let a = gtk::gio::SimpleAction::new("insertlink", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    let sel = buf.selection_bounds();
                    let selected_text = sel.as_ref()
                        .map(|(s,e)| buf.text(s, e, false).to_string())
                        .unwrap_or_default();
                    let placeholder = if selected_text.is_empty() { "url" } else { &selected_text };
                    let md = format!("[{}]({})", selected_text, placeholder);
                    if let Some((start, end)) = sel {
                        buf.delete(&mut start.clone(), &mut end.clone());
                    }
                    let ins = buf.selection_bounds()
                        .map(|(i,_)| i).unwrap_or_else(|| buf.start_iter());
                    let mut pos = ins;
                    buf.insert(&mut pos, &md);
                }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.insertlink", &["<Primary>k"]);
        }

        // Insert Table
        {
            let tv = tab_view.clone();
            let a = gtk::gio::SimpleAction::new("insert-table", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    let rows = 3;
                    let cols = 3;
                    let mut md = String::new();
                    // Header row
                    md.push('|');
                    for c in 0..cols { md.push_str(&format!(" Header {} |", c+1)); }
                    md.push('\n');
                    // Separator
                    md.push('|');
                    for _ in 0..cols { md.push_str(" --- |"); }
                    md.push('\n');
                    // Data rows
                    for r in 0..rows {
                        md.push('|');
                        for c in 0..cols { md.push_str(&format!(" Cell {}.{} |", r+1, c+1)); }
                        md.push('\n');
                    }
                    let ins = buf.selection_bounds()
                        .map(|(i,_)| i).unwrap_or_else(|| buf.start_iter());
                    let mut pos = ins;
                    buf.insert(&mut pos, &md);
                }
            });
            app.add_action(&a);
        }

        // ── Page layout setting listeners ────────────────────────
        {
            let r = ruler_widget.clone();
            let tv = tab_view.clone();
            let s = settings.clone();
            let keys: &[&str] = &["page-width-pt", "page-height-pt",
                "page-margin-top", "page-margin-bottom",
                "page-margin-left", "page-margin-right", "ruler-metric"];
            for key in keys {
                let r = r.clone();
                let tv = tv.clone();
                let s = s.clone();
                s.connect_changed(Some(key), move |settings, _k| {
                    r.set_page_width(settings.double("page-width-pt"));
                    r.set_margins(
                        settings.double("page-margin-left"),
                        settings.double("page-margin-right"),
                    );
                    r.set_indents(
                        settings.double("page-margin-left"),
                        settings.double("page-margin-left"),
                    );
                    r.set_metric(settings.boolean("ruler-metric"));
                    // Update all page containers too
                    for i in 0..tv.n_pages() {
                        let page = tv.nth_page(i);
                        if let Some(pc) = page.child().first_child()
                            .and_then(|c| c.downcast::<crate::page_container::PageContainer>().ok())
                        {
                            pc.reload_settings(settings);
                        }
                    }
                });
            }
        }

        // ── Spell-check setting listener ──────────────────────────
        // zspell runs per-buffer; toggle takes effect on new documents.
        {
            let s = settings.clone();
            s.connect_changed(Some("spell-check-enabled"), move |settings, _key| {
                let _enabled = settings.boolean("spell-check-enabled");
                // Existing documents keep their current state.
                // New documents will respect the setting when created.
            });
        }

        LettersWindow { window: suite_win.window, tab_view, stack, word_count_label, settings }
    }

    pub fn present(&self) { self.window.present(); }

    fn register_actions(tv: &adw::TabView, st: &gtk4::Stack, wc: &gtk4::Label, win: &adw::ApplicationWindow, app: &adw::Application, settings: &gio::Settings) {
        // New document
        {
            let tv = tv.clone(); let st = st.clone(); let s = settings.clone();
            let a = gtk::gio::SimpleAction::new("new-document", None);
            a.connect_activate(move |_, _| {
                let (container, buf) = make_doc_widget(Some(&s));
                let page = tv.append(&container);
                page.set_title("Untitled Document");
                page.set_needs_attention(false);
                st.set_visible_child_name("editor");
                tab_data_set(&container, TabData::new());
                let p = tv.page(&container);
                buf.connect_modified_changed(move |b| { p.set_needs_attention(b.is_modified()); });
            });
            app.add_action(&a);
        }

        // Open file
        {
            let tv = tv.clone(); let st = st.clone(); let w = win.clone(); let s = settings.clone();
            let a = gtk::gio::SimpleAction::new("open-file", None);
            a.connect_activate(move |_, _| {
                let tv = tv.clone(); let st = st.clone(); let w = w.clone(); let s = s.clone();
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.md"); f.add_pattern("*.txt"); f.add_pattern("*.html"); f.add_pattern("*.docx");
                f.set_name(Some("Documents"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            let path = file.path().unwrap_or_default();
                            let name = file.basename().map(|p| p.display().to_string()).unwrap_or_default();
                            let (container, buf) = make_doc_widget(Some(&s));
                            let is_docx = path.extension().and_then(|e| e.to_str()).map(|e| e == "docx").unwrap_or(false);
                            if is_docx {
                                // Use docx_bridge for formatting-preserving DOCX import
                                let path_str = path.to_string_lossy().to_string();
                                let _ = crate::docx_bridge::read_docx_to_buffer(&path_str, &buf);
                            } else {
                                let content = std::fs::read_to_string(&path).unwrap_or_default();
                                buf.set_text(&content);
                            }
                            let td = TabData::new();
                            td.0.borrow_mut().file = Some(path);
                            tab_data_set(&container, td);
                            let page = tv.append(&container);
                            page.set_title(&name);
                            page.set_tooltip(&name);
                            page.set_needs_attention(false);
                            st.set_visible_child_name("editor");
                            let p = tv.page(&container);
                            buf.connect_modified_changed(move |b| { p.set_needs_attention(b.is_modified()); });
                        }
                    },
                );
            });
            app.add_action(&a);
        }

        // Save
        {
            let tv = tv.clone(); let st = st.clone();
            let a = gtk::gio::SimpleAction::new("save-file", None);
            a.connect_activate(move |_, _| { do_save(&tv, &st); });
            app.add_action(&a);
        }

        // Save As
        {
            let tv = tv.clone(); let w = win.clone();
            let a = gtk::gio::SimpleAction::new("save-file-as", None);
            a.connect_activate(move |_, _| {
                let tv = tv.clone(); let w = w.clone();
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.md"); f.add_pattern("*.txt"); f.add_pattern("*.docx");
                f.set_name(Some("Documents"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                if let Some(page) = tv.selected_page() {
                                    let child = page.child();
                                    let buf = get_textview(&child)
                                        .map(|tv| tv.buffer());
                                    if let Some(buf) = buf {
                                        let path_str = path.to_string_lossy().to_string();
                                        let is_docx = path.extension().and_then(|e| e.to_str()).map(|e| e == "docx").unwrap_or(false);
                                        if is_docx {
                                            let _ = crate::docx_bridge::write_buffer_to_docx(&path_str, &buf);
                                        } else {
                                            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
                                            let doc = crate::engine::Document::from_text(&text);
                                            let _ = crate::engine::write(&path_str, &doc);
                                        }
                                    }
                                    page.set_needs_attention(false);
                                    if let Some(name) = file.basename() { page.set_title(&name.display().to_string()); }
                                    if let Some(path) = file.path() {
                                        if let Some(s) = path.to_str() { page.set_tooltip(s); }
                                    }
                                    if let Some(td) = tab_data_get(&child) {
                                        td.0.borrow_mut().file = file.path();
                                    }
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&a);
        }
    }
}

// ── Active buffer helper ─────────────────────────────────────────────

/// Walk from a TabPage child through PageContainer → ScrolledWindow → TextView.
fn get_textview(widget: &impl IsA<gtk::Widget>) -> Option<gtk::TextView> {
    widget.as_ref()
        .first_child()                                          // PageContainer
        .and_then(|pc| pc.first_child())                        // ScrolledWindow
        .and_then(|sw| sw.first_child())                        // TextView
        .and_then(|tv| tv.downcast::<gtk::TextView>().ok())
}

// ── Page setup helpers ────────────────────────────────────────────────

fn load_page_setup_from_settings(settings: &gio::Settings) -> Option<gtk::PageSetup> {
    let ps = gtk::PageSetup::new();
    let pw = settings.double("page-width-pt");
    let ph = settings.double("page-height-pt");
    let mt = settings.double("page-margin-top");
    let mb = settings.double("page-margin-bottom");
    let ml = settings.double("page-margin-left");
    let mr = settings.double("page-margin-right");
    if pw > 0.0 && ph > 0.0 {
        let paper_size = gtk::PaperSize::new_custom("custom", "Custom", pw, ph, gtk::Unit::Points);
        ps.set_paper_size_and_default_margins(&paper_size);
        ps.set_top_margin(mt, gtk::Unit::Points);
        ps.set_bottom_margin(mb, gtk::Unit::Points);
        ps.set_left_margin(ml, gtk::Unit::Points);
        ps.set_right_margin(mr, gtk::Unit::Points);
        Some(ps)
    } else {
        None
    }
}

fn save_page_setup_to_settings(settings: &gio::Settings, ps: &gtk::PageSetup) {
    let paper = ps.paper_size();
    let _ = settings.set_double("page-width-pt", paper.width(gtk::Unit::Points));
    let _ = settings.set_double("page-height-pt", paper.height(gtk::Unit::Points));
    let _ = settings.set_double("page-margin-top", ps.top_margin(gtk::Unit::Points));
    let _ = settings.set_double("page-margin-bottom", ps.bottom_margin(gtk::Unit::Points));
    let _ = settings.set_double("page-margin-left", ps.left_margin(gtk::Unit::Points));
    let _ = settings.set_double("page-margin-right", ps.right_margin(gtk::Unit::Points));
}

fn active_buffer(tv: &adw::TabView) -> Option<gtk::TextBuffer> {
    tv.selected_page()
        .and_then(|p| get_textview(&p.child()))
        .map(|tv| tv.buffer())
}

/// Apply a named GtkTextTag to the current selection or cursor position.
fn apply_tag_to_active(tv: &adw::TabView, tag_name: &str) {
    if let Some(buf) = active_buffer(tv) {
        if let Some(tag) = buf.tag_table().lookup(tag_name) {
            let sel = buf.selection_bounds();
            if let Some((start, end)) = sel {
                buf.apply_tag(&tag, &start, &end);
            }
        }
    }
}

/// Toggle a named GtkTextTag on the current selection.
fn toggle_tag(tv: &adw::TabView, tag_name: &str) {
    if let Some(buf) = active_buffer(tv) {
        if let Some(tag) = buf.tag_table().lookup(tag_name) {
            let sel = buf.selection_bounds();
            if let Some((start, end)) = sel {
                // Get all tags at the start of the selection and check if ours is present
                let tags_at_cursor = start.tags();
                let has = tags_at_cursor.iter().any(|t| t.name().as_deref() == Some(tag_name));
                if has {
                    buf.remove_tag(&tag, &start, &end);
                } else {
                    buf.apply_tag(&tag, &start, &end);
                }
            }
        }
    }
}

// ── Formatting action handlers ────────────────────────────────────────

fn toggle_inline_bold(tv: &adw::TabView)  { toggle_tag(tv, "bold"); }
fn toggle_inline_italic(tv: &adw::TabView) { toggle_tag(tv, "italic"); }
fn toggle_inline_underline(tv: &adw::TabView) { toggle_tag(tv, "underline"); }
fn toggle_inline_strikethrough(tv: &adw::TabView) { toggle_tag(tv, "strikethrough"); }
fn toggle_highlight(tv: &adw::TabView) { toggle_tag(tv, "highlight"); }

impl LettersWindow {
    fn register_formatting_actions(tv: &adw::TabView, app: &adw::Application) {
        // Inline formatting
        let pairs: &[(&str, fn(&adw::TabView))] = &[
            ("bold", toggle_inline_bold),
            ("italic", toggle_inline_italic),
            ("underline", toggle_inline_underline),
            ("strikethrough", toggle_inline_strikethrough),
            ("highlight", toggle_highlight),
        ];
        for (name, handler) in pairs {
            let tv = tv.clone();
            let a = gtk::gio::SimpleAction::new(name, None);
            a.connect_activate(move |_, _| handler(&tv));
            app.add_action(&a);
        }

        // Shortcuts for B/I/U
        app.set_accels_for_action("app.bold", &["<Primary>b"]);
        app.set_accels_for_action("app.italic", &["<Primary>i"]);
        app.set_accels_for_action("app.underline", &["<Primary>u"]);

        // Lists
        {
            let tv = tv.clone();
            let a = gtk::gio::SimpleAction::new("bullet-list", None);
            a.connect_activate(move |_, _| { toggle_list(&tv, "bullet"); });
            app.add_action(&a);
        }
        {
            let tv = tv.clone();
            let a = gtk::gio::SimpleAction::new("numbered-list", None);
            a.connect_activate(move |_, _| { toggle_list(&tv, "numbered"); });
            app.add_action(&a);
        }
        app.set_accels_for_action("app.bullet-list", &["<Primary><Shift>8"]);
        app.set_accels_for_action("app.numbered-list", &["<Primary><Shift>7"]);

        // Alignment
        let align_names: &[&str] = &["align-left", "align-center", "align-right", "align-justify"];
        for name in align_names {
            let tv = tv.clone();
            let a = gtk::gio::SimpleAction::new(name, None);
            let name = *name;
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    // Get cursor position from selection bounds
                    let bounds = buf.selection_bounds();
                    let (anchor, _) = bounds.unwrap_or_else(|| {
                        (buf.start_iter(), buf.start_iter())
                    });
                    let mut line_start = anchor.clone();
                    line_start.backward_line();
                    let mut line_end = anchor.clone();
                    line_end.forward_line();
                    // Remove all alignment tags from this line first
                    for an in &["align-left", "align-center", "align-right", "align-justify"] {
                        if let Some(at) = buf.tag_table().lookup(an) {
                            buf.remove_tag(&at, &line_start, &line_end);
                        }
                    }
                    // Apply the requested alignment
                    if let Some(tag) = buf.tag_table().lookup(name) {
                        buf.apply_tag(&tag, &line_start, &line_end);
                    }
                }
            });
            app.add_action(&a);
        }
        app.set_accels_for_action("app.align-left", &["<Primary>l"]);
        app.set_accels_for_action("app.align-center", &["<Primary>e"]);
        app.set_accels_for_action("app.align-right", &["<Primary>r"]);
        app.set_accels_for_action("app.align-justify", &["<Primary>j"]);

        // Font size
        {
            let tv = tv.clone();
            let a = gtk::gio::SimpleAction::new("increase-font", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    // Apply a larger scale tag
                    if let Some(tag) = buf.tag_table().lookup("font-larger") {
                        let sel = buf.selection_bounds();
                        if let Some((start, end)) = sel {
                            buf.apply_tag(&tag, &start, &end);
                        }
                    }
                }
            });
            app.add_action(&a);
        }
        {
            let tv = tv.clone();
            let a = gtk::gio::SimpleAction::new("decrease-font", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    if let Some(tag) = buf.tag_table().lookup("font-smaller") {
                        let sel = buf.selection_bounds();
                        if let Some((start, end)) = sel {
                            buf.apply_tag(&tag, &start, &end);
                        }
                    }
                }
            });
            app.add_action(&a);
        }
        app.set_accels_for_action("app.increase-font", &["<Primary><Shift>greater"]);
        app.set_accels_for_action("app.decrease-font", &["<Primary><Shift>less"]);

        // Styles
        let styles: &[(&str, &str)] = &[
            ("style_p", ""),
            ("style_h1", "h1"), ("style_h2", "h2"), ("style_h3", "h3"),
            ("style_h4", "h4"), ("style_h5", "h5"), ("style_h6", "h6"),
            ("style_code", "code"), ("style_quote", "blockquote"),
        ];
        for (action_name, tag_name) in styles {
            let tv = tv.clone();
            let a = gtk::gio::SimpleAction::new(action_name, None);
            let tag_name = *tag_name;
            a.connect_activate(move |_, _| {
                if !tag_name.is_empty() {
                    apply_tag_to_active(&tv, tag_name);
                }
            });
            app.add_action(&a);
        }
    }
}

// ── Save logic ───────────────────────────────────────────────────────

fn do_save(tv: &adw::TabView, _stack: &gtk4::Stack) {
    if let Some(page) = tv.selected_page() {
        if !page.needs_attention() { return; }
        let child = page.child();
        if let Some(td) = tab_data_get(&child) {
            let path = td.0.borrow().file.clone();
            if let Some(path) = path {
                let buf = get_textview(&child).map(|tv| tv.buffer());
                if let Some(buf) = buf {
                    let path_str = path.to_string_lossy().to_string();
                    let is_docx = path.extension().and_then(|e| e.to_str()).map(|e| e == "docx").unwrap_or(false);
                    if is_docx {
                        // Use docx_bridge for formatting-preserving DOCX save
                        // If saving to an existing file, preserve its styles
                        let _ = crate::docx_bridge::write_buffer_to_docx_preserving_styles(&path_str, &buf, &path_str);
                    } else {
                        let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
                        let doc = crate::engine::Document::from_text(&text);
                        let _ = crate::engine::write(&path_str, &doc);
                    }
                }
                page.set_needs_attention(false);
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    page.set_title(name);
                }
            }
        }
    }
}

// ── Tab context menu ─────────────────────────────────────────────────

fn make_tab_menu() -> gio::Menu {
    let m = gio::Menu::new();
    let s1 = gio::Menu::new();
    s1.append(Some("Move _Left"), Some("page.move-left"));
    s1.append(Some("Move _Right"), Some("page.move-right"));
    m.append_section(Some("Reorder"), &s1);
    let s2 = gio::Menu::new();
    s2.append(Some("_Move to New Window"), Some("page.move-to-new-window"));
    m.append_section(Some("Window"), &s2);
    let s3 = gio::Menu::new();
    s3.append(Some("Close _Other Tabs"), Some("win.close-other-pages"));
    s3.append(Some("_Close"), Some("win.close-current-page"));
    m.append_section(Some("Close"), &s3);
    m
}

// ── Find & Replace overlay ──────────────────────────────────────────

struct FindState {
    matches: Vec<(gtk::TextIter, gtk::TextIter)>,
    current: usize,
}

/// Build the find/replace revealer that slides down from the top.
fn make_find_replace_widget(tv: &adw::TabView) -> (gtk::Revealer, gtk::SearchEntry) {
    let tv = tv.clone();

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Find\u{2026}"));
    search_entry.set_hexpand(true);

    let replace_entry = gtk::Entry::new();
    replace_entry.set_placeholder_text(Some("Replace\u{2026}"));

    let match_label = gtk::Label::new(Some(""));
    match_label.add_css_class("dim-label");
    match_label.set_margin_start(4);
    match_label.set_margin_end(4);

    let find_prev = gtk::Button::new();
    find_prev.set_icon_name("go-up-symbolic");
    find_prev.set_tooltip_text(Some("Previous match (Shift+Enter)"));
    find_prev.add_css_class("flat");

    let find_next = gtk::Button::new();
    find_next.set_icon_name("go-down-symbolic");
    find_next.set_tooltip_text(Some("Next match (Enter)"));
    find_next.add_css_class("flat");

    let replace_btn = gtk::Button::with_label("Replace");
    replace_btn.set_tooltip_text(Some("Replace current match"));
    replace_btn.add_css_class("flat");

    let replace_all_btn = gtk::Button::with_label("All");
    replace_all_btn.set_tooltip_text(Some("Replace all matches"));
    replace_all_btn.add_css_class("flat");

    let case_toggle = gtk::ToggleButton::builder()
        .label("Aa")
        .tooltip_text("Case sensitive")
        .build();
    case_toggle.add_css_class("flat");

    let close_btn = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("Close (Escape)")
        .build();
    close_btn.add_css_class("flat");

    // Shared search state
    let state = Rc::new(RefCell::new(FindState { matches: Vec::new(), current: 0 }));

    // Shared widgets for closures
    let search_data = Rc::new((search_entry.clone(), match_label.clone(), case_toggle.clone()));

    // ── Helper: run search, populate matches, highlight ─────
    let run_search: Rc<Box<dyn Fn()>> = {
        let tv = tv.clone();
        let state = state.clone();
        let sd = search_data.clone();
        Rc::new(Box::new(move || {
            let query = sd.0.text().to_string();
            let ml = &sd.1;
            let ct = &sd.2;
            if query.is_empty() {
                ml.set_label("");
                state.borrow_mut().matches.clear();
                state.borrow_mut().current = 0;
                if let Some(buf) = active_buffer(&tv) {
                    for tag_name in &["search-match", "search-current"] {
                        if let Some(tag) = buf.tag_table().lookup(tag_name) {
                            buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
                        }
                    }
                }
                return;
            }
            if let Some(buf) = active_buffer(&tv) {
                let flags = if ct.is_active() {
                    gtk::TextSearchFlags::TEXT_ONLY
                } else {
                    gtk::TextSearchFlags::CASE_INSENSITIVE
                };
                // Clear previous highlights
                for tag_name in &["search-match", "search-current"] {
                    if let Some(tag) = buf.tag_table().lookup(tag_name) {
                        buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
                    }
                }
                // Find all matches
                let mut matches = Vec::new();
                let mut iter = buf.start_iter();
                while let Some((start, end)) = iter.forward_search(&query, flags, None) {
                    matches.push((start.clone(), end.clone()));
                    iter = end;
                }
                let count = matches.len();
                state.borrow_mut().matches = matches;
                state.borrow_mut().current = 0;
                ml.set_label(&format!("{}/{}", if count > 0 { 1 } else { 0 }, count));
                // Highlight all matches
                if let Some(tag) = buf.tag_table().lookup("search-match") {
                    for (s, e) in state.borrow().matches.iter() {
                        buf.apply_tag(&tag, s, e);
                    }
                }
                // Highlight current match
                if let Some(tag) = buf.tag_table().lookup("search-current") {
                    if let Some((s, e)) = state.borrow().matches.get(0) {
                        buf.apply_tag(&tag, s, e);
                        buf.select_range(s, e);
                        scroll_to_cursor(&tv);
                    }
                }
            }
        }))
    };

    // ── On each keystroke ────────────────────────────────────
    {
        let rs = run_search.clone();
        search_entry.connect_search_changed(move |_| {
            rs();
        });
    }

    // ── Case toggle ──────────────────────────────────────────
    {
        let rs = run_search.clone();
        case_toggle.connect_toggled(move |_| {
            rs();
        });
    }

    // ── Find Next ────────────────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        find_next.connect_clicked(move |_| {
            navigate_match(&tv, &state, &ml, 1);
        });
    }

    // ── Find Previous ────────────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let ml = match_label.clone();
        find_prev.connect_clicked(move |_| {
            navigate_match(&tv, &state, &ml, -1);
        });
    }

    // ── Replace current match ────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let re = replace_entry.clone();
        let rs = run_search.clone();
        replace_btn.connect_clicked(move |_| {
            let replacement = re.text().to_string();
            let st = state.borrow();
            if st.matches.is_empty() { return; }
            if let Some((start, end)) = st.matches.get(st.current) {
                if let Some(buf) = active_buffer(&tv) {
                    let mut s = start.clone();
                    let mut e = end.clone();
                    buf.begin_user_action();
                    buf.delete(&mut s, &mut e);
                    buf.insert(&mut s, &replacement);
                    buf.end_user_action();
                }
            }
            drop(st);
            rs();
        });
    }

    // ── Replace All ──────────────────────────────────────────
    {
        let tv = tv.clone();
        let state = state.clone();
        let re = replace_entry.clone();
        let rs = run_search.clone();
        replace_all_btn.connect_clicked(move |_| {
            let replacement = re.text().to_string();
            let st = state.borrow();
            let matches = st.matches.clone();
            drop(st);
            if matches.is_empty() { return; }
            if let Some(buf) = active_buffer(&tv) {
                buf.begin_user_action();
                for (s, e) in matches.into_iter().rev() {
                    let mut start = s;
                    let mut end = e;
                    buf.delete(&mut start, &mut end);
                    buf.insert(&mut start, &replacement);
                }
                buf.end_user_action();
            }
            rs();
        });
    }

    // ── Close button ─────────────────────────────────────────
    let rev = Rc::new(RefCell::new(None::<gtk::Revealer>));

    // ── Layout ───────────────────────────────────────────────
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    hbox.set_margin_start(6);
    hbox.set_margin_end(6);
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);
    hbox.append(&search_entry);
    hbox.append(&replace_entry);
    hbox.append(&match_label);
    hbox.append(&find_prev);
    hbox.append(&find_next);
    hbox.append(&replace_btn);
    hbox.append(&replace_all_btn);
    hbox.append(&case_toggle);
    hbox.append(&close_btn);

    let revealer = gtk::Revealer::new();
    revealer.set_child(Some(&hbox));
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_reveal_child(false);

    // Wire close (after revealer is created)
    {
        let rev = rev.clone();
        let r2 = revealer.clone();
        close_btn.connect_clicked(move |_| {
            r2.set_reveal_child(false);
        });
    }

    // Escape in search entry closes
    {
        let r = revealer.clone();
        search_entry.connect_activate(move |_| {
            // Enter in search = find next
            find_next.activate();
        });
    }

    (revealer, search_entry)
}

/// Scroll the active text view so the cursor is visible.
/// Navigate to the next/previous match and update highlights.
fn navigate_match(tv: &adw::TabView, state: &RefCell<FindState>, ml: &gtk::Label, direction: i32) {
    let mut st = state.borrow_mut();
    if st.matches.is_empty() { return; }
    let n = st.matches.len() as i32;
    let new_idx = ((st.current as i32 + direction).rem_euclid(n)) as usize;
    st.current = new_idx;
    let m = st.matches[new_idx].clone();
    drop(st);
    if let Some(buf) = active_buffer(tv) {
        if let Some(tag) = buf.tag_table().lookup("search-current") {
            buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
        }
        if let Some(tag) = buf.tag_table().lookup("search-current") {
            buf.apply_tag(&tag, &m.0, &m.1);
        }
        buf.select_range(&m.0, &m.1);
        scroll_to_cursor(tv);
    }
    ml.set_label(&format!("{}/{}", new_idx + 1, n));
}

fn scroll_to_cursor(tv: &adw::TabView) {
    if let Some(page) = tv.selected_page() {
        if let Some(textview) = get_textview(&page.child()) {
            let buf = textview.buffer();
            let mark = buf.get_insert();
            textview.scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
        }
    }
}

// ── TextTag registration ────────────────────────────────────────────────

pub fn register_formatting_tags(buffer: &gtk::TextBuffer) {
    let tb = buffer.tag_table();
    macro_rules! add { ($tag:expr) => { tb.add(&$tag); }}
    add!(gtk::TextTag::builder().name("bold").weight(700).build());
    add!(gtk::TextTag::builder().name("italic").style(gtk4::pango::Style::Italic).build());
    add!(gtk::TextTag::builder().name("underline").underline(gtk4::pango::Underline::Single).build());
    add!(gtk::TextTag::builder().name("strikethrough").strikethrough(true).build());
    add!(gtk::TextTag::builder().name("highlight").background("#FFFF00").build());
    add!(gtk::TextTag::builder().name("h1").scale(2.0).weight(700).build());
    add!(gtk::TextTag::builder().name("h2").scale(1.5).weight(700).build());
    add!(gtk::TextTag::builder().name("h3").scale(1.17).weight(700).build());
    add!(gtk::TextTag::builder().name("h4").scale(1.0).weight(700).build());
    add!(gtk::TextTag::builder().name("h5").scale(0.83).weight(700).build());
    add!(gtk::TextTag::builder().name("h6").scale(0.67).weight(700).build());
    add!(gtk::TextTag::builder().name("h-title").scale(2.36).weight(700).build());
    add!(gtk::TextTag::builder().name("h-subtitle").scale(1.36).weight(400).foreground("#666666").build());
    add!(gtk::TextTag::builder().name("normal").build());
    add!(gtk::TextTag::builder().name("code").family("Monospace").background("#F0F0F0").foreground("#333333").build());
    add!(gtk::TextTag::builder().name("blockquote").left_margin(40).style(gtk4::pango::Style::Italic).foreground("#666666").build());
    // Alignment tags
    add!(gtk::TextTag::builder().name("align-left").justification(gtk::Justification::Left).build());
    add!(gtk::TextTag::builder().name("align-center").justification(gtk::Justification::Center).build());
    add!(gtk::TextTag::builder().name("align-right").justification(gtk::Justification::Right).build());
    add!(gtk::TextTag::builder().name("align-justify").justification(gtk::Justification::Fill).build());
    // Font size tags
    add!(gtk::TextTag::builder().name("font-larger").scale(1.2).build());
    add!(gtk::TextTag::builder().name("font-smaller").scale(0.833).build());
    // Search highlight tags
    add!(gtk::TextTag::builder().name("search-match").background("#FFFF00").build());
    add!(gtk::TextTag::builder().name("search-current").background("#FF9800").build());
}

// ── List helpers ─────────────────────────────────────────────────────

fn line_text(buf: &gtk::TextBuffer, iter: &gtk::TextIter) -> String {
    let mut start = iter.clone();
    start.backward_line();
    let mut end = iter.clone();
    end.forward_line();
    buf.text(&start, &end, false).to_string()
}

fn toggle_list(tv: &adw::TabView, kind: &str) {
    if let Some(buf) = active_buffer(tv) {
        let bounds = buf.selection_bounds();
        let (ins, _) = bounds.unwrap_or((buf.start_iter(), buf.start_iter()));
        let text = line_text(&buf, &ins);
        // Check if already a list item
        let has_bullet = text.trim_start().starts_with('\u{2022}')
            || text.trim_start().starts_with("- ");
        let has_number = text.trim_start().starts_with(|c: char| c.is_ascii_digit())
            && text.trim_start().contains(". ");

        buf.begin_user_action();
        let mut start = ins.clone(); start.backward_line();
        let mut end = ins.clone(); end.forward_line();

        if (kind == "bullet" && has_bullet) || (kind == "numbered" && has_number) {
            // Remove list prefix - delete from line start to after prefix
            let line = line_text(&buf, &ins);
            let trimmed = line.trim_start();
            let prefix_end = if kind == "bullet" {
                trimmed.find(|c| c != '\u{2022}' && c != ' ').unwrap_or(0)
            } else {
                trimmed.find(". ").map(|i| i + 2).unwrap_or(0)
            };
            let indent = line.len() - trimmed.len();
            let del_len = indent + prefix_end;
            if del_len > 0 {
                let mut del_end = start.clone();
                del_end.forward_chars(del_len as i32);
                if del_end > start { buf.delete(&mut start, &mut del_end); }
            }
        } else {
            // Insert list prefix
            let prefix = if kind == "bullet" { "\u{2022} " } else { "1. " };
            buf.insert(&mut start, prefix);
        }
        buf.end_user_action();
    }
}

/// Connect list auto-continuation on Enter for a new buffer.
/// Uses EventControllerKey on the TextView to detect Enter.
fn connect_list_continuation(editor: &gtk::TextView, buf: &gtk::TextBuffer) {
    let buf = buf.clone();
    let ctrl = gtk::EventControllerKey::new();
    ctrl.connect_key_pressed(move |_, key, _code, _state| {
        if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
            let bounds = buf.selection_bounds();
            let (ins, _) = bounds.unwrap_or((buf.start_iter(), buf.start_iter()));
            let mut line_start = ins.clone();
            line_start.backward_line();
            let mut line_end = ins.clone();
            line_end.forward_line();
            let line = buf.text(&line_start, &line_end, false);
            let trimmed = line.trim_start();

            // Bullet list continuation
            if trimmed.starts_with("\u{2022}") || trimmed.starts_with("- ") {
                let indent = line.len() - trimmed.len();
                let marker = "\u{2022} ";
                let after_marker = trimmed
                    .strip_prefix("\u{2022}").or_else(|| trimmed.strip_prefix("- "))
                    .unwrap_or("").trim_start();
                if after_marker.is_empty() {
                    return glib::Propagation::Proceed;
                }
                let prefix = format!("{}{}", " ".repeat(indent), marker);
                buf.insert(&mut line_end, &prefix);
                return glib::Propagation::Stop;
            }

            // Numbered list continuation
            if trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains(". ") {
                let num_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                let after_num = &trimmed[num_str.len()..];
                let rest = after_num.strip_prefix(". ").unwrap_or("");
                if let Ok(n) = num_str.parse::<usize>() {
                    if rest.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    let indent = line.len() - trimmed.len();
                    let new_prefix = format!("{}{}. ", " ".repeat(indent), n + 1);
                    buf.insert(&mut line_end, &new_prefix);
                    return glib::Propagation::Stop;
                }
            }
        }
        glib::Propagation::Proceed
    });
    editor.add_controller(ctrl);
}

// ── Markdown macros ──────────────────────────────────────────────────
// Auto-formatting on Space/Enter: converts markdown syntax to rich text.

fn connect_markdown_macros(buf: &gtk::TextBuffer) {
    let buf = buf.clone();
    buf.connect_insert_text(move |buf, pos, text| {
        // Only trigger on Space (inline patterns) and Enter (block patterns)
        if text != " " && text != "\n" && text != "\r\n" { return; }

        let insert_pos = pos.offset();

        // ── Inline patterns (on Space) ──────────────────────────────
        if text == " " {
            // Check 2-10 chars before cursor for markdown patterns
            let start = if insert_pos >= 10 { insert_pos - 10 } else { 0 };
            let mut iter = buf.start_iter();
            iter.set_offset(start);
            let mut end = buf.start_iter();
            end.set_offset(insert_pos);
            let before = buf.text(&iter, &end, false).to_string();

            // Bold: **text** 
            if let Some(inner) = extract_md_pattern(&before, "**", "**") {
                apply_md_pattern(buf, &before, "**", inner, "bold");
                return;
            }
            // Italic: *text*
            if let Some(inner) = extract_md_pattern(&before, "*", "*") {
                apply_md_pattern(buf, &before, "*", inner, "italic");
                return;
            }
            // Strikethrough: ~~text~~
            if let Some(inner) = extract_md_pattern(&before, "~~", "~~") {
                apply_md_pattern(buf, &before, "~~", inner, "strikethrough");
                return;
            }
            // Inline code: `text`
            if let Some(inner) = extract_md_pattern(&before, "`", "`") {
                apply_md_pattern(buf, &before, "`", inner, "code");
                return;
            }
        }

        // ── Block patterns (on Enter) ──────────────────────────────
        if text == "\n" || text == "\r\n" {
            let mut line_iter = buf.start_iter();
            line_iter.set_offset(insert_pos);
            let mut line_start = line_iter.clone();
            line_start.backward_line();
            let mut line_end = line_iter.clone();
            line_end.forward_line();
            let line = buf.text(&line_start, &line_end, false);
            let trimmed = line.trim_start();

            // Heading: # ## ###
            for level in 1..=6 {
                let prefix = format!("{} ", "#".repeat(level));
                if trimmed.starts_with(&prefix) {
                    let tag_name = format!("h{}", level);
                    let content = trimmed[prefix.len()..].to_string();
                    let indent = line.len() - trimmed.len();
                    buf.begin_user_action();
                    // Delete the markdown prefix
                    let mut del_start = line_start.clone();
                    del_start.forward_chars(indent as i32 + prefix.len() as i32);
                    buf.delete(&mut line_start, &mut del_start);
                    // Apply heading tag
                    if let Some(tag) = buf.tag_table().lookup(&tag_name) {
                        let mut start = line_start; // now at content start
                        let mut end = line_end.clone();
                        end.backward_char(); // exclude trailing newline
                        buf.apply_tag(&tag, &start, &end);
                    }
                    buf.end_user_action();
                    return;
                }
            }

            // Blockquote: >
            if trimmed.starts_with("> ") {
                let content = trimmed[2..].to_string();
                let indent = line.len() - trimmed.len();
                buf.begin_user_action();
                let mut del_start = line_start.clone();
                del_start.forward_chars(indent as i32 + 2);
                buf.delete(&mut line_start, &mut del_start);
                if let Some(tag) = buf.tag_table().lookup("blockquote") {
                    let mut start = line_start;
                    let mut end = line_end.clone();
                    end.backward_char();
                    buf.apply_tag(&tag, &start, &end);
                }
                buf.end_user_action();
                return;
            }
        }
    });
}

/// Extract content between two delimiters in the text before cursor.
/// Returns the inner text if the pattern is found at the end of the string.
fn extract_md_pattern<'a>(before: &'a str, open: &str, close: &str) -> Option<&'a str> {
    // The pattern should be at the end: "something **text** "
    let trimmed = before.trim_end();
    // Check for space before pattern (word boundary)
    if !trimmed.ends_with(close) { return None; }
    let close_pos = trimmed.len() - close.len();
    if close_pos < open.len() { return None; }
    let before_close = &trimmed[..close_pos];
    if !before_close.ends_with(open) { return None; }
    let open_pos = before_close.len() - open.len();
    if open_pos == 0 || before_close.as_bytes()[open_pos - 1] == b' ' {
        let inner = &before_close[open_pos + open.len()..];
        if !inner.is_empty() {
            return Some(inner);
        }
    }
    None
}

/// Apply a markdown pattern: delete the markers, insert clean text, apply tag.
fn apply_md_pattern(buf: &gtk::TextBuffer, before: &str, delimiter: &str, inner: &str, tag_name: &str) {
    let offset = before.len() as i32;
    let del_len = (delimiter.len() * 2 + inner.len()) as i32;
    let start_off = offset - del_len;

    buf.begin_user_action();
    // Delete the markdown syntax (delimiters + inner text)
    let mut start = buf.start_iter();
    start.set_offset(start_off);
    let mut end = buf.start_iter();
    end.set_offset(offset);
    buf.delete(&mut start, &mut end);
    // Insert clean text
    let mut pos = buf.start_iter();
    pos.set_offset(start_off);
    buf.insert(&mut pos, inner);
    // Apply the formatting tag
    if let Some(tag) = buf.tag_table().lookup(tag_name) {
        let mut tag_start = buf.start_iter();
        tag_start.set_offset(start_off);
        let mut tag_end = buf.start_iter();
        tag_end.set_offset(start_off + inner.len() as i32);
        buf.apply_tag(&tag, &tag_start, &tag_end);
    }
    // Insert trailing space
    let mut space_pos = buf.start_iter();
    space_pos.set_offset(start_off + inner.len() as i32);
    buf.insert(&mut space_pos, " ");
    buf.end_user_action();
}
