//! text_editor.rs — editing a slide text box in place, as rich text
//! (DCK-007). Split out of canvas_keys.rs, which keeps the canvas's keys.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! A double-click opens a text session on the controller
//! (`DecksController::begin_text_session`): a working copy of the box with
//! its own undo, edited only through decks-core's range commands
//! (`rich_text::TextSession`). This module shows that session — a native
//! `GtkTextView` whose buffer and tags mirror the working copy, a
//! formatting bar, and the canvas's preview with its caret or selection —
//! and turns what the person does into session commands. It holds no text
//! of its own: every buffer insertion or deletion is the same replacement
//! on the session, every format is a session format, and the buffer is
//! re-tagged from the session after each.
//!
//! The boundary is explicit: Ctrl+Enter or Apply commits the session as one
//! document step; Escape or Cancel drops it without trace; moving focus to
//! the formatting bar is still the session. Leaving it any other way (a
//! click outside, an application shortcut such as Save or Present)
//! commits first, so the command acts on the committed text; a commit that
//! cannot land keeps the editor open with the reason. Input-method preedit
//! never reaches the buffer, so it can never be committed. Ctrl+Z inside
//! the session is the session's own undo; the deck's is refused until the
//! session ends.

use gtk4::{self as gtk, gdk, glib, pango, prelude::*};
use std::cell::Cell;
use std::rc::{Rc, Weak};

use crate::canvas::{canvas_to_slide, hit_test_object};
use crate::canvas_keys::EditorHandles;
use decks_core::engine::{ParaAlign, RunStyle, Slide, SlideObject};
use decks_core::format::{FormatEdit, ListKind};
use decks_core::rich_text::{byte_to_char, char_to_byte, RichText};
use decks_core::DecksController;

/// The text colours the formatting bar offers, by name.
const PALETTE: [(&str, &str); 10] = [
    ("Black", "000000"),
    ("Dark Grey", "595959"),
    ("Red", "c00000"),
    ("Orange", "ed7d31"),
    ("Yellow", "ffc000"),
    ("Green", "70ad47"),
    ("Blue", "4472c4"),
    ("Dark Blue", "1f3864"),
    ("Purple", "7030a0"),
    ("White", "ffffff"),
];

const ALIGNS: [(&str, &str, ParaAlign); 4] = [
    ("format-justify-left-symbolic", "Align Paragraph Left", ParaAlign::Left),
    ("format-justify-center-symbolic", "Centre Paragraph", ParaAlign::Center),
    ("format-justify-right-symbolic", "Align Paragraph Right", ParaAlign::Right),
    ("format-justify-fill-symbolic", "Justify Paragraph", ParaAlign::Justify),
];

/// The formatting bar's controls, shown from the session's selection.
struct Controls {
    family: gtk::DropDown,
    families: gtk::StringList,
    size: gtk::SpinButton,
    bold: gtk::ToggleButton,
    italic: gtk::ToggleButton,
    underline: gtk::ToggleButton,
    strike: gtk::ToggleButton,
    aligns: Vec<gtk::ToggleButton>,
    list: gtk::DropDown,
    space_before: gtk::SpinButton,
    space_after: gtk::SpinButton,
}

struct Editor {
    ctl: Rc<DecksController>,
    window: glib::WeakRef<gtk::Window>,
    canvas: glib::WeakRef<gtk::DrawingArea>,
    popover: gtk::Popover,
    view: gtk::TextView,
    scrolled: gtk::ScrolledWindow,
    /// An error or a paste disclosure, shown above the buttons.
    message: gtk::Label,
    controls: Controls,
    refresh: Rc<dyn Fn()>,
    /// The box being edited, in slide units, for placing the editor.
    bounds: Cell<(f64, f64, f64, f64)>,
    /// Set while the buffer or the controls are written from the session,
    /// so their signals don't write it straight back.
    syncing: Cell<bool>,
    /// An input method is composing: its keys are its own.
    preedit: Cell<bool>,
    /// Give the canvas the keyboard back when the editor closes.
    restore_focus: Cell<bool>,
    /// The last key event handled (time, key), so the window's and the
    /// popover's controllers never both act on one press.
    last_key: Cell<Option<(u32, gdk::Key)>>,
    /// The editor's own menus, whose keys are theirs.
    menus: std::cell::RefCell<Vec<gtk::Popover>>,
}

