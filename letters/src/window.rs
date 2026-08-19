// SPDX-License-Identifier: GPL-3.0-or-later
//
// LettersWindow — Tabbed word processor following gnome-gui-spec AdwTabView pattern.

use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::{AlertDialogExt, AlertDialogExtManual, AdwDialogExt};
use std::cell::RefCell;
use std::rc::Rc;

use crate::actions::{connect_list_continuation, connect_markdown_macros, register_formatting_tags};
use crate::dialogs::{active_buffer, get_textview, make_find_replace_widget, show_header_footer_dialog};
use crate::page_container::PageContainer;

// ── Crash-recovery snapshots ─────────────────────────────────────────────
// One AutosaveSlot per tab (not per window, unlike Tables/Decks): each tab
// is its own document, so each needs its own doc_id and its own slot.
static NEXT_DOC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn next_doc_id() -> String {
    let n = NEXT_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

fn autosave_state_dir() -> std::path::PathBuf {
    // glib::user_state_dir() needs the "v2_72" feature this workspace's
    // glib binding doesn't enable — do the XDG fallback ourselves.
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("letters")
}

// ── Per-tab state via widget Qdata ─────────────────────────────────────
// Session identity (file/closing/autosave) is canonical, GTK-free state —
// letters_core::DocumentSession (#103). The tab's actual document
// *content* is still the GtkTextBuffer itself; there's no GTK-free
// representation of that today (a larger design question than this
// slice), so TabData only owns identity, not content.

#[derive(Clone)]
struct TabData(Rc<RefCell<TabDataInner>>);
type TabDataInner = letters_core::DocumentSession;
impl TabData {
    fn new() -> Self {
        TabData(Rc::new(RefCell::new(TabDataInner::new(Rc::new(
            suite_common::autosave::AutosaveSlot::new(autosave_state_dir(), next_doc_id()),
        )))))
    }
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
    // Focus the editor whenever its tab becomes visible; otherwise keystrokes
    // fall through to the window and the find SearchBar captures them.
    editor.connect_map(|ed| {
        let ed = ed.clone();
        glib::idle_add_local_once(move || { ed.grab_focus(); });
    });
    // Transparent background so PageContainer's white page shows through (no black block in dark mode)
    let css_provider = gtk::CssProvider::new();
    let font_css = settings
        .map(|s| s.string("font"))
        .filter(|f| !f.is_empty())
        .map(|f| gtk4::pango::FontDescription::from_string(&f))
        .filter(|desc| desc.size() > 0)
        .map(|desc| {
            let family = desc.family().map(|f| f.to_string()).unwrap_or_else(|| "sans-serif".into());
            let size_pt = desc.size() as f64 / gtk4::pango::SCALE as f64;
            format!("textview, textview text {{ font-family: \"{family}\"; font-size: {size_pt}pt; }}")
        })
        .unwrap_or_default();
    // The page is always white regardless of app theme (see above), so
    // the text color must be pinned dark too — otherwise dark mode's
    // light theme-default text color renders white-on-white and the
    // whole document becomes invisible while still fully editable.
    css_provider.load_from_string(&format!(
        "textview, textview text, scrolledwindow {{ background: transparent; }} \
         textview text {{ color: rgba(0, 0, 0, 0.85); }} {font_css}"
    ));
    gtk::style_context_add_provider_for_display(&editor.display(), &css_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    // Spell-check via zspell (hunspell-compatible, pure Rust).
    // Applies red wavy underline to misspelled words, re-checks on edits.
    let spell_enabled = settings.map(|s| s.boolean("spell-check-enabled")).unwrap_or(true);
    if spell_enabled {
        crate::spell::SpellChecker::new(&buffer).start();
    }
    // Restore line spacing from GSettings
    if let Some(s) = settings {
        let ls = s.double("line-spacing");
        let tag_name = if ls >= 1.8 { "line-spacing-2.0" }
            else if ls >= 1.4 { "line-spacing-1.5" }
            else if ls >= 1.1 { "line-spacing-1.15" }
            else { "line-spacing-1.0" };
        if let Some(tag) = buffer.tag_table().lookup(tag_name) {
            let start = buffer.start_iter();
            let end = buffer.end_iter();
            buffer.apply_tag(&tag, &start, &end);
        }
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
    // Cross-app clipboard (DESIGN-UI): Ctrl+C offers the suite fragment
    // (styled runs) alongside HTML and plain text; Ctrl+V prefers it.
    // Capture phase so we can supersede the TextView's built-in
    // plain-text handling only when suite content is involved.
    {
        let buf = buffer.clone();
        let ed = editor.clone();
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _code, mods| {
            let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            if ctrl && keyval == gtk4::gdk::Key::c {
                if let Some((start, end)) = buf.selection_bounds() {
                    let doc = crate::bridge::capture_from_buffer(&buf);
                    let frag = letters_core::fragment::from_selection(
                        &doc,
                        start.offset() as usize,
                        end.offset() as usize,
                    );
                    let provider = suite_common::clipboard::provider(
                        letters_core::fragment::MIME,
                        &frag.to_json(),
                        &frag.to_html(),
                        &frag.to_plain(),
                    );
                    let _ = ed.clipboard().set_content(Some(&provider));
                    return gtk4::glib::Propagation::Stop;
                }
                return gtk4::glib::Propagation::Proceed;
            }
            if ctrl && keyval == gtk4::gdk::Key::v {
                let clipboard = ed.clipboard();
                if suite_common::clipboard::offers(&clipboard, letters_core::fragment::MIME) {
                    let buf = buf.clone();
                    suite_common::clipboard::read_string(
                        &clipboard,
                        letters_core::fragment::MIME,
                        move |json| {
                            if let Some(frag) = json
                                .as_deref()
                                .and_then(letters_core::fragment::Fragment::from_json)
                            {
                                insert_fragment(&buf, &frag);
                            }
                        },
                    );
                    return gtk4::glib::Propagation::Stop;
                }
                return gtk4::glib::Propagation::Proceed;
            }
            gtk4::glib::Propagation::Proceed
        });
        editor.add_controller(key);
    }

    // Selection format popover: context reveals capability (DESIGN-UI §1).
    // Non-autohide so it never steals focus from the editor; buttons fire
    // the same app actions as the toolbar.
    {
        let pop = gtk::Popover::new();
        pop.set_parent(&editor);
        pop.set_autohide(false);
        pop.set_position(gtk::PositionType::Top);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("linked");
        for (icon, tooltip, action) in [
            ("format-text-bold-symbolic", "Bold", "app.bold"),
            ("format-text-italic-symbolic", "Italic", "app.italic"),
            ("format-text-underline-symbolic", "Underline", "app.underline"),
            ("format-text-strikethrough-symbolic", "Strikethrough", "app.strikethrough"),
            ("color-select-symbolic", "Highlight", "app.highlight"),
            ("insert-link-symbolic", "Insert link", "app.insertlink"),
        ] {
            let b = gtk::Button::from_icon_name(icon);
            b.add_css_class("flat");
            b.set_tooltip_text(Some(tooltip));
            b.set_action_name(Some(action));
            row.append(&b);
        }
        pop.set_child(Some(&row));

        let ed = editor.clone();
        let pop2 = pop.clone();
        buffer.connect_mark_set(move |buf, _iter, mark| {
            let name = mark.name();
            let name = name.as_deref();
            if name != Some("insert") && name != Some("selection_bound") {
                return;
            }
            if let Some((start, _end)) = buf.selection_bounds() {
                let loc = ed.iter_location(&start);
                let (x, y) = ed.buffer_to_window_coords(
                    gtk::TextWindowType::Widget, loc.x(), loc.y());
                pop2.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                    x, y, 1, loc.height())));
                if !pop2.is_visible() {
                    pop2.popup();
                }
            } else if pop2.is_visible() {
                pop2.popdown();
            }
        });
        let pop3 = pop.clone();
        editor.connect_destroy(move |_| pop3.unparent());
    }

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&editor));
    scroll.set_vexpand(true); scroll.set_hexpand(true);
    // Transparent background so PageContainer's white page shows through
    gtk::style_context_add_provider_for_display(&scroll.display(), &css_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    let container = PageContainer::new();
    if let Some(s) = settings {
        container.load_from_settings(s);
    }
    scroll.set_parent(&container);
    container.set_vexpand(true); container.set_hexpand(true);
    // Zoom via Ctrl+Scroll
    {
        let pc = container.clone();
        let s = settings.cloned();
        let scroll_ctrl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll_ctrl.connect_scroll(move |ctrl, _dx, dy| {
            // Check if Ctrl is held
            let state = ctrl.current_event_state();
            if !state.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            let current = pc.zoom_level();
            let delta = if dy > 0.0 { -10.0 } else { 10.0 };
            let new_zoom = (current + delta).clamp(50.0, 200.0);
            pc.set_zoom(new_zoom);
            if let Some(ref s) = s { let _ = s.set_double("zoom-level", new_zoom); }
            glib::Propagation::Stop
        });
        editor.add_controller(scroll_ctrl);
    }
    // Pagination: recalculate page count on buffer changes (debounced)
    if let Some(s) = settings {
        let s = s.clone();
        let pc = container.clone();
        let ed = editor.clone();
        let timer = std::rc::Rc::new(std::cell::RefCell::new(None::<glib::SourceId>));
        let pages_store = std::rc::Rc::new(std::cell::RefCell::new(Vec::<crate::layout::Page>::new()));
        let ps = pages_store.clone();
        let t = timer.clone();
        let b2 = buffer.clone();
        buffer.connect_changed(move |_| {
            if let Some(id) = t.borrow_mut().take() { id.remove(); }
            let buf = b2.clone();
            let pc = pc.clone();
            let ed = ed.clone();
            let s = s.clone();
            let t2 = t.clone();
            let ps2 = ps.clone();
            let id = glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let config = crate::layout::LayoutConfig::from_settings(&s);
                let pages = crate::layout::paginate(&buf, &config, &ed.pango_context());
                pc.set_page_count(pages.len());
                ps2.borrow_mut().clone_from(&pages);
                t2.borrow_mut().take();
                glib::ControlFlow::Break
            });
            *t.borrow_mut() = Some(id);
        });
    }
    (container, buffer)
}

