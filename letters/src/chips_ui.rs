// SPDX-License-Identifier: GPL-3.0-or-later
//
// chips_ui.rs — inserting and using smart chips (letters_core::chips).
//
// Typing "@" at the start of a word, in either view, opens a small
// popover at the caret: a search entry and the chips it suggests (today,
// tomorrow, yesterday; a date, a link or a person as you type). Enter or a
// click inserts the chosen chip in place of the "@", as one model op, so it
// is one undo step in both views. Escape leaves the "@" as typed.
// `app.insert-chip` opens the same popover without an "@".
//
// Clicking a chip on the page opens its card: a date chip's calendar (pick
// a day to change it), or a link or person chip's address with Open.

use gtk4::{self as gtk, glib, prelude::*};
use letters_core::chips::{self, Chip, ChipKind};
use letters_core::edit::Op;
use letters_core::{ParaStyle, Paragraph, Run};

/// Widget data key: the view's popover opener. The page view's is also
/// kept on the buffer, which is what its typing path has.
const OPENER: &str = "letters-chip-opener";
const PAGE_OPENER: &str = "letters-chip-page-opener";

type Opener = std::rc::Rc<dyn Fn(bool)>;

fn today() -> chips::NaiveDate {
    chips::today()
}

/// Insert `run` at the caret as one model op; `replace_at` first removes
/// the "@" just before the caret.
pub fn insert(buf: &gtk::TextBuffer, run: Run, replace_at: bool) -> bool {
    let Some(live) = crate::live::of(buf) else { return false };
    let mut m = live.borrow_mut();
    let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
    let mut at = m.sequence_offset(buf, caret);
    let mut ops = Vec::new();
    if replace_at && at > 0 {
        ops.push(Op::Delete { at: at - 1, len: 1 });
        at -= 1;
    }
    ops.push(Op::Insert { at, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![run] }] });
    let done = m.apply_user_ops(buf, &ops, false);
    drop(m);
    crate::live::sync_actions(buf);
    done
}

fn kind_icon(kind: ChipKind) -> &'static str {
    match kind {
        ChipKind::Date => "x-office-calendar-symbolic",
        ChipKind::Person => "avatar-default-symbolic",
        ChipKind::Link => "insert-link-symbolic",
    }
}