fn toggle(icon: &str, label: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::builder().icon_name(icon).tooltip_text(label).build();
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

fn button(icon: &str, label: &str) -> gtk::Button {
    let b = gtk::Button::builder().icon_name(icon).tooltip_text(label).build();
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

fn linked(widgets: &[&gtk::Widget]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("linked");
    for w in widgets {
        b.append(*w);
    }
    b
}

fn labelled_spin(label: &str, lo: f64, hi: f64, step: f64) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(lo, hi, step);
    s.set_tooltip_text(Some(label));
    s.update_property(&[gtk::accessible::Property::Label(label)]);
    s
}

/// Relative luminance of `RRGGBB`, 0 (black) to 1 (white).
fn luminance(hex: &str) -> Option<f64> {
    let c = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok().map(|v| v as f64 / 255.0);
    Some(0.299 * c(0)? + 0.587 * c(2)? + 0.114 * c(4)?)
}

/// The buffer tag named `name`, made by `make` the first time.
fn tag(table: &gtk::TextTagTable, name: &str, make: impl FnOnce() -> gtk::TextTag) -> gtk::TextTag {
    if let Some(t) = table.lookup(name) {
        return t;
    }
    let t = make();
    table.add(&t);
    t
}

/// The tags showing `style` in the editor. Sizes are shown within a
/// readable band and colours that would vanish against the editor get a
/// contrasting ground: the slide preview shows them exactly.
fn tags_for(table: &gtk::TextTagTable, style: &RunStyle, editor_ink: f64) -> Vec<gtk::TextTag> {
    let mut out = Vec::new();
    if style.bold {
        out.push(tag(table, "bold", || gtk::TextTag::builder().name("bold").weight(700).build()));
    }
    if style.italic {
        out.push(tag(table, "italic", || gtk::TextTag::builder().name("italic").style(pango::Style::Italic).build()));
    }
    if style.underline {
        out.push(tag(table, "underline", || gtk::TextTag::builder().name("underline").underline(pango::Underline::Single).build()));
    }
    if style.strikethrough {
        out.push(tag(table, "strikethrough", || gtk::TextTag::builder().name("strikethrough").strikethrough(true).build()));
    }
    if let Some(family) = style.font_family.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        let name = format!("family:{family}");
        out.push(tag(table, &name, || gtk::TextTag::builder().name(name.as_str()).family(family).build()));
    }
    if let Some(hp) = style.font_size_hp {
        let name = format!("size:{hp}");
        let pt = (hp as f64 / 2.0).clamp(9.0, 32.0);
        out.push(tag(table, &name, || gtk::TextTag::builder().name(name.as_str()).size_points(pt).build()));
    }
    if let Some(hex) = style.color.as_deref().map(|c| c.trim_start_matches('#')).filter(|c| c.len() == 6) {
        let lum = luminance(hex).unwrap_or(0.0);
        // The editor's own ground is the opposite of its ink.
        let unreadable = (lum - (1.0 - editor_ink)).abs() < 0.35;
        let name = format!("color:{hex}:{unreadable}");
        out.push(tag(table, &name, || {
            let b = gtk::TextTag::builder().name(name.as_str()).foreground(format!("#{hex}"));
            if unreadable {
                b.background(if lum > 0.5 { "#3d3d3d" } else { "#e6e6e6" }).build()
            } else {
                b.build()
            }
        }));
    }
    out
}

/// Tag the whole buffer from `rt`'s runs. The buffer holds exactly
/// `rt.text`.
fn retag(buffer: &gtk::TextBuffer, rt: &RichText, editor_ink: f64) {
    let (start, end) = buffer.bounds();
    buffer.remove_all_tags(&start, &end);
    let table = buffer.tag_table();
    let mut at = 0i32;
    for run in &rt.runs {
        let n = run.text.chars().count() as i32;
        if n > 0 {
            let (a, b) = (buffer.iter_at_offset(at), buffer.iter_at_offset(at + n));
            for t in tags_for(&table, &run.style, editor_ink) {
                buffer.apply_tag(&t, &a, &b);
            }
        }
        at += n;
    }
}

/// The session's working copy of the box on `slide`, for the canvas to
/// draw in its place with the caret or selection.
pub(crate) fn canvas_preview(ctl: &DecksController, slide: &Slide) -> Option<crate::canvas::TextPreview> {
    let (slide_id, object_id, working, range) = ctl.text_session(|s| (s.slide_id, s.object_id, s.working().clone(), s.range()))?;
    if slide.ids.slide != slide_id {
        return None;
    }
    let index = slide.object_index(object_id)?;
    let mut object = slide.objects.get(index)?.clone();
    working.write_to(&mut object);
    Some(crate::canvas::TextPreview { index, object, range })
}

impl Editor {
    fn editor_ink(&self) -> f64 {
        let c = self.view.color();
        0.299 * c.red() as f64 + 0.587 * c.green() as f64 + 0.114 * c.blue() as f64
    }

    fn redraw(&self) {
        if let Some(canvas) = self.canvas.upgrade() {
            canvas.queue_draw();
        }
    }

    /// Show the session: its text (when `text`), its styles, its
    /// selection (when `select`), the controls and the canvas preview.
    fn show(&self, text: bool, select: bool) {
        let Some((working, (start, end))) = self.ctl.text_session(|s| (s.working().clone(), s.range())) else { return };
        let buffer = self.view.buffer();
        let was = self.syncing.replace(true);
        if text {
            buffer.set_text(&working.text);
        }
        retag(&buffer, &working, self.editor_ink());
        if select {
            let at = |b: usize| buffer.iter_at_offset(byte_to_char(&working.text, b).unwrap_or(0) as i32);
            buffer.select_range(&at(end), &at(start));
        }
        self.syncing.set(was);
        self.show_controls();
        self.redraw();
    }

    /// The controls, from what the selection (or the caret's typing
    /// style) looks like.
    fn show_controls(&self) {
        let Some(f) = self.ctl.text_session(|s| s.format_here()) else { return };
        let was = self.syncing.replace(true);
        let c = &self.controls;
        c.bold.set_active(f.bold);
        c.italic.set_active(f.italic);
        c.underline.set_active(f.underline);
        c.strike.set_active(f.strikethrough);
        c.size.set_value(f.font_size.unwrap_or(decks_core::format::DEFAULT_FONT_PT));
        let family = f.font_family.unwrap_or_default();
        let n = c.families.n_items();
        let found = (1..n).find(|i| c.families.string(*i).is_some_and(|s| s.as_str() == family));
        let index = match (family.is_empty(), found) {
            (true, _) => 0,
            (false, Some(i)) => i,
            // A family this system lacks (Calibri drawn as Carlito) is
            // still the run's: listed rather than shown as Default.
            (false, None) => {
                c.families.append(&family);
                n
            }
        };
        c.family.set_selected(index);
        for (b, (_, _, a)) in c.aligns.iter().zip(ALIGNS) {
            b.set_active(f.align == a);
        }
        c.list.set_selected(match f.list {
            Some(ListKind::Bullet) => 1,
            Some(ListKind::Number) => 2,
            _ => 0,
        });
        let para = self.ctl.text_session(|s| {
            let w = s.working();
            w.body.para(w.para_of(s.range().0))
        });
        if let Some(p) = para {
            let pt = |s: decks_core::engine::Spacing| match s {
                decks_core::engine::Spacing::Units(u) => u * 0.75,
                decks_core::engine::Spacing::Lines(_) => 0.0,
            };
            c.space_before.set_value(pt(p.space_before));
            c.space_after.set_value(pt(p.space_after));
        }
        self.syncing.set(was);
    }

