// doc_tab.rs — what a Letters document tab is, and how one is built.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of window.rs (#168, #285). window.rs had grown past the 1,800-line
// ceiling the release contract enforces, and this is the part of it that is
// not window wiring: per-tab identity, the editor/page widget pair each tab
// holds, and the reporting for a tab that could not be filled because its file
// would not open.
//
// Kept as one module because these answer one question — what a document tab
// consists of — rather than to reach a line count.

use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

use crate::actions::{connect_list_continuation, connect_markdown_macros, register_formatting_tags};
use crate::page_container::PageContainer;
use crate::window::insert_fragment;

// ── Crash-recovery snapshots ─────────────────────────────────────────────
// One AutosaveSlot per tab (not per window, unlike Tables/Decks): each tab
// is its own document, so each needs its own doc_id and its own slot.
static NEXT_DOC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn next_doc_id() -> String {
    let n = NEXT_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

pub(crate) fn autosave_state_dir() -> std::path::PathBuf {
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
pub(crate) struct TabData(pub(crate) Rc<RefCell<TabDataInner>>);
type TabDataInner = letters_core::DocumentSession;
impl TabData {
    pub(crate) fn new() -> Self {
        TabData(Rc::new(RefCell::new(TabDataInner::new(Rc::new(
            suite_common::autosave::AutosaveSlot::new(autosave_state_dir(), next_doc_id()),
        )))))
    }
}
pub(crate) fn tab_data_set(w: &impl IsA<gtk::Widget>, d: TabData) { unsafe { w.upcast_ref::<gtk::Widget>().set_data("tab-data", d); } }
pub(crate) fn tab_data_get(w: &gtk::Widget) -> Option<TabData> { unsafe { w.data::<TabData>("tab-data").map(|p| p.as_ref().clone()) } }

// ── Make a tab's document widget ──────────────────────────────────────

/// Body text for a failed document open.
///
/// Kept separate from the dialog so the wording is unit-testable: the phrase
/// matters, because a file the app cannot read must say so in words a user
/// recognises as a failure rather than leaving them with a blank editor.
pub(crate) fn open_failure_message(path: &str, error: &str) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    format!("{name}\n\n{error}")
}

/// Tell the user a document could not be opened.
///
/// Letters previously only wrote these to stderr. In the file-dialog path it
/// then carried on and built the tab anyway, so an unreadable file produced an
/// empty editor titled with that file's name and pointed at that file's path —
/// one Ctrl+S away from overwriting the original with nothing (#447). Tables
/// has always shown a dialog here; this brings Letters in line.
pub(crate) fn report_open_failure(parent: Option<&adw::ApplicationWindow>, path: &str, error: &str) {
    suite_common::show_error_dialog(
        parent,
        &suite_common::i18n("Could not open document"),
        &open_failure_message(path, error),
    );
}

/// Show a freshly loaded document's page setup in its page view.
///
/// `capture_from_buffer` reads header, footer and page geometry off the
/// buffer, so saving is already correct without this — but until the
/// container is told, the page renders at default geometry with empty
/// header/footer areas, so a document that has them looks like it does not
/// (#438). Every open route needs this, which is why it is a function rather
/// than a few lines inlined at one of them.
pub(crate) fn apply_page_setup_from_buffer(container: &PageContainer, buf: &gtk::TextBuffer) {
    let (header, footer) = crate::bridge::buffer_header_footer(buf);
    container.set_header_text(&header.unwrap_or_default());
    container.set_footer_text(&footer.unwrap_or_default());
    // Geometry is optional: a markdown file has none, and a document without
    // it should keep the container's defaults rather than be forced to zero.
    if let Some(page) = crate::bridge::buffer_page_geometry(buf) {
        container.set_page_size(page.width_pt, page.height_pt);
        container.set_margins(
            page.margin_top_pt,
            page.margin_bottom_pt,
            page.margin_left_pt,
            page.margin_right_pt,
        );
        container.set_column_count(page.columns.max(1) as u32);
    }
}

pub(crate) fn make_doc_widget(settings: Option<&gio::Settings>) -> (PageContainer, gtk::TextBuffer) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_container::PageContainer;

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

    // ── page setup reaches the view (#438) ────────────────────────────────
    // Saving was fixed separately; these cover the other direction, that an
    // opened document's header/footer/geometry are actually shown.

    use suite_common::gtk_test::run as gtk_test;

    fn geometry() -> letters_core::model::PageGeometry {
        letters_core::model::PageGeometry {
            width_pt: 612.0,
            height_pt: 792.0,
            margin_top_pt: 36.0,
            margin_bottom_pt: 48.0,
            margin_left_pt: 54.0,
            margin_right_pt: 60.0,
            columns: 2,
            column_gap_pt: 18.0,
        }
    }

    #[test]
    fn header_and_footer_reach_the_page_view() {
        gtk_test(|| {
            let container = PageContainer::new();
            let buf = gtk::TextBuffer::new(None);
            let mut doc = letters_core::model::Document::from_plain_text("body");
            doc.header = Some("Quarterly Report".into());
            doc.footer = Some("Page {page}".into());
            crate::bridge::render_to_buffer(&doc, &buf);

            apply_page_setup_from_buffer(&container, &buf);
            assert_eq!(container.header_text(), "Quarterly Report");
            assert_eq!(container.footer_text(), "Page {page}");
        });
    }

    #[test]
    fn page_geometry_reaches_the_page_view() {
        gtk_test(|| {
            let container = PageContainer::new();
            let buf = gtk::TextBuffer::new(None);
            let mut doc = letters_core::model::Document::from_plain_text("body");
            doc.page = Some(geometry());
            crate::bridge::render_to_buffer(&doc, &buf);

            apply_page_setup_from_buffer(&container, &buf);
            assert_eq!(container.page_size(), (612.0, 792.0), "page size must follow the document");
            assert_eq!(container.margins(), (36.0, 48.0, 54.0, 60.0), "margins too");
            assert_eq!(container.column_count(), 2, "and the column count");
        });
    }

    /// A document with no header must clear the view, not leave the previous
    /// document's text showing in a reused container.
    #[test]
    fn opening_a_document_without_a_header_clears_the_view() {
        gtk_test(|| {
            let container = PageContainer::new();
            container.set_header_text("Left over from before");
            container.set_footer_text("Also stale");

            let buf = gtk::TextBuffer::new(None);
            let doc = letters_core::model::Document::from_plain_text("no chrome");
            crate::bridge::render_to_buffer(&doc, &buf);

            apply_page_setup_from_buffer(&container, &buf);
            assert_eq!(container.header_text(), "", "stale header must be cleared");
            assert_eq!(container.footer_text(), "", "stale footer must be cleared");
        });
    }

    /// A document with no geometry keeps the container's defaults rather than
    /// being forced to a zero-sized page.
    #[test]
    fn absent_geometry_leaves_container_defaults_alone() {
        gtk_test(|| {
            let container = PageContainer::new();
            container.set_page_size(595.0, 842.0);
            let before = container.page_size();

            let buf = gtk::TextBuffer::new(None);
            let doc = letters_core::model::Document::from_plain_text("markdown has no geometry");
            crate::bridge::render_to_buffer(&doc, &buf);

            apply_page_setup_from_buffer(&container, &buf);
            assert_eq!(container.page_size(), before, "defaults must survive");
        });
    }

    // ── failed opens are reported, not swallowed (#447) ───────────────────

    /// The message names the file, so a user with several documents open can
    /// tell which one failed.
    #[test]
    fn open_failure_message_names_the_file_and_the_reason() {
        let msg = open_failure_message("/home/u/docs/quarterly.docx", "not a zip archive");
        assert!(msg.contains("quarterly.docx"), "should name the file: {msg}");
        assert!(msg.contains("not a zip archive"), "should give the reason: {msg}");
        assert!(!msg.contains("/home/u/docs"), "the full path is noise in a dialog: {msg}");
    }

    /// A path with no file name still produces something addressed to a human
    /// rather than an empty dialog.
    #[test]
    fn open_failure_message_falls_back_to_the_whole_path() {
        let msg = open_failure_message("/", "is a directory");
        assert!(msg.contains("is a directory"));
        assert!(!msg.trim().is_empty());
    }

    /// The corpus journey in #447 looks for a visible node whose name contains
    /// one of a set of failure phrases. The heading has to keep matching one of
    /// them, so pin it here rather than discovering a rename in a GUI run.
    #[test]
    fn open_failure_heading_matches_the_corpus_journey_phrases() {
        let heading = suite_common::i18n("Could not open document").to_lowercase();
        let accepted = [
            "could not", "cannot open", "failed to",
            "unable to", "unsupported", "invalid file", "error opening",
        ];
        assert!(
            accepted.iter().any(|phrase| heading.contains(phrase)),
            "heading {heading:?} matches none of the phrases the corpus test accepts"
        );
    }
}