fn suggestion_row(run: &Run) -> gtk::ListBoxRow {
    let chip = run.style.chip.clone().unwrap_or(Chip { kind: ChipKind::Link, value: String::new() });
    let icon = gtk::Image::from_icon_name(kind_icon(chip.kind));
    let title = gtk::Label::builder().label(&run.text).xalign(0.0).build();
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.append(&icon);
    line.append(&title);
    // A date says which day it is relative to today, as Docs does.
    let relative = chips::NaiveDate::parse_from_str(&chip.value, "%Y-%m-%d").ok().and_then(|d| match (d - today()).num_days() {
        0 => Some("Today"),
        1 => Some("Tomorrow"),
        -1 => Some("Yesterday"),
        _ => None,
    });
    let detail = match chip.kind {
        ChipKind::Date => relative.map(str::to_string),
        _ => (chip.value != run.text).then(|| chip.value.clone()),
    };
    if let Some(detail) = detail {
        let detail = gtk::Label::builder().label(&detail).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
        detail.add_css_class("dim-label");
        line.append(&detail);
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    row.update_property(&[gtk::accessible::Property::Label(&run.text)]);
    row
}

/// Attach the "@" popover to `view` (the page view or the Draft editor),
/// pointing at the caret through `locate` (buffer offset → rectangle in
/// `view`). `draft` views watch the buffer for a typed "@"; the page view
/// reports its own typing (`typed`).
pub fn attach(
    view: &gtk::Widget,
    buf: &gtk::TextBuffer,
    locate: impl Fn(usize) -> Option<gtk::gdk::Rectangle> + 'static,
    draft: bool,
) {
    let pop = gtk::Popover::new();
    pop.set_parent(view);
    pop.set_position(gtk::PositionType::Bottom);
    let entry = gtk::SearchEntry::builder().placeholder_text("Date, person or link").build();
    entry.update_property(&[gtk::accessible::Property::Label("Smart chip")]);
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    list.update_property(&[gtk::accessible::Property::Label("Smart chip suggestions")]);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.set_size_request(260, -1);
    column.append(&entry);
    column.append(&list);
    pop.set_child(Some(&column));

    let suggestions: std::rc::Rc<std::cell::RefCell<Vec<Run>>> = Default::default();
    let replace_at = std::rc::Rc::new(std::cell::Cell::new(false));
    let refill = {
        let (list, suggestions) = (list.clone(), suggestions.clone());
        move |query: &str| {
            while let Some(r) = list.first_child() {
                list.remove(&r);
            }
            let now = chips::suggestions(query, today());
            for run in &now {
                list.append(&suggestion_row(run));
            }
            list.select_row(list.row_at_index(0).as_ref());
            *suggestions.borrow_mut() = now;
        }
    };
    let choose = {
        let (buf, pop, view, suggestions, replace_at) = (buf.clone(), pop.clone(), view.clone(), suggestions.clone(), replace_at.clone());
        std::rc::Rc::new(move |index: usize| {
            let run = suggestions.borrow().get(index).cloned();
            pop.popdown();
            if let Some(run) = run {
                insert(&buf, run, replace_at.get());
            }
            view.grab_focus();
        })
    };
    {
        let refill = refill.clone();
        entry.connect_search_changed(move |e| refill(&e.text()));
    }
    {
        let (choose, list) = (choose.clone(), list.clone());
        entry.connect_activate(move |_| {
            let index = list.selected_row().map_or(0, |r| r.index().max(0) as usize);
            choose(index);
        });
    }
    {
        let choose = choose.clone();
        list.connect_row_activated(move |_, row| choose(row.index().max(0) as usize));
    }
    {
        let (list, pop) = (list.clone(), pop.clone());
        entry.connect_stop_search(move |_| pop.popdown());
        // Up and Down move through the suggestions without leaving the entry.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            let step = match key {
                gtk::gdk::Key::Down => 1,
                gtk::gdk::Key::Up => -1,
                _ => return glib::Propagation::Proceed,
            };
            let i = list.selected_row().map_or(0, |r| r.index()) + step;
            if let Some(row) = list.row_at_index(i.max(0)) {
                list.select_row(Some(&row));
            }
            glib::Propagation::Stop
        });
        entry.add_controller(keys);
    }
    {
        let view = view.clone();
        pop.connect_closed(move |_| {
            view.grab_focus();
        });
    }

    let opener: Opener = {
        let (buf, pop, entry, replace_at) = (buf.clone(), pop.clone(), entry.clone(), replace_at.clone());
        std::rc::Rc::new(move |after_at: bool| {
            let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
            let Some(rect) = locate(caret) else { return };
            replace_at.set(after_at);
            entry.set_text("");
            refill("");
            pop.set_pointing_to(Some(&rect));
            pop.popup();
            entry.grab_focus();
        })
    };
    unsafe { view.set_data(OPENER, opener.clone()) };
    if !draft {
        unsafe { buf.set_data(PAGE_OPENER, opener.clone()) };
    }

    if draft {
        let view = view.clone();
        buf.connect_insert_text(move |b, pos, text| {
            if text != "@" || crate::live::is_busy(b) || !view.is_mapped() {
                return;
            }
            let mut before = *pos;
            let starts_word = !before.backward_char() || before.char().is_whitespace();
            if starts_word {
                // After the insertion completes: the caret is past the "@".
                let opener = opener.clone();
                glib::idle_add_local_once(move || opener(true));
            }
        });
    }
}

