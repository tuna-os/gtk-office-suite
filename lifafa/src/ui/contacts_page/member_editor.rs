//! The group form's member list, and reading the contact form back.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use relm4::prelude::*;

use crate::contacts::{ContactEdit, Labeled, Member};
use crate::i18n::{i18n, i18n_f};

use super::forms::{AddressForm, ValueRows};
use super::rows::{flat_button, group};
use super::types::ContactForm;

/// Who matches a typed text ([`member_matcher`]).
type Matcher = Rc<dyn Fn(&str) -> Vec<Pick>>;

/// Adds a picked person and starts the field over.
type Take = Rc<dyn Fn(&Pick, &adw::EntryRow)>;

/// One of the people the group form offers for what is typed.
#[derive(Clone)]
struct Pick {
    member: Member,
    title: String,
    subtitle: String,
    /// From Contacts, not from mail history.
    contact: bool,
}

/// The group form's members: a row each with a remove button, and a row to
/// add someone, by an address or by picking from `people` (the composer's
/// suggestions, by address) as a name or address is typed. `no_address`
/// are the contacts with none, offered too while `cards` says the group's
/// book names its members by contact (they cannot be in a list of
/// addresses). Returns the live list read at save time.
pub(super) fn member_editor(
    detail: &gtk::Box,
    initial: &[Member],
    people: Rc<RefCell<Vec<crate::contacts::Suggestion>>>,
    no_address: Rc<Vec<(Member, String)>>,
    cards: Rc<Cell<bool>>,
) -> Rc<RefCell<Vec<Member>>> {
    let list = group(detail, &i18n("Members"));
    let members: Rc<RefCell<Vec<Member>>> = Rc::new(RefCell::new(Vec::new()));
    let entry = adw::EntryRow::new();
    entry.set_title(&i18n("Add a person by name or address"));
    entry.set_show_apply_button(true);
    list.append(&entry);
    // The people matching what is typed, under the members.
    let found = gtk::ListBox::new();
    found.add_css_class("boxed-list");
    found.add_css_class("member-matches");
    found.set_selection_mode(gtk::SelectionMode::Single);
    found.set_can_focus(false);
    found.set_margin_top(6);
    found.set_visible(false);
    detail.append(&found);

    let add = member_adder(&list, &entry, &members);
    for m in initial {
        add(m.clone());
    }

    let matches = member_matcher(people.clone(), members.clone(), no_address, cards);
    // The matches listed, and the one Return takes: the first while a name
    // is typed, none while the text is an address (Return adds what was
    // typed), whichever the arrow keys move to.
    let shown: Rc<RefCell<Vec<Pick>>> = Rc::new(RefCell::new(Vec::new()));
    let chosen: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let highlight = {
        let (found, chosen) = (found.downgrade(), chosen.clone());
        move |at: Option<usize>| {
            chosen.set(at);
            let Some(found) = found.upgrade() else { return };
            match at.and_then(|i| found.row_at_index(i as i32)) {
                Some(row) => found.select_row(Some(&row)),
                None => found.unselect_all(),
            }
        }
    };
    let highlight: Rc<dyn Fn(Option<usize>)> = Rc::new(highlight);
    // Add a match as a member and start the field over.
    let take = {
        let add = add.clone();
        move |pick: &Pick, entry: &adw::EntryRow| {
            add(pick.member.clone());
            entry.set_text("");
            entry.grab_focus();
        }
    };
    let take: Take = Rc::new(take);
    show_matches_as_typed(
        &entry,
        &found,
        matches,
        shown.clone(),
        highlight.clone(),
        take.clone(),
    );
    move_through_matches(
        &entry,
        &found,
        shown.clone(),
        chosen.clone(),
        highlight.clone(),
    );
    // Return or the apply button: the match picked, else the addresses typed
    // or pasted.
    let commit = move |e: &adw::EntryRow| {
        let picked = chosen.get().and_then(|i| shown.borrow().get(i).cloned());
        if let Some(sug) = picked.filter(|_| found.is_visible()) {
            take(&sug, e);
            return;
        }
        let typed = typed_addresses(&e.text());
        if typed.is_empty() {
            e.add_css_class("error");
            return;
        }
        for (name, email) in typed {
            // An address typed bare keeps the name Contacts or mail has for it.
            let name = if name.is_empty() || name == email {
                people
                    .borrow()
                    .iter()
                    .find(|s| s.email.eq_ignore_ascii_case(&email) && s.name != s.email)
                    .map(|s| s.name.clone())
                    .unwrap_or_default()
            } else {
                name
            };
            add(Member {
                name,
                email,
                ..Member::default()
            });
        }
        e.set_text("");
    };
    let commit = Rc::new(commit);
    {
        let commit = commit.clone();
        entry.connect_apply(move |e| commit(e));
    }
    entry.connect_entry_activated(move |e| commit(e));
    entry.connect_changed(|e| e.remove_css_class("error"));
    members
}