    /// Apply a format to the selection (a caret's typing style for a
    /// character format).
    fn format(&self, edit: FormatEdit) {
        if self.syncing.get() {
            return;
        }
        let changed = self.ctl.edit_text_session(|s| s.format(&edit)).unwrap_or(false);
        if changed {
            self.show(false, false);
        } else {
            self.show_controls();
        }
    }

    fn toggle_style(&self, key: gdk::Key) {
        let Some(f) = self.ctl.text_session(|s| s.format_here()) else { return };
        self.format(match key {
            gdk::Key::b => FormatEdit::Bold(!f.bold),
            gdk::Key::i => FormatEdit::Italic(!f.italic),
            _ => FormatEdit::Underline(!f.underline),
        });
    }

    /// The session's own undo or redo.
    fn history(&self, undo: bool) {
        let done = self.ctl.edit_text_session(|s| if undo { s.undo() } else { s.redo() }).unwrap_or(false);
        if done {
            self.show(true, true);
        }
    }

    fn say(&self, text: Option<&str>, error: bool) {
        match text {
            Some(t) => {
                self.message.set_text(t);
                if error {
                    self.message.add_css_class("error");
                } else {
                    self.message.remove_css_class("error");
                }
                self.message.set_visible(true);
            }
            None => self.message.set_visible(false),
        }
    }

    /// Copy (and, with `cut`, delete) the selection as styled text.
    fn copy(&self, cut: bool) -> bool {
        let frag = self.ctl.text_session(|s| {
            let (a, b) = s.range();
            (a < b).then(|| decks_core::fragment::copy_range(s.working(), a, b))
        });
        let Some(frag) = frag.flatten() else { return false };
        let provider = suite_common::clipboard::provider(decks_core::fragment::MIME, &frag.to_json(), &frag.to_html(), &frag.to_plain());
        let _ = self.view.clipboard().set_content(Some(&provider));
        if cut {
            // Through the buffer, so the deletion is the same session
            // replacement a keyboard deletion is.
            self.view.buffer().delete_selection(true, true);
        }
        true
    }

    /// Paste suite styled text over the selection; `false` (the text
    /// view pastes plain text itself) when the clipboard holds none.
    fn paste_styled(self: &Rc<Self>) -> bool {
        let clipboard = self.view.clipboard();
        if !suite_common::clipboard::offers(&clipboard, decks_core::fragment::MIME) {
            return false;
        }
        let weak = Rc::downgrade(self);
        suite_common::clipboard::read_string(&clipboard, decks_core::fragment::MIME, move |json| {
            let Some(ed) = weak.upgrade() else { return };
            let Some(frag) = json.as_deref().and_then(decks_core::fragment::Fragment::from_json) else { return };
            let pasted = decks_core::fragment::paste_runs(&frag);
            let done = ed.ctl.edit_text_session(|s| s.insert_runs(pasted.runs, &pasted.paras).unwrap_or(false)).unwrap_or(false);
            if done {
                ed.show(true, true);
            }
            if pasted.flattened.is_empty() {
                ed.say(None, false);
            } else {
                let what = pasted.flattened.join(", ");
                ed.say(Some(&format!("{}: {what}", suite_common::i18n("Pasted as plain text"))), false);
            }
        });
        true
    }

    /// End the session: commit it (`commit`) or drop it. A commit that
    /// cannot land keeps the editor open, focused, with the reason, and
    /// returns `false`.
    fn finish(&self, commit: bool) -> bool {
        if self.ctl.has_text_session() {
            if commit {
                if let Err(e) = self.ctl.commit_text_session() {
                    self.say(Some(&e.to_string()), true);
                    self.view.grab_focus();
                    return false;
                }
            } else {
                self.ctl.cancel_text_session();
            }
            (self.refresh)();
        }
        self.restore_focus.set(true);
        self.popover.popdown();
        true
    }