/// The page view's typing reached the model: open the popover when it was
/// an "@" starting a word.
pub fn typed(buf: &gtk::TextBuffer, text: &str) {
    if text != "@" {
        return;
    }
    let mut at = buf.iter_at_mark(&buf.get_insert());
    at.backward_char();
    let mut before = at;
    if before.backward_char() && !before.char().is_whitespace() {
        return;
    }
    if let Some(opener) = unsafe { buf.data::<Opener>(PAGE_OPENER) } {
        let opener = unsafe { opener.as_ref() }.clone();
        glib::idle_add_local_once(move || opener(true));
    }
}

/// A click on the page at `x` that placed the caret at buffer offset `off`:
/// if it landed on a chip, open the chip's card.
pub fn card_on_click(view: &crate::page_view::PageView, buf: &gtk::TextBuffer, x: f64, off: usize) {
    let Some((chip, label, range, rect)) = chip_under_click(view, buf, x, off) else { return };
    let (view, buf) = (view.clone(), buf.clone());
    glib::idle_add_local_once(move || show_card(view.upcast_ref(), &buf, &rect, chip, &label, range));
}

/// The chip a click at `x` hit, when it put the caret at buffer offset
/// `off`: the caret lands before or after a chip, whichever edge is
/// nearer, and the click must be between those edges.
fn chip_under_click(view: &crate::page_view::PageView, buf: &gtk::TextBuffer, x: f64, off: usize) -> Option<(Chip, String, (usize, usize), gtk::gdk::Rectangle)> {
    let found = chip_at(buf, off).filter(|c| c.2 .0 == off).or_else(|| off.checked_sub(1).and_then(|o| chip_at(buf, o)));
    let (chip, label, range) = found?;
    let ((x0, y, h), (x1, _, _)) = (view.caret_rect(range.0)?, view.caret_rect(range.1)?);
    if x < x0 || x > x1 {
        return None;
    }
    let rect = gtk::gdk::Rectangle::new(x0 as i32, y as i32, (x1 - x0).max(1.0) as i32, h.ceil() as i32);
    Some((chip, label, range, rect))
}

/// Open `view`'s chip popover at the caret.
pub fn open(view: &gtk::Widget, after_at: bool) {
    if let Some(opener) = unsafe { view.data::<Opener>(OPENER) } {
        let opener = unsafe { opener.as_ref() }.clone();
        opener(after_at);
    }
}

/// `app.insert-chip` (Insert ▸ Smart Chip, Ctrl+Alt+C): the popover at the
/// caret of the active tab's visible view.
pub fn register_action(app: &libadwaita::Application, tv: &libadwaita::TabView) {
    let a = gtk::gio::SimpleAction::new("insert-chip", None);
    let tv = tv.clone();
    a.connect_activate(move |_, _| {
        let Some(child) = tv.selected_page().map(|p| p.child()) else { return };
        let page = child
            .clone()
            .downcast::<crate::page_container::PageContainer>()
            .ok()
            .filter(|pc| pc.is_print_layout())
            .and_then(|pc| pc.page_view())
            .map(|v| v.upcast::<gtk::Widget>());
        if let Some(view) = page.or_else(|| crate::dialogs::get_textview(&child).map(|t| t.upcast())) {
            open(&view, false);
        }
    });
    app.add_action(&a);
    app.set_accels_for_action("app.insert-chip", &["<Primary><Alt>c"]);
    suite_common::actions::register_labels(&[("app.insert-chip", &suite_common::i18n("Insert Smart Chip…"))]);
}

/// The chip whose object char starts at buffer offset `off`, if one does:
/// (chip, label, its buffer range).
pub fn chip_at(buf: &gtk::TextBuffer, off: usize) -> Option<(Chip, String, (usize, usize))> {
    let iter = buf.iter_at_offset(off as i32);
    let (tag, chip) = iter.tags().into_iter().find_map(|t| {
        let name = t.name()?;
        let chip = crate::bridge::parse_chip_tag(&name)?;
        Some((t, chip))
    })?;
    let mut start = iter;
    if !start.starts_tag(Some(&tag)) {
        start.backward_to_tag_toggle(Some(&tag));
    }
    let mut end = iter;
    end.forward_to_tag_toggle(Some(&tag));
    Some((chip, buf.text(&start, &end, false).to_string(), (start.offset() as usize, end.offset() as usize)))
}