/// The function that adds a member to the form: a row with a remove
/// button above the entry, unless they are in already.
fn member_adder(
    list: &gtk::ListBox,
    entry: &adw::EntryRow,
    members: &Rc<RefCell<Vec<Member>>>,
) -> Rc<dyn Fn(Member)> {
    // Widgets are held weakly in the handlers they own, so the form is
    // freed when the pane is cleared.
    let (list, members, entry) = (list.downgrade(), members.clone(), entry.downgrade());
    Rc::new(move |m: Member| {
        let (Some(list), Some(entry)) = (list.upgrade(), entry.upgrade()) else {
            return;
        };
        if members.borrow().iter().any(|x| x.same(&m)) {
            return;
        }
        let row = adw::ActionRow::new();
        let named = !m.name.trim().is_empty() && m.name != m.email;
        row.set_title(&gtk::glib::markup_escape_text(if named {
            &m.name
        } else {
            &m.email
        }));
        if m.email.is_empty() {
            row.set_subtitle(&i18n("No email address"));
        } else if named {
            row.set_subtitle(&gtk::glib::markup_escape_text(&m.email));
        }
        let remove = flat_button("user-trash-symbolic", &i18n("Remove"));
        {
            let (list, row, members, gone) = (
                list.downgrade(),
                row.downgrade(),
                members.clone(),
                m.clone(),
            );
            remove.connect_clicked(move |_| {
                members.borrow_mut().retain(|x| !x.same(&gone));
                if let (Some(list), Some(row)) = (list.upgrade(), row.upgrade()) {
                    list.remove(&row);
                }
            });
        }
        row.add_suffix(&remove);
        list.insert(&row, entry.index());
        members.borrow_mut().push(m);
    })
}

/// Who matches what is typed, as the group form offers them.
fn member_matcher(
    people: Rc<RefCell<Vec<crate::contacts::Suggestion>>>,
    members: Rc<RefCell<Vec<Member>>>,
    no_address: Rc<Vec<(Member, String)>>,
    cards: Rc<Cell<bool>>,
) -> Matcher {
    // Who matches the typing, by name or address and not already in,
    // ranked as the composer ranks them; then the contacts with no address
    // whose name matches.
    Rc::new(move |typed: &str| -> Vec<Pick> {
        let q = typed.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let taken: std::collections::HashSet<String> = members
            .borrow()
            .iter()
            .map(|m| m.email.to_lowercase())
            .collect();
        let mut found: Vec<crate::contacts::Suggestion> = people
            .borrow()
            .iter()
            .filter(|s| {
                s.group.is_none() && !taken.contains(&s.email.to_lowercase()) && s.matches(q)
            })
            .cloned()
            .collect();
        crate::contacts::rank(&mut found, q);
        found.truncate(6);
        let mut picks: Vec<Pick> = found
            .into_iter()
            .map(|sug| Pick {
                // History may know an address without a name.
                member: Member {
                    name: if sug.name == sug.email {
                        String::new()
                    } else {
                        sug.name.clone()
                    },
                    email: sug.email.clone(),
                    ..Member::default()
                },
                subtitle: if sug.name != sug.email {
                    sug.email.clone()
                } else {
                    String::new()
                },
                title: sug.name,
                contact: sug.from_contacts,
            })
            .collect();
        if cards.get() {
            let q = q.to_lowercase();
            let members = members.borrow();
            picks.extend(
                no_address
                    .iter()
                    .filter(|(m, _)| {
                        m.name.to_lowercase().contains(&q) && !members.iter().any(|x| x.same(m))
                    })
                    .take(4)
                    .map(|(m, book)| Pick {
                        member: m.clone(),
                        title: m.name.clone(),
                        subtitle: i18n_f("No email address · {book}", &[("book", book)]),
                        contact: true,
                    }),
            );
        }
        picks
    })
}