    /// Whether the keyboard is in the editor itself, not in a menu it
    /// opened (the colour palette, paragraph spacing). On X11 a popover's
    /// keys arrive on the window's surface and the window's focus is not
    /// the popover's, so this asks the widgets: the focus is within the
    /// editor and none of its menus is open.
    fn keyboard_in_editor(&self) -> bool {
        let focus_native = self
            .window
            .upgrade()
            .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
            .and_then(|w| w.native())
            .is_some_and(|n| n.upcast::<gtk::Widget>() == *self.popover.upcast_ref::<gtk::Widget>());
        (focus_native || self.popover.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN) || self.view.has_focus())
            && !self.menus.borrow().iter().any(|m| m.is_visible())
    }

    /// The session's keys. Both the window's capture controller (ahead of
    /// the application's shortcuts, which GTK runs from the window) and
    /// the popover's call this; an event is handled once.
    fn key(self: &Rc<Self>, controller: &gtk::EventControllerKey, keyval: gdk::Key, mods: gdk::ModifierType, on_window: bool) -> glib::Propagation {
        use glib::Propagation::{Proceed, Stop};
        let stamp = (controller.current_event_time(), keyval);
        if self.last_key.replace(Some(stamp)) == Some(stamp) {
            return Proceed;
        }
        if self.preedit.get() {
            return Proceed;
        }
        let mods = mods & gtk::accelerator_get_default_mod_mask();
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let alt = mods.contains(gdk::ModifierType::ALT_MASK);
        if self.keyboard_in_editor() {
            match keyval {
                gdk::Key::Escape if mods.is_empty() => {
                    self.finish(false);
                    return Stop;
                }
                gdk::Key::Return | gdk::Key::KP_Enter if ctrl => {
                    self.finish(true);
                    return Stop;
                }
                _ => {}
            }
            if ctrl && !alt {
                let key = keyval.to_lower();
                let handled = match key {
                    gdk::Key::z => {
                        self.history(!shift);
                        true
                    }
                    gdk::Key::y if !shift => {
                        self.history(false);
                        true
                    }
                    // Ctrl+Shift+U stays the input method's Unicode entry.
                    gdk::Key::b | gdk::Key::i | gdk::Key::u if !shift => {
                        self.toggle_style(key);
                        true
                    }
                    gdk::Key::c | gdk::Key::x if !shift => self.copy(key == gdk::Key::x),
                    gdk::Key::v if !shift => self.paste_styled(),
                    _ => false,
                };
                if handled {
                    return Stop;
                }
            }
        }
        // Any other application shortcut (Save, Present, a slide command)
        // ends the session first, so it acts on the committed text.
        let app = self.window.upgrade().and_then(|w| w.application());
        if let Some(app) = app {
            let accel = gtk::accelerator_name(keyval.to_lower(), mods);
            let actions = app.actions_for_accel(&accel);
            if !actions.is_empty() {
                if !self.finish(true) {
                    return Stop;
                }
                // From the window, the shortcut runs next by itself; from
                // the popover, the window never saw it: run it here.
                if !on_window {
                    if let Some(window) = self.window.upgrade() {
                        for detailed in &actions {
                            if let Ok((name, target)) = gtk::gio::Action::parse_detailed_name(detailed) {
                                let _ = window.activate_action(&name, target.as_ref());
                            }
                        }
                    }
                    return Stop;
                }
                return Proceed;
            }
        }
        Proceed
    }

    fn open(&self) {
        self.say(None, false);
        self.preedit.set(false);
        self.show(true, true);
        if let Some(canvas) = self.canvas.upgrade() {
            position_text_editor(&canvas, &self.popover, &self.scrolled, self.bounds.get());
        }
        self.popover.popup();
        self.view.grab_focus();
        crate::canvas_input::trace_input(|| {
            let (_, nat) = self.popover.preferred_size();
            format!(
                "editor popup: visible {}, mapped {}, pointing to {:?}, natural {}x{}, window {:?}",
                self.popover.is_visible(),
                self.popover.is_mapped(),
                self.popover.pointing_to().1,
                nat.width(),
                nat.height(),
                self.popover.root().map(|r| (r.width(), r.height())),
            )
        });
    }
}