// ── LettersWindow ───────────────────────────────────────────────────────

pub struct LettersWindow {
    pub window: adw::ApplicationWindow,
    tab_view: adw::TabView,
    stack: gtk4::Stack,
    word_count_label: gtk4::Label,
    style_label: gtk4::Label,
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
        // Cursor style readout ("Heading 2 · Bold") — DESIGN-UI: the status
        // bar is live, not decorative.
        let style_label = gtk4::Label::new(Some("Normal"));
        style_label.add_css_class("caption");
        style_label.add_css_class("dim-label");
        style_label.set_margin_start(12);
        status_bar.append(&style_label);
        // Zoom slider in status bar
        let zoom_adj = gtk4::Adjustment::new(100.0, 50.0, 200.0, 5.0, 10.0, 0.0);
        let zoom_slider = gtk4::Scale::new(gtk4::Orientation::Horizontal, Some(&zoom_adj));
        zoom_slider.set_width_request(120);
        zoom_slider.set_draw_value(false);
        let zoom_label = gtk4::Label::new(Some("100%"));
        status_bar.append(&zoom_label);
        status_bar.append(&zoom_slider);
        // Wire zoom slider to update all PageContainers
        {
            let tv = tab_view.clone();
            let zl = zoom_label.clone();
            let s = settings.clone();
            zoom_slider.connect_value_changed(move |slider| {
                let val = slider.value();
                zl.set_text(&format!("{}%", val as i32));
                let _ = s.set_double("zoom-level", val);
                for i in 0..tv.n_pages() {
                    let page = tv.nth_page(i);
                    if let Some(pc) = page.child().first_child()
                        .and_then(|c| c.downcast::<crate::page_container::PageContainer>().ok())
                    {
                        pc.set_zoom(val);
                    }
                }
            });
        }