/// Replace the chip spanning buffer `range` with `run`, as one step.
fn replace(buf: &gtk::TextBuffer, range: (usize, usize), run: Run) {
    let Some(live) = crate::live::of(buf) else { return };
    let mut m = live.borrow_mut();
    let at = m.sequence_offset(buf, range.0);
    let ops = [
        Op::Delete { at, len: 1 },
        Op::Insert { at, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![run] }] },
    ];
    m.apply_user_ops(buf, &ops, false);
    drop(m);
    crate::live::sync_actions(buf);
}

/// Show the card of the chip at buffer range `range` on `view`, pointing
/// at `rect`: a calendar for a date, the address and Open otherwise.
pub fn show_card(view: &gtk::Widget, buf: &gtk::TextBuffer, rect: &gtk::gdk::Rectangle, chip: Chip, label: &str, range: (usize, usize)) {
    let pop = gtk::Popover::new();
    pop.set_parent(view);
    pop.set_pointing_to(Some(rect));
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.set_margin_top(6);
    column.set_margin_bottom(6);
    column.set_margin_start(6);
    column.set_margin_end(6);
    let title = gtk::Label::builder().label(label).xalign(0.0).build();
    title.add_css_class("heading");
    column.append(&title);
    match chip.kind {
        ChipKind::Date => {
            let calendar = gtk::Calendar::new();
            if let Ok(d) = chips::NaiveDate::parse_from_str(&chip.value, "%Y-%m-%d") {
                use chips::Datelike;
                if let Ok(dt) = glib::DateTime::from_local(d.year(), d.month() as i32, d.day() as i32, 12, 0, 0.0) {
                    calendar.select_day(&dt);
                }
            }
            let (buf, pop2) = (buf.clone(), pop.clone());
            calendar.connect_day_selected(move |c| {
                let dt = c.date();
                let Some(d) = chips::NaiveDate::from_ymd_opt(dt.year(), dt.month() as u32, dt.day_of_month() as u32) else { return };
                replace(&buf, range, chips::date_chip(d));
                pop2.popdown();
            });
            column.append(&calendar);
        }
        ChipKind::Person | ChipKind::Link => {
            let uri = chips::chip_run(chip.clone(), label).style.link;
            let value = gtk::Label::builder().label(&chip.value).xalign(0.0).selectable(true).build();
            value.add_css_class("dim-label");
            column.append(&value);
            if let Some(uri) = uri {
                let open = gtk::Button::with_label(if chip.kind == ChipKind::Link { "Open Link" } else { "Send Email" });
                open.add_css_class("suggested-action");
                let pop2 = pop.clone();
                open.connect_clicked(move |b| {
                    let window = b.root().and_downcast::<gtk::Window>();
                    gtk::UriLauncher::new(&uri).launch(window.as_ref(), gtk::gio::Cancellable::NONE, |_| {});
                    pop2.popdown();
                });
                column.append(&open);
            }
        }
    }
    pop.set_child(Some(&column));
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    fn tab(text: &str) -> gtk::TextBuffer {
        let buf = gtk::TextBuffer::new(None);
        crate::actions::register_formatting_tags(&buf);
        crate::live::LiveModel::attach(&buf);
        crate::bridge::load_document(&letters_core::Document::from_plain_text(text), &buf);
        buf
    }

    #[test]
    fn an_inserted_chip_is_one_object_in_the_model_and_its_label_in_draft() {
        gtk_test(|| {
            let buf = tab("Due @ ok");
            // The caret after the "@".
            buf.place_cursor(&buf.iter_at_offset(5));
            let d = chips::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
            assert!(insert(&buf, chips::date_chip(d), true));
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            assert_eq!(text, "Due 3 Oct 2026 ok", "the chip's label replaces the @");
            let live = crate::live::of(&buf).unwrap();
            let doc = live.borrow_mut().document(&buf).clone();
            assert_eq!(letters_core::edit::doc_len(&doc), "Due ".len() + 1 + " ok".len(), "the chip is one char of the document");
            let chip = doc.paragraphs[0].runs.iter().find_map(|r| r.style.chip.clone()).unwrap();
            assert_eq!(chip, Chip { kind: ChipKind::Date, value: "2026-10-03".into() });
            // The Draft view reads the same document back.
            assert_eq!(crate::bridge::capture_with_starts(&buf).0, doc);
            // The chip is found at any of its label's offsets, with its span.
            let (found, label, range) = chip_at(&buf, 8).unwrap();
            assert_eq!((found.kind, label.as_str(), range), (ChipKind::Date, "3 Oct 2026", (4, 14)));
            // Changing the date is one step; undo brings the first date back.
            let d2 = chips::NaiveDate::from_ymd_opt(2026, 10, 4).unwrap();
            replace(&buf, range, chips::date_chip(d2));
            assert!(buf.text(&buf.start_iter(), &buf.end_iter(), false).contains("4 Oct 2026"));
            crate::live::undo(&buf, false);
            assert!(buf.text(&buf.start_iter(), &buf.end_iter(), false).contains("3 Oct 2026"));
            crate::live::undo(&buf, false);
            assert_eq!(buf.text(&buf.start_iter(), &buf.end_iter(), false), "Due @ ok");
        });
    }

    /// A click on a chip on the page finds it (from either half); a click
    /// on the text beside it does not. (GUI clicks do not reach the page
    /// view in the test harness, so this is checked here.)
    #[test]
    fn a_click_on_a_chip_on_the_page_finds_it() {
        gtk_test(|| {
            let buf = tab("Due  ok");
            buf.place_cursor(&buf.iter_at_offset(4));
            let d = chips::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
            insert(&buf, chips::date_chip(d), false);
            let view = crate::page_view::PageView::new();
            crate::page_edit::make_editable(&view, &buf);
            let (doc, starts) = crate::live::of(&buf).unwrap().borrow_mut().snapshot(&buf);
            let typeset = letters_core::layout::pango::Typeset::new(doc, letters_core::layout::LayoutOptions::default());
            view.set_typeset(typeset, starts);
            // The chip spans buffer 4..14 ("3 Oct 2026").
            let (x0, _, _) = view.caret_rect(4).unwrap();
            let (x1, _, _) = view.caret_rect(14).unwrap();
            assert!(x1 - x0 > 30.0, "the pill is as wide as its label: {x0}..{x1}");
            for (x, off) in [(x0 + 2.0, 4), (x1 - 2.0, 14)] {
                let hit = chip_under_click(&view, &buf, x, off).map(|c| (c.0.kind, c.2));
                assert_eq!(hit, Some((ChipKind::Date, (4, 14))), "click at {x}");
            }
            assert!(chip_under_click(&view, &buf, x0 - 20.0, 3).is_none(), "the text before it");
            assert!(chip_under_click(&view, &buf, x1 + 12.0, 16).is_none(), "the text after it");
        });
    }

    #[test]
    fn two_chips_side_by_side_stay_two() {
        gtk_test(|| {
            let buf = tab("");
            let d = chips::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
            insert(&buf, chips::date_chip(d), false);
            insert(&buf, chips::date_chip(d), false);
            let doc = crate::bridge::capture_with_starts(&buf).0;
            // Adjacent identical labels under one tag would read back as one
            // chip; the model keeps two objects and the capture must agree.
            let live = crate::live::of(&buf).unwrap();
            assert_eq!(live.borrow_mut().document(&buf).paragraphs[0].runs.len(), 2);
            assert_eq!(doc.paragraphs[0].runs.len(), 2, "{:?}", doc.paragraphs[0].runs);
        });
    }
}