/// The formatting bar's two rows and its controls, not yet connected.
fn build_controls() -> (gtk::Box, Controls) {
    let bar = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let row1 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let row2 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let families = gtk::StringList::new(&["Default"]);
    {
        let mut names: Vec<String> = gtk::TextView::new().pango_context().list_families().iter().map(|f| f.name().to_string()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        for n in &names {
            families.append(n);
        }
    }
    let expression = gtk::PropertyExpression::new(gtk::StringObject::static_type(), None::<gtk::Expression>, "string");
    let family = gtk::DropDown::new(Some(families.clone()), Some(expression));
    family.set_enable_search(true);
    family.set_tooltip_text(Some("Font"));
    family.update_property(&[gtk::accessible::Property::Label("Font of Selection")]);
    let size = labelled_spin("Font Size of Selection", 4.0, 400.0, 1.0);
    size.set_digits(1);
    let bold = toggle("format-text-bold-symbolic", "Bold Selection (Ctrl+B)");
    let italic = toggle("format-text-italic-symbolic", "Italic Selection (Ctrl+I)");
    let underline = toggle("format-text-underline-symbolic", "Underline Selection (Ctrl+U)");
    let strike = toggle("format-text-strikethrough-symbolic", "Strike Through Selection");
    row1.append(&family);
    row1.append(&size);
    row1.append(&linked(&[bold.upcast_ref(), italic.upcast_ref(), underline.upcast_ref(), strike.upcast_ref()]));
    let aligns: Vec<gtk::ToggleButton> = ALIGNS.iter().map(|(icon, label, _)| toggle(icon, label)).collect();
    let refs: Vec<&gtk::Widget> = aligns.iter().map(|b| b.upcast_ref()).collect();
    row2.append(&linked(&refs));
    let list = gtk::DropDown::from_strings(&["No List", "Bullets", "Numbers"]);
    list.set_tooltip_text(Some("List"));
    list.update_property(&[gtk::accessible::Property::Label("List of Paragraph")]);
    row2.append(&list);
    let space_before = labelled_spin("Space Before Paragraph (pt)", 0.0, 400.0, 1.0);
    let space_after = labelled_spin("Space After Paragraph (pt)", 0.0, 400.0, 1.0);
    bar.append(&row1);
    bar.append(&row2);
    let controls = Controls { family, families, size, bold, italic, underline, strike, aligns, list, space_before, space_after };
    (bar, controls)
}

/// A native multiline rich text editor anchored to the actual canvas. Its
/// host must be outside CanvasArea: that widget exposes only virtual
/// slide objects as accessible children, hiding real child widgets from
/// assistive tools. The popover lives as long as the window
/// (suite-common's surface-lifetime rule); moving focus inside it is not
/// a commit.
pub(crate) fn register(h: EditorHandles, editor_host: &gtk::Overlay) {
    let popover = suite_common::popover::reused(editor_host, "slide-text-editor");
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_autohide(true);
    let view = gtk::TextView::new();
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_left_margin(8);
    view.set_right_margin(8);
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.buffer().set_enable_undo(false);
    view.update_property(&[
        gtk::accessible::Property::Label("Slide text editor"),
        gtk::accessible::Property::Description(
            "Enter inserts a new line. Control+Enter applies changes. Escape cancels. Control+B, I and U format the selection.",
        ),
    ]);
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_child(Some(&view));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let (bar, controls) = build_controls();
    // The bar is 443px wide, wider than a phone-sized window: a popover
    // that cannot fit its screen is never shown, so at 400px the editor
    // closed as it opened and slide text could not be edited. It keeps its
    // full width where there is room and scrolls sideways where there is
    // not.
    let bar_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .child(&bar)
        .build();
    body.append(&bar_scroller);
    body.append(&scrolled);
    let hint = gtk::Label::new(Some(&suite_common::i18n(
        "Enter: new line · Ctrl+Enter: apply · Escape: cancel · Formats apply to the selection. The slide shows exact sizes and layout.",
    )));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    hint.set_wrap(true);
    hint.set_max_width_chars(48);
    body.append(&hint);
    let message = gtk::Label::new(None);
    message.set_wrap(true);
    message.set_visible(false);
    message.set_accessible_role(gtk::AccessibleRole::Alert);
    body.append(&message);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let colour = button("color-select-symbolic", "Text Colour of Selection");
    let spacing = gtk::Button::with_label(&suite_common::i18n("Spacing"));
    spacing.set_tooltip_text(Some("Paragraph Spacing"));
    spacing.update_property(&[gtk::accessible::Property::Label("Paragraph Spacing")]);
    let outdent = button("format-indent-less-symbolic", "Decrease Paragraph Indent");
    let indent = button("format-indent-more-symbolic", "Increase Paragraph Indent");
    let cancel = gtk::Button::with_label(&suite_common::i18n("Cancel"));
    let apply = gtk::Button::with_label(&suite_common::i18n("Apply"));
    apply.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&apply);
    body.append(&buttons);
    popover.set_child(Some(&body));

    let ed = Rc::new(Editor {
        ctl: h.controller.clone(),
        window: h.window.clone().upcast::<gtk::Window>().downgrade(),
        canvas: h.canvas.downgrade(),
        popover: popover.clone(),
        view: view.clone(),
        scrolled: scrolled.clone(),
        message,
        controls,
        refresh: h.refresh_hud.clone(),
        bounds: Cell::new((0.0, 0.0, 0.0, 0.0)),
        syncing: Cell::new(false),
        preedit: Cell::new(false),
        restore_focus: Cell::new(false),
        last_key: Cell::new(None),
        menus: Default::default(),
    });
    // The second row gets the indent, colour and spacing controls.
    if let Some(row2) = bar.last_child().and_downcast::<gtk::Box>() {
        row2.append(&linked(&[outdent.upcast_ref(), indent.upcast_ref()]));
        row2.append(&colour);
        row2.append(&spacing);
    }
    let weak = Rc::downgrade(&ed);
    connect_controls(&ed, &weak, &colour, &spacing, &outdent, &indent);
    for (b, commit) in [(cancel, false), (apply, true)] {
        let weak = weak.clone();
        b.connect_clicked(move |_| {
            if let Some(ed) = weak.upgrade() {
                ed.finish(commit);
            }
        });
    }
    connect_buffer(&ed, &weak);
    {
        let weak = weak.clone();
        view.connect_preedit_changed(move |_, preedit| {
            if let Some(ed) = weak.upgrade() {
                ed.preedit.set(!preedit.is_empty());
            }
        });
    }
    // Ahead of the application's shortcuts: GTK runs a window's capture
    // controllers newest first, and the shortcuts were added with the
    // window.
    for on_window in [true, false] {
        let weak = weak.clone();
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(move |controller, keyval, _, mods| {
            let Some(ed) = weak.upgrade() else { return glib::Propagation::Proceed };
            if !ed.ctl.has_text_session() {
                return glib::Propagation::Proceed;
            }
            ed.key(controller, keyval, mods, on_window)
        });
        if on_window {
            h.window.add_controller(key);
        } else {
            popover.add_controller(key);
        }
    }
    {
        let weak = weak.clone();
        popover.connect_closed(move |_| {
            crate::canvas_input::trace_input(|| "editor popover closed".to_owned());
            let Some(ed) = weak.upgrade() else { return };
            ed.preedit.set(false);
            // Leaving the session any other way commits it; a commit that
            // cannot land reopens the editor with the reason.
            if ed.ctl.has_text_session() {
                match ed.ctl.commit_text_session() {
                    Ok(_) => (ed.refresh)(),
                    Err(e) => {
                        let weak = Rc::downgrade(&ed);
                        glib::idle_add_local_once(move || {
                            if let Some(ed) = weak.upgrade() {
                                ed.popover.popup();
                                ed.say(Some(&e.to_string()), true);
                                ed.view.grab_focus();
                            }
                        });
                        return;
                    }
                }
            }
            if let Some(canvas) = ed.canvas.upgrade() {
                canvas.queue_draw();
                if ed.restore_focus.replace(false) {
                    canvas.grab_focus();
                }
            }
        });
    }
    {
        let weak = weak.clone();
        h.canvas.connect_resize(move |canvas, _, _| {
            if let Some(ed) = weak.upgrade().filter(|ed| ed.ctl.has_text_session()) {
                position_text_editor(canvas, &ed.popover, &ed.scrolled, ed.bounds.get());
            }
        });
    }
    {
        // Ctrl+click on linked text opens its link (#1998): the gesture
        // PowerPoint and Impress use while editing, so a plain click still
        // selects. Only a click that lands on a link is claimed.
        let weak = weak.clone();
        let slides = h.slides.clone();
        let masters = h.masters.clone();
        let follow = gtk::GestureClick::new();
        follow.set_button(1);
        follow.connect_pressed(move |gesture, n, x, y| {
            let ctrl = gesture.current_event_state().contains(gdk::ModifierType::CONTROL_MASK);
            let Some(ed) = weak.upgrade().filter(|ed| ctrl && n == 1 && !ed.ctl.has_text_session()) else { return };
            let Some(canvas) = ed.canvas.upgrade() else { return };
            let (cw, ch) = (canvas.width() as f64, canvas.height() as f64);
            let (hx, hy) = canvas_to_slide(x, y, cw, ch);
            let Some(idx) = ed.ctl.selected_slide_index() else { return };
            let link = {
                let slides = slides.borrow();
                slides.get(idx).and_then(|slide| {
                    let oi = hit_test_object(&slide.objects, hx, hy)?;
                    let obj = &slide.objects[oi];
                    let SlideObject::TextBox { runs, .. } = obj else { return None };
                    let masters = masters.borrow();
                    let master = crate::canvas::master_for(&slides, idx, &masters);
                    let at = crate::canvas::text_offset_at(obj, crate::canvas::slide_geometry(cw, ch), master.as_deref(), (x, y))?;
                    decks_core::rich_text::link_at(runs, at).map(str::to_owned)
                })
            };
            let Some(uri) = link.filter(|u| decks_core::rich_text::is_followable(u)) else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let parent = canvas.root().and_downcast::<gtk::Window>();
            gtk::UriLauncher::new(&uri).launch(parent.as_ref(), gtk::gio::Cancellable::NONE, |_| {});
        });
        h.canvas.add_controller(follow);
    }
    {
        // The double-click gesture owns the editor: it lives as long as the
        // canvas, and every other handler holds it weakly.
        let slides = h.slides.clone();
        let masters = h.masters.clone();
        let dbl = gtk::GestureClick::new();
        dbl.set_button(1);
        // The editor opens on the double-click's release, not its press.
        // Opened on the press, the autohiding popover was up before the
        // second click's release arrived; that release lands on the canvas,
        // outside it, and closed it at once wherever the popover mapped
        // that fast (the release soak's wide-1280-scale2: mapped, then
        // closed, no editor).
        let pending_open = Rc::new(Cell::new(false));
        {
            let weak = Rc::downgrade(&ed);
            let pending = pending_open.clone();
            let open_now = move || {
                if pending.replace(false) {
                    if let Some(ed) = weak.upgrade().filter(|ed| ed.ctl.has_text_session()) {
                        ed.open();
                    }
                }
            };
            let on_stop = open_now.clone();
            dbl.connect_released(move |_, _, _, _| open_now());
            // No release, the sequence taken over: open all the same.
            dbl.connect_stopped(move |_| on_stop());
        }
        dbl.connect_pressed(move |gesture, n, x, y| {
            crate::canvas_input::trace_input(|| format!("click {n} at {x:.1},{y:.1}, text session {}", ed.ctl.has_text_session()));
            if n != 2 || ed.ctl.has_text_session() {
                return;
            }
            let Some(canvas) = ed.canvas.upgrade() else { return };
            let (cw, ch) = (canvas.width() as f64, canvas.height() as f64);
            let (hx, hy) = canvas_to_slide(x, y, cw, ch);
            // Resolve the view index through the controller first: the
            // window's slide borrow below must never overlap a
            // controller call on the same RefCell.
            let Some(idx) = ed.ctl.selected_slide_index() else { return };
            let target = {
                let mut slides = slides.borrow_mut();
                decks_core::ops::ensure_ids(&mut slides);
                slides.get(idx).and_then(|slide| {
                    let oi = hit_test_object(&slide.objects, hx, hy)?;
                    let obj = &slide.objects[oi];
                    if !matches!(obj, SlideObject::TextBox { .. }) {
                        return None;
                    }
                    let masters = masters.borrow();
                    let master = crate::canvas::master_for(&slides, idx, &masters);
                    let caret = crate::canvas::text_offset_at(obj, crate::canvas::slide_geometry(cw, ch), master.as_deref(), (x, y));
                    Some((slide.ids.slide, slide.ids.objects[oi], decks_core::undo::obj_bounds(obj), caret))
                })
            };
            crate::canvas_input::trace_input(|| format!("double-click at slide {hx:.1},{hy:.1} hits a text box: {}", target.is_some()));
            let Some((slide_id, object_id, bounds, caret)) = target else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if !ed.ctl.begin_text_session(slide_id, object_id) {
                crate::canvas_input::trace_input(|| "the text session did not begin".to_owned());
                return;
            }
            // The caret goes where the double-click was, on the text the
            // canvas draws.
            if let Some(at) = caret {
                ed.ctl.edit_text_session(|s| {
                    let _ = s.set_range(at, at);
                });
            }
            ed.bounds.set(bounds);
            (ed.refresh)();
            pending_open.set(true);
        });
        h.canvas.add_controller(dbl);
    }
}