        suite_common::actions::register_labels(&[
            ("app.bold", &suite_common::i18n("Bold")),
            ("app.italic", &suite_common::i18n("Italic")),
            ("app.underline", &suite_common::i18n("Underline")),
            ("app.strikethrough", &suite_common::i18n("Strikethrough")),
            ("app.highlight", &suite_common::i18n("Highlight")),
            ("app.bullet-list", &suite_common::i18n("Bullet List")),
            ("app.numbered-list", &suite_common::i18n("Numbered List")),
            ("app.align-left", &suite_common::i18n("Align Left")),
            ("app.align-center", &suite_common::i18n("Align Center")),
            ("app.align-right", &suite_common::i18n("Align Right")),
            ("app.align-justify", &suite_common::i18n("Justify")),
            ("app.insertlink", &suite_common::i18n("Insert Link…")),
            ("app.insertimage", &suite_common::i18n("Insert Image…")),
            ("app.insert-table", &suite_common::i18n("Insert Table…")),
            ("app.table-insert-row-above", &suite_common::i18n("Insert Row Above")),
            ("app.table-insert-row-below", &suite_common::i18n("Insert Row Below")),
            ("app.table-insert-col-left", &suite_common::i18n("Insert Column Left")),
            ("app.table-insert-col-right", &suite_common::i18n("Insert Column Right")),
            ("app.table-delete-row", &suite_common::i18n("Delete Row")),
            ("app.table-delete-col", &suite_common::i18n("Delete Column")),
            ("app.list-indent", &suite_common::i18n("Indent List")),
            ("app.list-outdent", &suite_common::i18n("Outdent List")),
            ("app.list-restart-numbering", &suite_common::i18n("Restart Numbering")),
            ("app.insert-page-break", &suite_common::i18n("Insert Page Break")),
            ("app.cycle-line-spacing", &suite_common::i18n("Cycle Line Spacing")),
            ("app.cycle-columns", &suite_common::i18n("Cycle Column Layout")),
            ("app.increase-font", &suite_common::i18n("Increase Font Size")),
            ("app.decrease-font", &suite_common::i18n("Decrease Font Size")),
            ("app.find", &suite_common::i18n("Find and Replace")),
            ("app.undo", &suite_common::i18n("Undo")),
            ("app.redo", &suite_common::i18n("Redo")),
            ("app.page-setup", &suite_common::i18n("Page Setup…")),
            ("app.print", &suite_common::i18n("Print…")),
            ("app.print-preview", &suite_common::i18n("Print Preview")),
            ("app.export-pdf", &suite_common::i18n("Export as PDF…")),
            ("app.edit-headers", &suite_common::i18n("Edit Headers and Footers…")),
            ("app.style-p", &suite_common::i18n("Paragraph Style: Normal")),
            ("app.style-h1", &suite_common::i18n("Paragraph Style: Heading 1")),
            ("app.style-h2", &suite_common::i18n("Paragraph Style: Heading 2")),
            ("app.style-h3", &suite_common::i18n("Paragraph Style: Heading 3")),
            ("app.style-h4", &suite_common::i18n("Paragraph Style: Heading 4")),
            ("app.style-h5", &suite_common::i18n("Paragraph Style: Heading 5")),
            ("app.style-h6", &suite_common::i18n("Paragraph Style: Heading 6")),
            ("app.style-code", &suite_common::i18n("Paragraph Style: Code")),
            ("app.style-quote", &suite_common::i18n("Paragraph Style: Block Quote")),
            ("app.insert-footnote", &suite_common::i18n("Insert Footnote\u{2026}")),
            ("app.autosave-now", &suite_common::i18n("Save Crash-Recovery Snapshot Now")),
        ]);

        let primary_toolbar: Vec<suite_common::ToolbarItem> = vec![
            ("format-text-bold-symbolic", "Bold (Ctrl+B)", "app.bold"),
            ("format-text-italic-symbolic", "Italic (Ctrl+I)", "app.italic"),
            ("format-text-underline-symbolic", "Underline (Ctrl+U)", "app.underline"),
        ];

        let extended_toolbar: Vec<suite_common::ToolbarItem> = vec![
            ("format-text-strikethrough-symbolic", "Strikethrough", "app.strikethrough"),
            ("color-select-symbolic", "Highlight", "app.highlight"),
            ("view-list-bullet-symbolic", "Bullet list (Ctrl+Shift+8)", "app.bullet-list"),
            ("view-list-ordered-symbolic", "Numbered list (Ctrl+Shift+7)", "app.numbered-list"),
            ("format-justify-left-symbolic", "Align left (Ctrl+L)", "app.align-left"),
            ("format-justify-center-symbolic", "Align center (Ctrl+E)", "app.align-center"),
            ("format-justify-right-symbolic", "Align right (Ctrl+R)", "app.align-right"),
            ("format-justify-fill-symbolic", "Justify (Ctrl+J)", "app.align-justify"),
            ("insert-link-symbolic", "Insert link (Ctrl+Shift+K)", "app.insertlink"),
            ("view-continuous-symbolic", "Line spacing", "app.cycle-line-spacing"),
            ("view-dual-symbolic", "Column layout", "app.cycle-columns"),
        ];

        let suite_win = suite_common::SuiteWindow::new(app, "Letters", primary_toolbar, extended_toolbar);
        suite_common::bind_window_geometry(&suite_win.window, &settings);
        suite_win.toolbar.container.set_visible(settings.boolean("show-toolbar"));
        {
            let tb = suite_win.toolbar.container.clone();
            settings.connect_changed(Some("show-toolbar"), move |s, _| {
                tb.set_visible(s.boolean("show-toolbar"));
            });
        }
        // HIG: tab bar lives inside the header bar, not as a separate
        // stacked bar (fixes #73).
        suite_win.header_bar.set_title_widget(Some(&tab_bar));
        // Content is set below, after wrapping toast_overlay in the find/replace
        // gtk::Overlay — setting it here would give toast_overlay a parent and
        // make the later Overlay::set_child fail, orphaning the whole editor UI.
        suite_win.add_bottom_bar(&status_bar);