/// List the people matching the typing under the field as it changes.
fn show_matches_as_typed(
    entry: &adw::EntryRow,
    found: &gtk::ListBox,
    matches: Matcher,
    shown: Rc<RefCell<Vec<Pick>>>,
    highlight: Rc<dyn Fn(Option<usize>)>,
    take: Take,
) {
    let found = found.downgrade();
    entry.connect_changed(move |e| {
        let Some(found) = found.upgrade() else { return };
        while let Some(child) = found.first_child() {
            found.remove(&child);
        }
        let text = e.text();
        *shown.borrow_mut() = matches(&text);
        for sug in shown.borrow().iter() {
            let row = adw::ActionRow::new();
            row.set_title(&gtk::glib::markup_escape_text(&sug.title));
            if !sug.subtitle.is_empty() {
                row.set_subtitle(&gtk::glib::markup_escape_text(&sug.subtitle));
            }
            // Where the address came from, as in the composer: the
            // address book or mail history.
            let icon = gtk::Image::from_icon_name(if sug.contact {
                "avatar-default-symbolic"
            } else {
                "document-open-recent-symbolic"
            });
            icon.add_css_class("dim-label");
            row.add_prefix(&icon);
            row.set_activatable(true);
            // The typing stays in the field while the arrows move here.
            row.set_can_focus(false);
            let (take, entry, sug) = (take.clone(), e.downgrade(), sug.clone());
            row.connect_activated(move |_| {
                if let Some(entry) = entry.upgrade() {
                    take(&sug, &entry);
                }
            });
            found.append(&row);
        }
        let any = found.first_child().is_some();
        found.set_visible(any);
        let address = !typed_addresses(&text).is_empty();
        highlight((any && !address).then_some(0));
    });
}

/// The keys of the field: Down and Up move through the matches, Up from
/// the first goes back to what was typed, and Escape puts the list away.
fn move_through_matches(
    entry: &adw::EntryRow,
    found: &gtk::ListBox,
    shown: Rc<RefCell<Vec<Pick>>>,
    chosen: Rc<Cell<Option<usize>>>,
    highlight: Rc<dyn Fn(Option<usize>)>,
) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let found = found.downgrade();
    keys.connect_key_pressed(move |_, keyval, _, _| {
        use gtk::glib::Propagation;
        let Some(found) = found.upgrade().filter(|f| f.is_visible()) else {
            return Propagation::Proceed;
        };
        let last = shown.borrow().len().saturating_sub(1);
        if keyval == gtk::gdk::Key::Down || keyval == gtk::gdk::Key::KP_Down {
            highlight(Some(chosen.get().map_or(0, |i| (i + 1).min(last))));
        } else if keyval == gtk::gdk::Key::Up || keyval == gtk::gdk::Key::KP_Up {
            highlight(chosen.get().and_then(|i| i.checked_sub(1)));
        } else if keyval == gtk::gdk::Key::Escape {
            highlight(None);
            found.set_visible(false);
        } else {
            return Propagation::Proceed;
        }
        Propagation::Stop
    });
    entry.add_controller(keys);
}

/// The whole addresses in a typed or pasted text, as (name, address).
fn typed_addresses(text: &str) -> Vec<(String, String)> {
    crate::worker::parse_recipients(text)
        .into_iter()
        .filter(|(_, email)| {
            email.contains('@') && crate::worker::invalid_recipient(email).is_none()
        })
        .collect()
}

/// Read the editor form back into a `ContactEdit`.
pub(super) fn collect_edit(e: &ContactForm) -> ContactEdit {
    let values = |rows: &ValueRows| -> Vec<Labeled> {
        let rows = rows.borrow();
        rows.rows
            .iter()
            .filter_map(|(entry, label)| {
                let value = entry.text().trim().to_string();
                let label = label
                    .as_ref()
                    .and_then(|d| rows.labels.get(d.selected() as usize))
                    .map_or(String::new(), |l| l.to_string());
                (!value.is_empty()).then_some(Labeled { label, value })
            })
            .collect()
    };
    let buffer = e.note.buffer();
    ContactEdit {
        addresses: e
            .addresses
            .borrow()
            .iter()
            .map(AddressForm::read)
            .filter(|a| !a.is_empty())
            .collect(),
        birthday: e.birthday.read(),
        photo: e.photo.borrow().clone(),
        name: e.name.text().trim().to_string(),
        nickname: e.nickname.text().trim().to_string(),
        org: e.org.text().trim().to_string(),
        title: e.title.text().trim().to_string(),
        note: buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_string(),
        emails: values(&e.emails),
        phones: values(&e.phones),
        urls: values(&e.urls).into_iter().map(|l| l.value).collect(),
    }
}