/// The formatting bar's signals: each one session format.
fn connect_controls(ed: &Rc<Editor>, weak: &Weak<Editor>, colour: &gtk::Button, spacing: &gtk::Button, outdent: &gtk::Button, indent: &gtk::Button) {
    let c = &ed.controls;
    // Buttons hand the keyboard back to the text, so typing goes on.
    let fmt = |weak: &Weak<Editor>, edit: FormatEdit, refocus: bool| {
        if let Some(ed) = weak.upgrade() {
            ed.format(edit);
            if refocus && !ed.syncing.get() {
                ed.view.grab_focus();
            }
        }
    };
    type MakeEdit = fn(bool) -> FormatEdit;
    let styles: [(&gtk::ToggleButton, MakeEdit); 4] = [
        (&c.bold, FormatEdit::Bold),
        (&c.italic, FormatEdit::Italic),
        (&c.underline, FormatEdit::Underline),
        (&c.strike, FormatEdit::Strikethrough),
    ];
    for (b, make) in styles {
        let weak = weak.clone();
        b.connect_toggled(move |t| fmt(&weak, make(t.is_active()), true));
    }
    for (b, (_, _, align)) in c.aligns.iter().zip(ALIGNS) {
        let weak = weak.clone();
        b.connect_toggled(move |t| {
            if t.is_active() {
                fmt(&weak, FormatEdit::Align(align), true);
            } else if let Some(ed) = weak.upgrade().filter(|ed| !ed.syncing.get()) {
                // Alignment is one of four: clicking the current one
                // keeps it.
                ed.show_controls();
            }
        });
    }
    for (b, more) in [(outdent, false), (indent, true)] {
        let weak = weak.clone();
        b.connect_clicked(move |_| fmt(&weak, FormatEdit::Indent(more), true));
    }
    {
        let weak = weak.clone();
        c.size.connect_value_changed(move |s| fmt(&weak, FormatEdit::FontSize(s.value()), false));
    }
    {
        let weak = weak.clone();
        c.family.connect_selected_notify(move |d| {
            let Some(ed) = weak.upgrade() else { return };
            let name = (d.selected() > 0).then(|| ed.controls.families.string(d.selected())).flatten().map(|s| s.to_string());
            ed.format(FormatEdit::FontFamily(name));
        });
    }
    {
        let weak = weak.clone();
        c.list.connect_selected_notify(move |d| {
            let kind = match d.selected() {
                1 => ListKind::Bullet,
                2 => ListKind::Number,
                _ => ListKind::None,
            };
            fmt(&weak, FormatEdit::List(kind), false);
        });
    }
    // The colour palette: named swatches and Automatic.
    let grid = gtk::Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    // Menus of the editor's own: kept for the window's life, like the
    // editor (suite-common's surface-lifetime rule).
    let palette = suite_common::popover::reused(colour, "text-colour-palette");
    for (i, (name, hex)) in PALETTE.iter().enumerate() {
        let swatch = gtk::Label::new(None);
        swatch.set_markup(&format!("<span foreground=\"#{hex}\" size=\"x-large\">■</span>"));
        let b = gtk::Button::builder().child(&swatch).tooltip_text(*name).build();
        b.add_css_class("flat");
        b.update_property(&[gtk::accessible::Property::Label(name)]);
        let (weak, palette) = (weak.clone(), palette.downgrade());
        let hex = hex.to_string();
        b.connect_clicked(move |_| {
            if let Some(p) = palette.upgrade() {
                p.popdown();
            }
            fmt(&weak, FormatEdit::TextColor(Some(hex.clone())), true);
        });
        grid.attach(&b, (i % 5) as i32, (i / 5) as i32, 1, 1);
    }
    let auto = gtk::Button::with_label(&suite_common::i18n("Automatic Colour"));
    {
        let (weak, palette) = (weak.clone(), palette.downgrade());
        auto.connect_clicked(move |_| {
            if let Some(p) = palette.upgrade() {
                p.popdown();
            }
            fmt(&weak, FormatEdit::TextColor(None), true);
        });
    }
    grid.attach(&auto, 0, 2, 5, 1);
    palette.set_child(Some(&grid));
    ed.menus.borrow_mut().push(palette.clone());
    {
        let palette = palette.downgrade();
        colour.connect_clicked(move |_| {
            if let Some(p) = palette.upgrade() {
                p.popup();
            }
        });
    }
    // Paragraph spacing: above and below, in points.
    let sp = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for (label, spin, before) in [("Space before (pt)", &c.space_before, true), ("Space after (pt)", &c.space_after, false)] {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let l = gtk::Label::new(Some(&suite_common::i18n(label)));
        l.set_hexpand(true);
        l.set_xalign(0.0);
        row.append(&l);
        row.append(spin);
        sp.append(&row);
        let weak = weak.clone();
        spin.connect_value_changed(move |s| {
            let pt = s.value();
            fmt(&weak, if before { FormatEdit::SpaceBefore(pt) } else { FormatEdit::SpaceAfter(pt) }, false);
        });
    }
    let sp_popover = suite_common::popover::reused(spacing, "paragraph-spacing");
    sp_popover.set_child(Some(&sp));
    ed.menus.borrow_mut().push(sp_popover.clone());
    {
        let sp_popover = sp_popover.downgrade();
        spacing.connect_clicked(move |_| {
            if let Some(p) = sp_popover.upgrade() {
                p.popup();
            }
        });
    }
}