        // ── Ruler ──────────────────────────────────────────────────
        let ruler_widget = crate::ruler::Ruler::new();
        ruler_widget.load_from_settings(&settings);
        {
            let s = settings.clone();
            let rw = ruler_widget.downgrade();
            let tv = tab_view.clone();
            ruler_widget.connect_changed(move || {
                if let Some(r) = rw.upgrade() {
                    let _ = s.set_double("page-margin-left", r.margin_left());
                    let _ = s.set_double("page-margin-right", r.margin_right());
                    // Apply tab stops to active buffer
                    if let Some(tab_array) = r.get_tab_array() {
                        if let Some(buf) = active_buffer(&tv) {
                            let cursor = buf.cursor_position();
                            let mut start = buf.iter_at_offset(cursor);
                            start.backward_line();
                            let mut end = buf.iter_at_offset(cursor);
                            if !end.ends_line() { end.forward_to_line_end(); }
                            if let Some(tag) = buf.tag_table().lookup("tab-stops") {
                                buf.remove_tag(&tag, &start, &end);
                            }
                            // Create/update tab-stops tag
                            let tag = if let Some(t) = buf.tag_table().lookup("tab-stops") {
                                t
                            } else {
                                let t = gtk::TextTag::builder().name("tab-stops").build();
                                buf.tag_table().add(&t);
                                t
                            };
                            tag.set_tabs(Some(&tab_array));
                            buf.apply_tag(&tag, &start, &end);
                        }
                    }
                }
            });
        }
        suite_win.add_top_bar(&ruler_widget);
        // Rulers are opt-in (DESIGN-UI): hidden by default, toggled via
        // the palette / Ctrl+Shift+R.
        ruler_widget.set_visible(false);
        // Keep the ruler origin glued to the visible page edge (zoom,
        // resize, sidebar changes all move it). set_screen_page only
        // redraws on actual change, so the per-frame cost is a compare.
        {
            let tv = tab_view.clone();
            ruler_widget.add_tick_callback(move |ruler, _| {
                let pc = tv
                    .selected_page()
                    .and_then(|p| p.child().downcast::<crate::page_container::PageContainer>().ok());
                if let Some(pc) = pc {
                    let (px, sw) = pc.page_screen_geometry();
                    if sw > 0.0 {
                        if let Some(pt) = pc.compute_point(
                            ruler,
                            &gtk4::graphene::Point::new(px as f32, 0.0),
                        ) {
                            ruler.set_screen_page(pt.x() as f64, sw);
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        {
            let rw = ruler_widget.clone();
            let a = gtk::gio::SimpleAction::new("toggle-ruler", None);
            a.connect_activate(move |_, _| rw.set_visible(!rw.is_visible()));
            app.add_action(&a);
            app.set_accels_for_action("app.toggle-ruler", &["<Primary><Shift>r"]);
            suite_common::actions::register_labels(&[("app.toggle-ruler", &suite_common::i18n("Toggle Ruler"))]);
        }

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
        // Compact dropdown inside the toolbar (a full-width style band
        // was the design review's worst double-chrome offender).
        style_dropdown.set_tooltip_text(Some(&suite_common::i18n("Paragraph style")));
        suite_win.toolbar.container.prepend(&style_dropdown);

        // Narrow breakpoint (≤ 500sp): hide the style dropdown to save
        // horizontal space (fixes #79).
        suite_win.narrow_breakpoint.add_setter(
            &style_dropdown, "visible", Some(&false.to_value()));

        let win = suite_win.window.clone();

        // Window title follows the active document (HIG: title reflects
        // content, not just the app name).
        {
            let w = win.clone();
            tab_view.connect_selected_page_notify(move |tv| {
                let title = match tv.selected_page() {
                    // The notify can fire before the page title is set.
                    Some(p) if !p.title().is_empty() => {
                        format!("{} — Letters", p.title())
                    }
                    _ => "Letters".to_string(),
                };
                w.set_title(Some(&title));
            });
        }

        // ── Tab: selected-page ──────────────────────────────────────
        let st = stack.clone();
        tab_view.connect_selected_page_notify(move |_| { st.set_visible_child_name("editor"); });

        // ── Tab: close-page with unsaved confirmation ───────────────
        {
            let _tv = tab_view.clone();
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
                dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                dialog.choose(None::<&gtk::Window>, None::<&gio::Cancellable>,
                    move |response: glib::GString| {
                        match response.as_str() {
                            "save" => {
                                if let Some(td) = tab_data_get(&child) { td.0.borrow_mut().closing_after_save = true; }
                                do_save(&tv2, &st2);
                            }
                            "discard" => {
                                clear_tab_autosave(&child);
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
                    "The following documents have unsaved changes:\n• {}\nChoose Save All to keep your changes, or Discard All to close without saving.",
                    dirty.join("\n• ")
                );
                let dialog = adw::AlertDialog::new(Some("Unsaved changes"), Some(body.as_str()));
                dialog.add_responses(&[("cancel", "_Cancel"), ("discard", "_Discard All"), ("save", "_Save All")]);
                dialog.set_close_response("cancel");
                dialog.set_default_response(Some("save"));
                dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
                dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                let win_weak = win.downgrade();
                let force_close_clone = force_close.clone();
                let tv_clone = tv.clone();
                dialog.choose(Some(win), None::<&gio::Cancellable>,
                    move |response: glib::GString| {
                        match response.as_str() {
                            "discard" => {
                                for i in 0..tv_clone.n_pages() {
                                    clear_tab_autosave(&tv_clone.nth_page(i).child());
                                }
                                force_close_clone.set(true);
                                if let Some(w) = win_weak.upgrade() { w.close(); }
                            }
                            "save" => {
                                if let Some(w) = win_weak.upgrade() {
                                    let n = tv_clone.n_pages();
                                    let queue: std::collections::VecDeque<adw::TabPage> = (0..n)
                                        .map(|i| tv_clone.nth_page(i))
                                        .filter(|p| p.needs_attention())
                                        .collect();
                                    close_all_dirty_pages(w, tv_clone.clone(), queue, force_close_clone.clone());
                                }
                            }
                            _ => {}
                        }
                    },
                );
                glib::Propagation::Stop
            });
        }

        // ── Find/Replace revealer ──────────────────────────────────
        let (find_revealer, find_entry) = make_find_replace_widget(&tab_view);
        find_revealer.set_key_capture_widget(Some(&suite_win.window));
        // Place search bar as overlay on content (not as stacked top bar)
        let content_overlay = gtk::Overlay::new();
        content_overlay.set_child(Some(&toast_overlay));
        content_overlay.add_overlay(&find_revealer);
        suite_win.set_content(&content_overlay);

        // ── Find/Replace actions ────────────────────────────────────
        let fe = find_entry.clone();
        let fr = find_revealer.clone();
        let a = gtk::gio::SimpleAction::new("find", None);
        a.connect_activate(move |_, _| {
            fr.set_search_mode(true);
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
                // GtkPageSetupUnixDialog predates GTK4's FileDialog-style async
                // dialogs and has no non-deprecated replacement for its
                // response signal; `.present()` below is the real 4.10 fix.
                #[allow(deprecated)]
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
                dialog.present();
            });
            app.add_action(&a);
            app.set_accels_for_action("app.page-setup", &["<Primary><Shift>l"]);
        }

        // ── Actions ────────────────────────────────────────────────
        Self::register_actions(&tab_view, &stack, &word_count_label, &style_label, &win, app, &settings);
        crate::actions::register_formatting_actions(&tab_view, app);
        crate::actions::register_structured_actions(&tab_view, app);

        // ── Drag and Drop file opening ────────────────────────────
        {
            let tv_for_drop = tab_view.clone();
            let settings_for_drop = settings.clone();
            suite_common::attach_file_drop_target(&win, move |paths| {
                for path in paths {
                    let path_str = path.to_string_lossy().to_string();
                    if let Some(buf) = active_buffer(&tv_for_drop) {
                        let _ = crate::bridge::load_file_to_buffer(&path_str, &buf);
                        suite_common::push_recent_file(&settings_for_drop, &path_str);
                    }
                }
            });
        }

        // ── Print action ──────────────────────────────────────────
        {
            let tv = tab_view.clone();
            let w = win.clone();
            let s = settings.clone();
            let a = gtk::gio::SimpleAction::new("print", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    let config = crate::layout::LayoutConfig::from_settings(&s);
                    let ctx = gtk4::pango::Context::new();
                    let pages = crate::layout::paginate(&buf, &config, &ctx);
                    let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                    // Read header/footer from PageContainer
                    let (hdr, ftr) = tv.selected_page()
                        .and_then(|p| p.child().first_child())
                        .and_then(|c| c.downcast::<crate::page_container::PageContainer>().ok())
                        .map(|pc| (pc.header_text(), pc.footer_text()))
                        .unwrap_or_default();

                    let op = gtk::PrintOperation::new();
                    op.set_n_pages(pages.len() as i32);
                    op.connect_draw_page(move |_op, ctx, nth| {
                        let page_idx = nth as usize;
                        if page_idx >= pages.len() { return; }
                        let cr = ctx.cairo_context();
                        let page = &pages[page_idx];
                        // Draw page frame using shared PageContainer rendering (scale=1.0 for print points)
                        crate::page_container::draw_page_to_cairo(
                            &cr, page_idx, 0.0, 0.0,
                            config.page_width_pt, config.page_height_pt, 1.0,
                            config.margin_left, config.margin_right,
                            config.margin_top, config.margin_bottom,
                            &hdr, &ftr,
                        );
                        // Render page text
                        let page_text = if page.end_offset as usize <= text.len() {
                            &text[page.start_offset as usize..page.end_offset as usize]
                        } else { &text };
                        let layout = pangocairo::functions::create_layout(&cr);
                        layout.set_text(page_text);
                        let content_w = (config.page_width_pt - config.margin_left - config.margin_right).max(10.0);
                        layout.set_width((content_w * (pango::SCALE as f64)) as i32);
                        cr.move_to(config.margin_left, config.margin_top);
                        pangocairo::functions::show_layout(&cr, &layout);
                    });
                    op.set_export_filename("output.pdf");
                    let _ = op.run(gtk::PrintOperationAction::PrintDialog, Some(&w));
                }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.print", &["<Primary>p"]);
        }

        // ── Export PDF action (Typst-backed, distinct from print-to-file) ──
        {
            let tv = tab_view.clone();
            let w = win.clone();
            let a = gtk::gio::SimpleAction::new("export-pdf", None);
            a.connect_activate(move |_, _| {
                let Some(buf) = active_buffer(&tv) else { return };
                let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.pdf");
                f.set_name(Some("PDF"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.set_initial_name(Some("Untitled.pdf"));
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                if let Err(e) = crate::engine::export_pdf(&text, &path.to_string_lossy()) {
                                    eprintln!("export pdf failed: {e}");
                                }
                            }
                        }
                    });
            });
            app.add_action(&a);
            app.set_accels_for_action("app.export-pdf", &["<Primary><Shift>e"]);
        }

        // ── Print Preview action ──────────────────────────────────
        {
            let tv = tab_view.clone();
            let w = win.clone();
            let s = settings.clone();
            let a = gtk::gio::SimpleAction::new("print-preview", None);
            a.connect_activate(move |_, _| {
                let buf = active_buffer(&tv);
                if let Some(buf) = buf {
                    let (hdr, ftr) = tv.selected_page()
                        .and_then(|p| p.child().first_child())
                        .and_then(|c| c.downcast::<crate::page_container::PageContainer>().ok())
                        .map(|pc| (pc.header_text(), pc.footer_text()))
                        .unwrap_or_default();
                    crate::print_preview::show_print_preview(&w, &buf, &s, &hdr, &ftr);
                }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.print-preview", &["<Primary><Shift>p"]);
        }

        // ── Line spacing action ──────────────────────────────────
        {
            let tv = tab_view.clone();
            let a = gtk::gio::SimpleAction::new("cycle-line-spacing", None);
            a.connect_activate(move |_, _| {
                if let Some(buf) = active_buffer(&tv) {
                    let (start, end) = buf.selection_bounds().unwrap_or_else(|| {
                        let s = buf.cursor_position();
                        let mut ls = buf.iter_at_offset(s); ls.backward_line();
                        let mut le = buf.iter_at_offset(s);
                        if !le.ends_line() { le.forward_to_line_end(); }
                        (ls, le)
                    });
                    let spacing_tags = ["line-spacing-1.0", "line-spacing-1.15", "line-spacing-1.5", "line-spacing-2.0"];
                    let mut current = 0usize;
                    for (i, t) in spacing_tags.iter().enumerate() {
                        if let Some(tag) = buf.tag_table().lookup(t) {
                            if start.has_tag(&tag) { current = i; break; }
                        }
                    }
                    let next = (current + 1) % spacing_tags.len();
                    buf.begin_user_action();
                    for t in spacing_tags {
                        if let Some(tag) = buf.tag_table().lookup(t) { buf.remove_tag(&tag, &start, &end); }
                    }
                    if let Some(tag) = buf.tag_table().lookup(spacing_tags[next]) {
                        buf.apply_tag(&tag, &start, &end);
                        // Persist line spacing to GSettings
                        let spacing_map = [("line-spacing-1.0", 1.0), ("line-spacing-1.15", 1.15), ("line-spacing-1.5", 1.5), ("line-spacing-2.0", 2.0)];
                        let val = spacing_map.iter().find(|(n,_)| *n == spacing_tags[next]).map(|(_,v)| *v).unwrap_or(1.15);
                        let s = gtk4::gio::Settings::new("org.tunaos.letters");
                        let _ = s.set_double("line-spacing", val);
                    }
                    buf.end_user_action();
                }
            });
            app.add_action(&a);
        }

        // ── Columns action ────────────────────────────────────────
        {
            let a = gtk::gio::SimpleAction::new("cycle-columns", None);
            let s = settings.clone();
            a.connect_activate(move |_, _| {
                let current = s.int("column-count").max(1);
                let next = if current >= 3 { 1 } else { current + 1 };
                let _ = s.set_int("column-count", next);
            });
            app.add_action(&a);
        }

        // Test-only state snapshot (#104): only registered when
        // GTK_OFFICE_TEST_MODE is set — see tables/src/window.rs for the
        // identical pattern. Content is extracted via the same
        // buffer->Document path save-to-docx already uses (letters_core's
        // Document already derives serde::Serialize, so no hand-written
        // JSON writer is needed here unlike Tables/Decks).
        if std::env::var_os("GTK_OFFICE_TEST_MODE").is_some() {
            let tv = tab_view.clone();
            let act = gtk::gio::SimpleAction::new("test-snapshot", None);
            act.connect_activate(move |_, _| {
                let Ok(path) = std::env::var("GTK_OFFICE_SNAPSHOT_PATH") else { return };
                let Some(buf) = active_buffer(&tv) else { return };
                let doc = crate::bridge::capture_from_buffer(&buf);
                if let Ok(json) = serde_json::to_string(&doc) {
                    let _ = std::fs::write(path, json);
                }
            });
            app.add_action(&act);
        }

        // Header/Footer edit dialog action
    {
        let tv = tab_view.clone();
        let a = gtk::gio::SimpleAction::new("edit-headers", None);
        a.connect_activate(move |_, _| {
            if let Some(_buf) = active_buffer(&tv) {
                // Find the PageContainer and show an edit dialog
                let page = tv.selected_page();
                if let Some(page) = page {
                    let child = page.child();
                    if let Some(pc) = child.first_child().and_then(|c| c.downcast::<crate::page_container::PageContainer>().ok()) {
                        show_header_footer_dialog(&pc);
                    }
                }
            }
        });
        app.add_action(&a);
    }

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
            // Ctrl+K belongs to the command palette (DESIGN-UI.md).
            app.set_accels_for_action("app.insertlink", &["<Primary><Shift>k"]);
        }

        // Insert Footnote: prompt for the note text, append it to the
        // buffer's footnote list, drop a superscript marker at the cursor.
        {
            let tv = tab_view.clone();
            let w = suite_win.window.clone();
            let a = gtk::gio::SimpleAction::new("insert-footnote", None);
            a.connect_activate(move |_, _| {
                let Some(buf) = active_buffer(&tv) else { return };
                let entry = gtk::Entry::builder()
                    .placeholder_text("Footnote text")
                    .activates_default(true)
                    .build();
                let dlg = adw::AlertDialog::builder()
                    .heading(suite_common::i18n("Insert Footnote"))
                    .build();
                dlg.set_extra_child(Some(&entry));
                dlg.add_response("cancel", &suite_common::i18n("Cancel"));
                dlg.add_response("insert", &suite_common::i18n("Insert"));
                dlg.set_response_appearance("insert", adw::ResponseAppearance::Suggested);
                dlg.set_default_response(Some("insert"));
                let buf2 = buf.clone();
                dlg.connect_response(None, move |d, resp| {
                    if resp != "insert" {
                        return;
                    }
                    let text = entry.text().to_string();
                    if text.is_empty() {
                        return;
                    }
                    let mut notes: Vec<String> = unsafe {
                        buf2.data::<Vec<String>>(crate::bridge::FOOTNOTES_KEY)
                            .map(|p| p.as_ref().clone())
                            .unwrap_or_default()
                    };
                    notes.push(text);
                    let idx = notes.len() - 1;
                    unsafe { buf2.set_data(crate::bridge::FOOTNOTES_KEY, notes) };
                    let mut pos = buf2
                        .selection_bounds()
                        .map(|(_, e)| e)
                        .unwrap_or_else(|| buf2.iter_at_mark(&buf2.get_insert()));
                    crate::bridge::insert_footnote_marker(&buf2, &mut pos, idx);
                    d.close();
                });
                dlg.present(Some(&w));
            });
            app.add_action(&a);
            app.set_accels_for_action("app.insert-footnote", &["<Primary><Alt>f"]);
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

        LettersWindow { window: suite_win.window, tab_view, stack, word_count_label, style_label, settings }
    }

    pub fn present(&self) { self.window.present(); }

    /// Open a document from a filesystem path into a new tab (used by
    /// CLI/file-manager launches via GApplication::open).
    pub fn open_path(&self, path: &str) {
        let (container, buf) = make_doc_widget(Some(&self.settings));
        if let Err(e) = crate::bridge::load_file_to_buffer(path, &buf) {
            eprintln!("open failed: {e}");
            return;
        }
        let td = TabData::new();
        td.0.borrow_mut().file = Some(std::path::PathBuf::from(path));
        tab_data_set(&container, td);
        let page = self.tab_view.append(&container);
        let name = std::path::Path::new(path)
            .file_name().map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        page.set_title(&name);
        page.set_tooltip(&name);
        // The selected-page notify fired at append, before the title was
        // set — refresh the window title now that it exists.
        self.window.set_title(Some(&format!("{name} — Letters")));
        page.set_needs_attention(false);
        suite_common::push_recent_file(&self.settings, path);
        self.stack.set_visible_child_name("editor");
        let wc = self.word_count_label.clone();
        let sl = self.style_label.clone();
        buf.connect_modified_changed({
            let p = page.clone();
            move |b| { p.set_needs_attention(b.is_modified()); }
        });
        connect_word_count(&buf, &wc);
        update_word_count(&buf, &wc);
        connect_style_readout(&buf, &sl);
        update_style_readout(&buf, &sl);
        self.tab_view.set_selected_page(&page);
    }

    /// Recover every snapshot orphaned by a crash, each into its own new
    /// tab — unlike Tables/Decks (one document per window), Letters can
    /// have several dirty tabs open at once, and a crash with N of them
    /// dirty leaves N orphans; recovering only the first would silently
    /// drop the rest. Call once, right after construction, before any
    /// explicit CLI-open. Returns the number of tabs recovered.
    pub fn recover_from_snapshot(&self) -> usize {
        let state_dir = autosave_state_dir();
        let orphan_ids = suite_common::autosave::find_orphaned_snapshots(&state_dir);
        let mut recovered = 0;
        for orphan_id in orphan_ids {
            let orphan = suite_common::autosave::AutosaveSlot::new(state_dir.clone(), orphan_id);
            let Some((bytes, meta)) = orphan.read() else { continue };
            let Ok(doc) = serde_json::from_slice::<letters_core::model::Document>(&bytes) else { continue };

            let (container, buf) = make_doc_widget(Some(&self.settings));
            crate::bridge::render_to_buffer(&doc, &buf);
            // render_to_buffer ends with buf.set_modified(false) (it's also
            // used for a normal file open); recovered content is unsaved
            // by definition, so mark it dirty right back so the close guard
            // protects it and autosave will re-snapshot it under this tab's
            // own (fresh) doc_id if this session crashes again too.
            buf.set_modified(true);

            let td = TabData::new();
            td.0.borrow_mut().file = meta.original_path.clone();
            tab_data_set(&container, td);
            let page = self.tab_view.append(&container);
            let name = meta.original_path.as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled Document".to_string());
            page.set_title(&format!("{name} (Recovered)"));
            page.set_needs_attention(true);
            self.stack.set_visible_child_name("editor");
            let wc = self.word_count_label.clone();
            let sl = self.style_label.clone();
            buf.connect_modified_changed({
                let p = page.clone();
                move |b| { p.set_needs_attention(b.is_modified()); }
            });
            connect_word_count(&buf, &wc);
            update_word_count(&buf, &wc);
            connect_style_readout(&buf, &sl);
            update_style_readout(&buf, &sl);

            let _ = orphan.clear();
            recovered += 1;
        }
        if recovered > 0 {
            self.window.set_title(Some("Letters — Recovered documents"));
        }
        recovered
    }

    fn register_actions(tv: &adw::TabView, st: &gtk4::Stack, wc: &gtk4::Label, sl: &gtk4::Label, win: &adw::ApplicationWindow, app: &adw::Application, settings: &gio::Settings) {
        // Word count: refresh on every buffer change and when switching tabs.
        {
            let wc = wc.clone();
            let sl = sl.clone();
            tv.connect_selected_page_notify(move |tv| {
                if let Some(buf) = active_buffer(tv) {
                    update_word_count(&buf, &wc);
                    update_style_readout(&buf, &sl);
                }
            });
        }
        // New document
        {
            let tv = tv.clone(); let st = st.clone(); let s = settings.clone(); let wc = wc.clone();
            let sl = sl.clone();
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
                connect_word_count(&buf, &wc);
                connect_style_readout(&buf, &sl);
                // AdwTabView only auto-selects a new page when it's the
                // first one; a second (or later) "New Document" while a tab
                // is already open otherwise leaves the old tab selected and
                // silently sends typing there instead.
                tv.set_selected_page(&page);
            });
            app.add_action(&a);
        }

        // Open file
        {
            let tv = tv.clone(); let st = st.clone(); let w = win.clone(); let s = settings.clone(); let wc = wc.clone();
            let sl = sl.clone();
            let a = gtk::gio::SimpleAction::new("open-file", None);
            a.connect_activate(move |_, _| {
                let tv = tv.clone(); let st = st.clone(); let w = w.clone(); let s = s.clone(); let wc = wc.clone();
                let sl = sl.clone();
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.md"); f.add_pattern("*.txt"); f.add_pattern("*.html"); f.add_pattern("*.docx"); f.add_pattern("*.odt");
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
                            let path_str = path.to_string_lossy().to_string();
                            if let Err(e) = crate::bridge::load_file_to_buffer(&path_str, &buf) {
                                eprintln!("open failed: {e}");
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
                            connect_word_count(&buf, &wc);
                            connect_style_readout(&buf, &sl);
                            tv.set_selected_page(&page);
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
            let s = settings.clone();
            a.connect_activate(move |_, _| {
                let tv = tv.clone(); let w = w.clone();
                let dlg = gtk::FileDialog::new();
                let f = gtk::FileFilter::new();
                f.add_pattern("*.md"); f.add_pattern("*.txt"); f.add_pattern("*.docx"); f.add_pattern("*.odt");
                f.set_name(Some("Documents"));
                let fl = gio::ListStore::new::<gtk::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                let default_ext = s.string("default-format");
                dlg.set_initial_name(Some(&format!("Untitled.{}", if default_ext.is_empty() { "odt" } else { &default_ext })));
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
                                            let config = crate::layout::LayoutConfig::from_settings(
                                                &gtk4::gio::Settings::new("org.tunaos.letters")
                                            );
                                            let ctx = gtk4::pango::Context::new();
                                            let pages = crate::layout::paginate(&buf, &config, &ctx);
                                            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                                            let page_breaks: Vec<usize> = pages.iter().skip(1).map(|p| {
                                                text[..p.start_offset as usize].lines().count()
                                            }).collect();
                                            let _ = crate::docx_bridge::write_buffer_to_docx_with_layout(
                                                &path_str, &buf, None, &page_breaks
                                            );
                                        } else {
                                            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
                                            let doc = crate::engine::Document::from_text(&text);
                                            let _ = crate::engine::write(&path_str, &doc);
                                        }
                                        buf.set_modified(false);
                                    }
                                    page.set_needs_attention(false);
                                    if let Some(name) = file.basename() { page.set_title(&name.display().to_string()); }
                                    if let Some(path) = file.path() {
                                        if let Some(s) = path.to_str() { page.set_tooltip(s); }
                                    }
                                    if let Some(td) = tab_data_get(&child) {
                                        td.0.borrow_mut().file = file.path();
                                    }
                                    clear_tab_autosave(&child);
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&a);
        }

        // ── Autosave: periodic per-tab crash-recovery snapshot ──────────
        // One tick covers every open tab. Serializes through
        // capture_from_buffer (the same model the bridge uses for a real
        // save) to JSON — snapshotting doesn't need a real file format,
        // just a lossless round-trip back into a buffer on recovery.
        {
            let atv = tv.clone();
            let a = gtk::gio::SimpleAction::new("autosave-now", None);
            a.connect_activate(move |_, _| { autosave_all_tabs(&atv); });
            app.add_action(&a);
        }
        {
            let atv = tv.clone();
            let interval = settings.int("auto-save-interval");
            if interval > 0 {
                glib::source::timeout_add_seconds_local(interval.max(10) as u32, move || {
                    autosave_all_tabs(&atv);
                    glib::ControlFlow::Continue
                });
            }
        }
    }
}

fn autosave_all_tabs(tv: &adw::TabView) {
    for i in 0..tv.n_pages() {
        let page = tv.nth_page(i);
        if !page.needs_attention() {
            continue;
        }
        let child = page.child();
        let (Some(td), Some(buf)) = (tab_data_get(&child), get_textview(&child).map(|tv| tv.buffer())) else {
            continue;
        };
        let doc = crate::bridge::capture_from_buffer(&buf);
        let Ok(bytes) = serde_json::to_vec(&doc) else { continue };
        let td = td.0.borrow();
        let meta = suite_common::autosave::SnapshotMeta {
            original_path: td.file.clone(),
            kind: "letters-json".to_string(),
        };
        let _ = td.autosave_slot.write(&bytes, &meta);
    }
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

/// Insert a suite fragment at the cursor: styled runs map onto the
/// editor's named tags; grids land as tab-separated lines (a real
/// cell-tagged table paste needs the buffer table support tracked in
/// PARITY's bridge gaps).
fn insert_fragment(buf: &gtk::TextBuffer, frag: &letters_core::fragment::Fragment) {
    use letters_core::fragment::Fragment;
    match frag {
        Fragment::Text(paras) => {
            for (i, p) in paras.iter().enumerate() {
                if i > 0 {
                    buf.insert_at_cursor("\n");
                }
                for run in &p.runs {
                    let mut tags: Vec<&str> = Vec::new();
                    if run.style.bold { tags.push("bold"); }
                    if run.style.italic { tags.push("italic"); }
                    if run.style.underline { tags.push("underline"); }
                    if run.style.strikethrough { tags.push("strikethrough"); }
                    if run.style.highlight { tags.push("highlight"); }
                    if run.style.code { tags.push("code"); }
                    let mut iter = buf.iter_at_mark(&buf.get_insert());
                    if tags.is_empty() {
                        buf.insert(&mut iter, &run.text);
                    } else {
                        buf.insert_with_tags_by_name(&mut iter, &run.text, &tags);
                    }
                }
            }
        }
        Fragment::Grid(_) => {
            buf.insert_at_cursor(&frag.to_plain());
        }
    }
}

fn update_word_count(buf: &gtk::TextBuffer, wc: &gtk4::Label) {
    let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
    let n = text.split_whitespace().count();
    wc.set_text(&format!("{} words", n));
}

fn connect_word_count(buf: &gtk::TextBuffer, wc: &gtk4::Label) {
    let wc = wc.clone();
    buf.connect_changed(move |b| update_word_count(b, &wc));
}

fn update_style_readout(buf: &gtk::TextBuffer, label: &gtk4::Label) {
    let mut iter = buf.iter_at_mark(&buf.get_insert());
    let mut tags = iter.tags();
    // At the end of a styled run the tag toggles off exactly at the
    // cursor; typing continues the preceding character's style, so read
    // that instead.
    if tags.is_empty() && iter.backward_char() {
        tags = iter.tags();
    }
    let names: Vec<String> = tags
        .iter()
        .filter_map(|t| t.name().map(|n| n.to_string()))
        .collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    label.set_text(&letters_core::model::style_readout(&refs));
}

fn connect_style_readout(buf: &gtk::TextBuffer, label: &gtk4::Label) {
    let l = label.clone();
    buf.connect_notify_local(Some("cursor-position"), move |b, _| {
        update_style_readout(b, &l);
    });
    let l = label.clone();
    buf.connect_changed(move |b| update_style_readout(b, &l));
    // Formatting changes don't move the cursor or emit `changed`; track
    // tag application directly.
    // apply-tag/remove-tag run before the default handler mutates the
    // buffer; defer the readout to idle so it sees the new state.
    let l = label.clone();
    buf.connect_apply_tag(move |b, _, _, _| {
        let b = b.clone();
        let l = l.clone();
        glib::idle_add_local_once(move || update_style_readout(&b, &l));
    });
    let l = label.clone();
    buf.connect_remove_tag(move |b, _, _, _| {
        let b = b.clone();
        let l = l.clone();
        glib::idle_add_local_once(move || update_style_readout(&b, &l));
    });
}

// ── Save logic ───────────────────────────────────────────────────────

/// Save `page`'s document to its already-known file path, if it has one.
/// Returns `false` (no-op) for a never-saved tab — callers fall back to a
/// Save As prompt in that case. Clears both the buffer's GTK-level modified
/// flag and the tab's `needs-attention` so later edits re-trigger dirty
/// tracking correctly (leaving the GTK flag stuck `true` after a save would
/// mean the next edit doesn't re-fire `modified-changed`, silently defeating
/// the close guard).
fn save_page(page: &adw::TabPage) -> bool {
    let child = page.child();
    let Some(td) = tab_data_get(&child) else { return false };
    let path = td.0.borrow().file.clone();
    let Some(path) = path else { return false };
    // All formats route through letters-core via the bridge; formatting
    // survives in both markdown and docx now.
    if let Some(buf) = get_textview(&child).map(|tv| tv.buffer()) {
        let path_str = path.to_string_lossy().to_string();
        if let Err(e) = crate::bridge::save_buffer_to_file(&buf, &path_str) {
            eprintln!("save failed: {e}");
        }
        let settings = gio::Settings::new("org.tunaos.letters");
        suite_common::push_recent_file(&settings, &path_str);
        buf.set_modified(false);
    }
    page.set_needs_attention(false);
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        page.set_title(name);
    }
    let _ = td.0.borrow().autosave_slot.clear();
    true
}

/// Clear a tab's autosave slot — call on Discard as well as on save,
/// since a discarded tab shouldn't be offered back as "recovered" either.
fn clear_tab_autosave(child: &gtk::Widget) {
    if let Some(td) = tab_data_get(child) {
        let _ = td.0.borrow().autosave_slot.clear();
    }
}

fn do_save(tv: &adw::TabView, _stack: &gtk4::Stack) {
    if let Some(page) = tv.selected_page() {
        if !page.needs_attention() { return; }
        save_page(&page);
    }
}

/// Save every dirty page in `queue`, prompting Save As for any tab that has
/// never been saved to a path, then close the window. A cancelled Save As
/// aborts the whole close (window stays open, remaining queue is dropped) —
/// the safe default, matching the per-tab close guard's Cancel behavior.
fn close_all_dirty_pages(
    win: adw::ApplicationWindow,
    tv: adw::TabView,
    mut queue: std::collections::VecDeque<adw::TabPage>,
    force_close: Rc<std::cell::Cell<bool>>,
) {
    while let Some(page) = queue.pop_front() {
        if save_page(&page) {
            continue;
        }
        let dlg = gtk::FileDialog::new();
        let f = gtk::FileFilter::new();
        f.add_pattern("*.md"); f.add_pattern("*.txt"); f.add_pattern("*.docx"); f.add_pattern("*.odt");
        f.set_name(Some("Documents"));
        let fl = gio::ListStore::new::<gtk::FileFilter>();
        fl.append(&f);
        dlg.set_filters(Some(&fl));
        dlg.set_initial_name(Some(&page.title()));
        let win2 = win.clone();
        let tv2 = tv.clone();
        let force_close2 = force_close.clone();
        let page2 = page.clone();
        dlg.save(Some(&win), None::<&gio::Cancellable>, move |result| {
            let Ok(file) = result else { return };
            let Some(path) = file.path() else { return };
            let child = page2.child();
            if let Some(buf) = get_textview(&child).map(|tv| tv.buffer()) {
                let path_str = path.to_string_lossy().to_string();
                if let Err(e) = crate::bridge::save_buffer_to_file(&buf, &path_str) {
                    eprintln!("save failed: {e}");
                }
                buf.set_modified(false);
            }
            page2.set_needs_attention(false);
            if let Some(name) = file.basename() { page2.set_title(&name.display().to_string()); }
            if let Some(td) = tab_data_get(&child) { td.0.borrow_mut().file = file.path(); }
            clear_tab_autosave(&child);
            close_all_dirty_pages(win2, tv2, queue, force_close2);
        });
        return;
    }
    force_close.set(true);
    win.close();
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── crash-recovery doc ids (pure, no GTK) ─────────────────────────

    #[test]
    fn next_doc_id_is_unique() {
        let a = next_doc_id();
        let b = next_doc_id();
        let c = next_doc_id();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn next_doc_id_has_pid_prefix_and_incrementing_counter() {
        let prefix = format!("{}-", std::process::id());
        let a = next_doc_id();
        let b = next_doc_id();
        assert!(a.starts_with(&prefix), "unexpected id {a}");
        assert!(b.starts_with(&prefix), "unexpected id {b}");
        let n_a: u64 = a.rsplit('-').next().unwrap().parse().unwrap();
        let n_b: u64 = b.rsplit('-').next().unwrap().parse().unwrap();
        assert_eq!(n_b, n_a + 1);
    }

    #[test]
    fn autosave_state_dir_points_into_letters_subdir() {
        let dir = autosave_state_dir();
        assert_eq!(dir.file_name().and_then(|s| s.to_str()), Some("letters"));
    }

    #[test]
    fn autosave_state_dir_env_fallbacks() {
        // One test so the process-global env mutations below never race
        // another test's view of HOME/XDG_STATE_HOME (cargo runs the tests
        // of a binary in one process). Restored on drop even if an assert
        // fails, so later tests keep a clean environment.
        struct Restore(Option<std::ffi::OsString>, Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match &self.0 {
                    Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                    None => std::env::remove_var("XDG_STATE_HOME"),
                }
                match &self.1 {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
        let _restore = Restore(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"));

        // XDG_STATE_HOME wins when set.
        std::env::set_var("XDG_STATE_HOME", "/custom/state");
        std::env::set_var("HOME", "/custom/home");
        assert_eq!(
            autosave_state_dir(),
            std::path::PathBuf::from("/custom/state/letters")
        );

        // Falls back to $HOME/.local/state without XDG_STATE_HOME.
        std::env::remove_var("XDG_STATE_HOME");
        std::env::set_var("HOME", "/custom/home");
        assert_eq!(
            autosave_state_dir(),
            std::path::PathBuf::from("/custom/home/.local/state/letters")
        );

        // Last resort is /tmp when neither is set.
        std::env::remove_var("XDG_STATE_HOME");
        std::env::remove_var("HOME");
        assert_eq!(
            autosave_state_dir(),
            std::path::PathBuf::from("/tmp/letters")
        );
    }
}
