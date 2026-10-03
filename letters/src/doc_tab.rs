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

use crate::actions::{connect_markdown_macros, register_formatting_tags};
use crate::page_container::PageContainer;
use crate::insert::insert_fragment;

// ── Crash-recovery snapshots ─────────────────────────────────────────────
// One AutosaveSlot per tab (not per window, unlike Tables/Decks): each tab
// is its own document, so each needs its own doc_id and its own slot.
static NEXT_DOC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn next_doc_id() -> String {
    let n = NEXT_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

pub(crate) fn autosave_state_dir() -> std::path::PathBuf {
    // XDG state dir, never a fixed name in /tmp (#829): see the shared helper.
    suite_common::autosave::state_dir("letters")
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
        &suite_common::i18n("Could Not Open File"),
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
    refresh_print_layout(container, buf);
}

/// Layout options for a tab: the container's page setup is the page for a
/// document that carries none of its own.
pub(crate) fn layout_options(container: &PageContainer) -> letters_core::layout::LayoutOptions {
    let (width_pt, height_pt) = container.page_size();
    let (margin_top_pt, margin_bottom_pt, margin_left_pt, margin_right_pt) = container.margins();
    letters_core::layout::LayoutOptions {
        page: letters_core::model::PageGeometry {
            width_pt,
            height_pt,
            margin_top_pt,
            margin_bottom_pt,
            margin_left_pt,
            margin_right_pt,
            columns: container.column_count().clamp(1, 255) as u8,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Lay the tab's document out again for its page view.
pub(crate) fn refresh_print_layout(container: &PageContainer, buf: &gtk::TextBuffer) {
    let Some(view) = container.page_view() else { return };
    let (typeset, starts) = typeset_with_starts(container, buf);
    view.set_typeset(typeset, starts);
}

/// The tab's document laid out into pages (ADR 0010): what Print Layout
/// shows, and what Print and Export as PDF draw — one layout for all three.
pub(crate) fn typeset_for(container: &PageContainer, buf: &gtk::TextBuffer) -> letters_core::layout::pango::Typeset {
    typeset_with_starts(container, buf).0
}

/// `typeset_for`, plus where each laid-out paragraph starts in the buffer,
/// which the page view needs to edit it.
fn typeset_with_starts(container: &PageContainer, buf: &gtk::TextBuffer) -> (letters_core::layout::pango::Typeset, Vec<usize>) {
    let (doc, starts) = match crate::live::of(buf) {
        Some(m) => m.borrow_mut().snapshot(buf),
        None => crate::bridge::capture_with_starts(buf),
    };
    let mut typeset = letters_core::layout::pango::Typeset::new(doc, layout_options(container));
    typeset.set_image_loader(crate::page_view::load_image);
    (typeset, starts)
}

/// Put the selection on `widget`'s clipboard as the suite fragment, with
/// HTML and plain text beside it. False with nothing selected.
pub(crate) fn copy_selection(widget: &gtk::Widget, buf: &gtk::TextBuffer) -> bool {
    let Some((start, end)) = buf.selection_bounds() else { return false };
    let frag = crate::bridge::selection_fragment(buf, start.offset() as usize, end.offset() as usize);
    let provider = suite_common::clipboard::provider(letters_core::fragment::MIME, &frag.to_json(), &frag.to_html(), &frag.to_plain());
    let _ = widget.clipboard().set_content(Some(&provider));
    true
}

/// Cross-app clipboard (DESIGN-UI) on `widget`, which edits `buf`: Ctrl+C
/// offers the suite fragment (styled runs) alongside HTML and plain text;
/// Ctrl+V prefers it. Capture phase so it supersedes the widget's own
/// plain-text handling only when suite content is involved.
pub(crate) fn connect_suite_clipboard(widget: &gtk::Widget, buf: &gtk::TextBuffer) {
    {
        let buf = buf.clone();
        let ed = widget.clone();
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(move |_, keyval, _code, mods| {
            let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            if ctrl && keyval == gtk4::gdk::Key::c {
                if copy_selection(&ed, &buf) {
                    return gtk4::glib::Propagation::Stop;
                }
                return gtk4::glib::Propagation::Proceed;
            }
            // Cut is a copy, then a model edit deleting the selection: the
            // fragment keeps its formatting, and one undo puts it back.
            if ctrl && keyval == gtk4::gdk::Key::x && crate::live::of(&buf).is_some() {
                if copy_selection(&ed, &buf) {
                    crate::insert::delete_selection(&buf);
                    crate::live::sync_actions(&buf);
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
        widget.add_controller(key);
    }
}

/// Selection format popover on `widget` (the page
/// view): context reveals capability (DESIGN-UI §1). Shown while `buf` has
/// a selection *and* `widget` is the view showing it, pointing at the
/// selection's start (`locate`: buffer offset to a rectangle in `widget`).
/// Non-autohide so it never steals focus from the editor; buttons fire the
/// same app actions as the toolbar.
fn connect_selection_popover(
    widget: &gtk::Widget,
    buf: &gtk::TextBuffer,
    locate: impl Fn(usize) -> Option<gtk4::gdk::Rectangle> + 'static,
) {
    let pop = gtk::Popover::new();
    pop.set_parent(widget);
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
        ("insert-link-symbolic", "Insert Link", "app.insertlink"),
    ] {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.set_tooltip_text(Some(tooltip));
        b.set_action_name(Some(action));
        row.append(&b);
    }
    pop.set_child(Some(&row));
    let (w, pop2) = (widget.downgrade(), pop.clone());
    buf.connect_mark_set(move |buf, _iter, mark| {
        let name = mark.name();
        if !matches!(name.as_deref(), Some("insert") | Some("selection_bound")) {
            return;
        }
        let shown = w.upgrade().is_some_and(|w| w.is_mapped());
        match buf.selection_bounds().filter(|_| shown) {
            Some((start, _)) => {
                if let Some(rect) = locate(start.offset().max(0) as usize) {
                    pop2.set_pointing_to(Some(&rect));
                    if !pop2.is_visible() {
                        pop2.popup();
                    }
                }
            }
            None if pop2.is_visible() => pop2.popdown(),
            None => {}
        }
    });
    let pop3 = pop.clone();
    widget.connect_destroy(move |_| pop3.unparent());
}

/// The body font Preferences names (the `font` key, a Pango font
/// description such as "Liberation Serif 12"), for a new document; `None`
/// when the key is empty or names no family.
pub(crate) fn preferred_base_font(font: &str) -> Option<letters_core::model::BaseFont> {
    let desc = gtk4::pango::FontDescription::from_string(font.trim());
    let family = desc.family().map(|f| f.to_string()).filter(|f| !f.is_empty())?;
    let size_hp = (desc.size() > 0)
        .then(|| (f64::from(desc.size()) / f64::from(gtk4::pango::SCALE) * 2.0).round() as u16)
        .filter(|hp| *hp > 0);
    Some(letters_core::model::BaseFont { family: Some(family), size_hp })
}

pub(crate) fn make_doc_widget(settings: Option<&gio::Settings>) -> (PageContainer, gtk::TextBuffer) {
    let buffer = gtk::TextBuffer::new(None);
    register_formatting_tags(&buffer);
    // A new document's body font is the one Preferences names (#1428); it
    // is the document's own, so the page shows it and every save writes it.
    // An opened file replaces it with its own as it loads. The key used to
    // style only the Draft editor, which is no longer shown.
    if let Some(base) = settings.and_then(|s| preferred_base_font(&s.string("font"))) {
        crate::bridge::set_base_font(&buffer, base);
    }
    // The tab's document: the live model is the source of truth, the
    // buffer what formatting actions edit, and its history the tab's undo
    // (live.rs).
    let live = crate::live::LiveModel::attach(&buffer);
    connect_markdown_macros(&buffer);
    // Transparent scrolled windows, so PageContainer's backdrop shows
    // around the pages.
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_string("scrolledwindow { background: transparent; }");
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &css_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    }
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
    crate::review_ui::apply_to(&buffer, crate::review_ui::tracking());

    // Print Layout is the one editing surface (ADR 0010, #1202): the page
    // view edits the buffer, and the container holds it for the code that
    // only has the tab's widget.
    let container = PageContainer::new();
    if let Some(s) = settings {
        container.load_from_settings(s);
    }
    container.set_buffer(&buffer);
    let page_view = container.attach_page_view();
    crate::page_edit::make_editable(&page_view, &buffer);
    {
        let pv = page_view.clone();
        let locate = move |start: usize| {
            pv.caret_rect(start).map(|(x, y, h)| gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, h.ceil() as i32))
        };
        connect_selection_popover(page_view.upcast_ref(), &buffer, locate.clone());
        crate::chips_ui::attach(page_view.upcast_ref(), &buffer, locate);
    }
    // Focus the page view whenever it is shown, or keystrokes fall through
    // to the window's search bar.
    page_view.connect_map(|v| {
        let v = v.clone();
        glib::idle_add_local_once(move || { v.grab_focus(); });
    });
    connect_suite_clipboard(page_view.upcast_ref(), &buffer);
    container.set_zoom(container.zoom_level());
    container.set_vexpand(true); container.set_hexpand(true);
    refresh_print_layout(&container, &buffer);
    // Zoom via Ctrl+Scroll. In the capture phase, because the scrolled
    // window inside would otherwise take the scroll first. It used to be on
    // the Draft editor, and after that view was retired nothing reached it.
    {
        let pc = container.clone();
        let s = settings.cloned();
        let scroll_ctrl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll_ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
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
        container.add_controller(scroll_ctrl);
    }
    // Lay the pages out again as soon as the main loop is idle after an
    // edit (edits in one event coalesce). The page view keeps its typeset
    // and re-shapes only the paragraphs the edit changed (ADR 0010 stage
    // 3c).
    {
        let pc = container.clone();
        let timer = std::rc::Rc::new(std::cell::RefCell::new(None::<glib::SourceId>));
        let b2 = buffer.clone();
        buffer.connect_changed(move |_| {
            if let Some(id) = timer.borrow_mut().take() { id.remove(); }
            let (buf, pc, t2, live) = (b2.clone(), pc.clone(), timer.clone(), live.clone());
            let id = glib::idle_add_local(move || {
                match pc.page_view().filter(|v| v.page_count() > 0) {
                    Some(view) => {
                        let (doc, starts) = live.borrow_mut().snapshot(&buf);
                        view.update_document(doc, layout_options(&pc), starts);
                    }
                    None => refresh_print_layout(&pc, &buf),
                }
                t2.borrow_mut().take();
                glib::ControlFlow::Break
            });
            *timer.borrow_mut() = Some(id);
        });
    }
    (container, buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_font_preference_names_a_family_and_maybe_a_size() {
        let base = |font: &str| preferred_base_font(font).map(|b| (b.family, b.size_hp));
        assert_eq!(base("Liberation Serif 12"), Some((Some("Liberation Serif".into()), Some(24))));
        assert_eq!(base("Cantarell"), Some((Some("Cantarell".into()), None)));
        assert_eq!(base("DejaVu Sans 10.5"), Some((Some("DejaVu Sans".into()), Some(21))));
        assert_eq!(base(""), None, "an empty key leaves the document's default");
        assert_eq!(base("   "), None);
    }
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
        // Increasing, not necessarily by one: `next_doc_id_is_unique` runs
        // in parallel and takes ids from the same counter (this raced).
        assert!(n_b > n_a, "{n_a} then {n_b}");
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

        // Neither set: the account's home from the password database, and
        // never a fixed directory in /tmp that another user could own (#829).
        std::env::remove_var("XDG_STATE_HOME");
        std::env::remove_var("HOME");
        let dir = autosave_state_dir();
        assert!(dir.ends_with(".local/state/letters"), "{}", dir.display());
        assert!(!dir.starts_with("/tmp"), "{}", dir.display());
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
        let heading = suite_common::i18n("Could Not Open File").to_lowercase();
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