/// The buffer's signals: every user change to the text is the same change
/// to the session, before GTK applies it to the buffer; the selection is
/// the session's selection.
fn connect_buffer(ed: &Rc<Editor>, weak: &Weak<Editor>) {
    let buffer = ed.view.buffer();
    {
        let weak = weak.clone();
        buffer.connect_insert_text(move |_, at, text| {
            let Some(ed) = weak.upgrade().filter(|ed| !ed.syncing.get()) else { return };
            let chars = at.offset().max(0) as usize;
            ed.ctl.edit_text_session(|s| {
                if let Some(b) = char_to_byte(&s.working().text, chars) {
                    let _ = s.replace(b, b, text);
                }
            });
        });
    }
    {
        let weak = weak.clone();
        buffer.connect_delete_range(move |_, a, b| {
            let Some(ed) = weak.upgrade().filter(|ed| !ed.syncing.get()) else { return };
            let (x, y) = (a.offset().min(b.offset()).max(0) as usize, a.offset().max(b.offset()).max(0) as usize);
            ed.ctl.edit_text_session(|s| {
                let range = {
                    let t = &s.working().text;
                    char_to_byte(t, x).zip(char_to_byte(t, y))
                };
                if let Some((x, y)) = range {
                    let _ = s.replace(x, y, "");
                }
            });
        });
    }
    {
        let weak = weak.clone();
        buffer.connect_changed(move |buffer| {
            let Some(ed) = weak.upgrade().filter(|ed| !ed.syncing.get()) else { return };
            // Inserted text takes its run's tags from the session.
            if let Some(working) = ed.ctl.text_session(|s| s.working().clone()) {
                ed.syncing.set(true);
                retag(buffer, &working, ed.editor_ink());
                ed.syncing.set(false);
            }
            ed.show_controls();
            ed.redraw();
        });
    }
    {
        let weak = weak.clone();
        buffer.connect_mark_set(move |buffer, _, mark| {
            if !matches!(mark.name().as_deref(), Some("insert") | Some("selection_bound")) {
                return;
            }
            let Some(ed) = weak.upgrade().filter(|ed| !ed.syncing.get()) else { return };
            let (a, b) = buffer.selection_bounds().unwrap_or_else(|| {
                let at = || buffer.iter_at_mark(&buffer.get_insert());
                (at(), at())
            });
            let (x, y) = (a.offset().min(b.offset()).max(0) as usize, a.offset().max(b.offset()).max(0) as usize);
            ed.ctl.edit_text_session(|s| {
                let range = {
                    let t = &s.working().text;
                    char_to_byte(t, x).zip(char_to_byte(t, y))
                };
                if let Some((x, y)) = range {
                    let _ = s.set_range(x, y);
                }
            });
            ed.show_controls();
            ed.redraw();
        });
    }
}

fn position_text_editor(
    canvas: &gtk::DrawingArea,
    popover: &gtk::Popover,
    scrolled: &gtk::ScrolledWindow,
    (x, y, w, h): (f64, f64, f64, f64),
) {
    let width = canvas.width() as f64;
    let height = canvas.height() as f64;
    let (left, top) = crate::canvas::slide_to_canvas(x, y, width, height);
    let (right, bottom) = crate::canvas::slide_to_canvas(x + w, y + h, width, height);
    // The host is the surrounding overlay, not the canvas. Translate
    // through the scrolled viewport so zoom/scroll offsets are respected.
    let Some(host) = popover.parent() else { return };
    let Some(origin) = canvas.compute_point(&host, &gtk::graphene::Point::new(0.0, 0.0)) else { return };
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
        (left + f64::from(origin.x())).round() as i32,
        (top + f64::from(origin.y())).round() as i32,
        (right - left).round().max(1.0) as i32,
        (bottom - top).round().max(1.0) as i32,
    )));
    // Tiny boxes still need usable controls; large boxes scroll rather than
    // forcing the native popup past the viewport.
    let text_w = (right - left).clamp(320.0, (width - 32.0).max(320.0)) as i32;
    let mut text_h = (bottom - top).clamp(120.0, (height * 0.5).max(120.0)) as i32;
    // And the whole popover has to fit above or below the box in the
    // window: one that fits on neither side was mapped and closed at once,
    // so a short window (the release soak's 640x512 wide-1280-scale2)
    // opened no editor at all. The text area gives up height first, down
    // to a few lines.
    if let (Some(root), Some(top_in_root), Some(bottom_in_root)) = (
        canvas.root(),
        canvas.compute_point(&canvas.root().unwrap(), &gtk::graphene::Point::new(left as f32, top as f32)),
        canvas.compute_point(&canvas.root().unwrap(), &gtk::graphene::Point::new(left as f32, bottom as f32)),
    ) {
        scrolled.set_size_request(text_w, text_h);
        let chrome = popover.measure(gtk::Orientation::Vertical, -1).1 - text_h;
        let below = root.height() - bottom_in_root.y() as i32;
        let above = top_in_root.y() as i32;
        let room = below.max(above) - POPOVER_MARGIN;
        if chrome + text_h > room {
            text_h = (room - chrome).max(MIN_TEXT_HEIGHT);
        }
        crate::canvas_input::trace_input(|| format!("editor fits {text_h} text over {chrome} chrome; {above} above, {below} below the box"));
    }
    scrolled.set_size_request(text_w, text_h);
}

/// Space the popover's arrow and the window edge take, beyond its content.
const POPOVER_MARGIN: i32 = 24;
/// The least text the editor shows: a few lines, scrolling beyond.
const MIN_TEXT_HEIGHT: i32 = 48;
