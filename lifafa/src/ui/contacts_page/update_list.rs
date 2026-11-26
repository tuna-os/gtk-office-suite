//! Messages about what the list shows: loading, searching, filtering,
//! sorting, the scope picked in the sidebar and the LDAP directories.

use adw::prelude::*;
use relm4::prelude::*;

use crate::contacts::{ContactDetails, ContactEdit};

use super::menus::matches_query;
use super::rows::same_contacts;
use super::types::{
    ContactFilter, ContactScope, ContactSort, ContactsPage, ContactsPageInput, ContactsPageOutput,
    DIRECTORY, DIRECTORY_LIMIT,
};
use super::{ContactsPageWidgets, Flow};

impl ContactsPage {
    pub(super) fn on_set_loading(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
    ) -> Flow {
        self.loading = true;
        if self.contacts.is_empty() && self.directories.is_empty() {
            widgets.page_stack.set_visible_child_name("none");
        }
        let s = sender.input_sender().clone();
        std::thread::spawn(move || {
            let _ = s.send(ContactsPageInput::DirectoriesKnown(
                crate::contacts::directory::enabled_names(),
            ));
        });
        Flow::Continue(())
    }

    pub(super) fn on_set_contacts(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
        mut contacts: Vec<ContactDetails>,
    ) -> Flow {
        self.pending = None;
        // Drop tombstoned (locally deleted) entries the read still
        // carries; a tombstone retires once a read comes back clean.
        let incoming: std::collections::HashSet<(String, String)> = contacts
            .iter()
            .map(|c| (c.book_uid.clone(), c.eds_uid.clone()))
            .collect();
        self.deleted.retain(|key| incoming.contains(key));
        contacts.retain(|c| {
            !self
                .deleted
                .contains(&(c.book_uid.clone(), c.eds_uid.clone()))
        });
        // Every visit re-reads the books, and a sync behind it reads
        // them again; unchanged, the list on screen stays (#477).
        if !contacts.is_empty() && same_contacts(&self.contacts, &contacts) {
            // In the same order, so every index still holds: the
            // fresh cards (an edit patches them) without new rows.
            self.contacts = contacts;
            self.loading = false;
            // Their photos too, which the rows have as thumbnails.
            self.decode_photos(Default::default(), &sender);
            self.update_view(widgets, sender);
            return Flow::Break(());
        }
        // Keep the same person selected across a refresh, by identity.
        let who = |c: &ContactDetails| (c.name.clone(), c.primary_email().map(str::to_string));
        let on_hit = self.selected.filter(|&i| i >= DIRECTORY);
        let keep = self.selected.and_then(|i| self.contacts.get(i)).map(who);
        // A directory entry just added to Contacts is that contact now; one
        // that was a contact already stays the directory's entry.
        let added = on_hit
            .and_then(|i| self.entry(i))
            .map(who)
            .filter(|hit| !self.contacts.iter().any(|c| who(c) == *hit));
        // And those selected together, the same way.
        let identity = |c: &ContactDetails| {
            (
                c.book_uid.clone(),
                c.eds_uid.clone(),
                c.name.clone(),
                c.primary_email().map(str::to_string),
            )
        };
        let multi: Vec<_> = self
            .multi
            .iter()
            .filter_map(|&i| self.contacts.get(i))
            .map(identity)
            .collect();
        self.multi = contacts
            .iter()
            .enumerate()
            .filter(|(_, c)| multi.contains(&identity(c)))
            .map(|(i, _)| i)
            .collect();
        self.items = self.items_for(&contacts);
        self.contacts = contacts;
        self.loading = false;
        self.editor = None;
        self.editing_target = None;
        self.selected = keep
            .or(added)
            .and_then(|person| self.contacts.iter().position(|c| who(c) == person))
            .or(on_hit);
        if self.selected.is_some_and(|i| self.entry(i).is_none()) {
            self.selected = None;
        }
        self.refresh_book_filter();
        self.rebuild(widgets, &sender);
        Flow::Continue(())
    }

    pub(super) fn on_query(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
        q: String,
    ) -> Flow {
        self.query = q.to_lowercase();
        self.directory_generation = self.directory_generation.wrapping_add(1);
        let typed = q.trim().to_string();
        if typed.chars().count() < crate::contacts::directory::MIN_QUERY {
            self.directory_hits.clear();
            self.directory_failures.clear();
        } else {
            // What the directories found before narrows with the
            // search until they answer this one.
            let query = self.query.clone();
            self.directory_hits
                .retain(|c| matches_query(c, query.trim()));
            // Asked once the typing pauses, not per key.
            let generation = self.directory_generation;
            let s = sender.input_sender().clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                let _ = s.send(ContactsPageInput::DirectoryLookup {
                    generation,
                    query: typed,
                });
            });
        }
        if self.selected.is_some_and(|i| self.entry(i).is_none()) {
            self.selected = None;
        }
        self.rebuild(widgets, &sender);
        Flow::Continue(())
    }

    pub(super) fn on_set_search(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        text: String,
    ) -> Flow {
        self.search_open = true;
        widgets.search.set_text(&text);
        Flow::Continue(())
    }

    pub(super) fn on_focus_search(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
    ) -> Flow {
        if self.search_pinned {
            widgets.search.grab_focus();
        } else if self.search_open {
            sender.input(ContactsPageInput::CloseSearch);
        } else if self
            .search_closed_at
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(300))
        {
            self.search_open = true;
            // Once the revealer has mapped it.
            let entry = widgets.search.clone();
            gtk::glib::idle_add_local_once(move || {
                entry.grab_focus();
            });
        }
        Flow::Continue(())
    }

    pub(super) fn on_close_search(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
    ) -> Flow {
        let had_focus = widgets.search.has_focus() || widgets.search.focus_child().is_some();
        if !self.search_pinned {
            self.search_open = false;
            self.search_closed_at = Some(std::time::Instant::now());
        }
        // A hidden search narrows nothing: its text goes.
        let narrowed = !self.query.is_empty();
        self.query.clear();
        widgets.search.set_text("");
        self.directory_generation = self.directory_generation.wrapping_add(1);
        self.directory_hits.clear();
        self.directory_failures.clear();
        if narrowed {
            self.rebuild(widgets, &sender);
        }
        if had_focus {
            widgets.list.grab_focus();
        }
        Flow::Continue(())
    }

    pub(super) fn on_set_book_picker(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        on: bool,
    ) -> Flow {
        if self.book_picker != on {
            self.book_picker = on;
            self.sync_book_picker(widgets);
        }
        Flow::Continue(())
    }

    pub(super) fn on_set_search_pinned(&mut self, on: bool) -> Flow {
        self.search_pinned = on;
        Flow::Continue(())
    }

    pub(super) fn on_set_filter(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
        filter: ContactFilter,
        on: bool,
    ) -> Flow {
        let filters = if on {
            self.filters | filter.bit()
        } else {
            self.filters & !filter.bit()
        };
        if filters != self.filters {
            self.filters = filters;
            self.sync_filter_menu();
            self.rebuild(widgets, &sender);
        }
        Flow::Continue(())
    }

    pub(super) fn on_clear_filters(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
    ) -> Flow {
        if self.filters != 0 {
            self.filters = 0;
            self.sync_filter_menu();
            self.rebuild(widgets, &sender);
        }
        Flow::Continue(())
    }

    pub(super) fn on_suggestions_loaded(
        &mut self,
        suggestions: Vec<crate::contacts::Suggestion>,
    ) -> Flow {
        *self.suggestions.borrow_mut() = suggestions;
        Flow::Continue(())
    }

    pub(super) fn on_pick_book(&mut self, sender: ComponentSender<Self>, n: u32) -> Flow {
        let scope = match (n as usize)
            .checked_sub(1)
            .and_then(|i| self.filter_books.get(i))
        {
            Some((uid, _)) => ContactScope::Book(uid.clone()),
            None => ContactScope::All,
        };
        sender.input(ContactsPageInput::SetScope(scope));
        Flow::Continue(())
    }

    pub(super) fn on_directories_known(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
        directories: Vec<(String, String)>,
    ) -> Flow {
        if directories == self.directories {
            return Flow::Break(());
        }
        self.directories = directories;
        self.refresh_book_filter();
        self.publish_tree(&sender);
        if !self.directories.is_empty() && self.contacts.is_empty() && !self.loading {
            widgets.page_stack.set_visible_child_name("browser");
        }
        Flow::Continue(())
    }

    pub(super) fn on_directory_lookup(
        &mut self,
        sender: ComponentSender<Self>,
        generation: u64,
        query: String,
    ) -> Flow {
        if generation != self.directory_generation {
            return Flow::Break(());
        }
        let s = sender.input_sender().clone();
        std::thread::spawn(move || {
            let (hits, failed) =
                crate::contacts::directory::search_contacts(&query, DIRECTORY_LIMIT);
            let _ = s.send(ContactsPageInput::DirectoryResults {
                generation,
                hits,
                failed,
            });
        });
        Flow::Continue(())
    }

    pub(super) fn on_directory_results(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
        generation: u64,
        hits: Vec<ContactDetails>,
        failed: Vec<crate::contacts::directory::DirectoryFailure>,
    ) -> Flow {
        if generation != self.directory_generation {
            return Flow::Break(());
        }
        self.directory_failures = failed;
        // The entry shown may be at another place in the new list.
        let shown = self
            .selected
            .filter(|&i| i >= DIRECTORY)
            .and_then(|i| self.entry(i))
            .map(|c| (c.name.clone(), c.primary_email().map(str::to_string)));
        self.directory_hits = hits;
        if let Some((name, email)) = shown {
            self.selected = self
                .directory_hits
                .iter()
                .position(|c| c.name == name && c.primary_email().map(str::to_string) == email)
                .map(|i| DIRECTORY + i);
        }
        // An open editor stays as it is; the list catches up with
        // the next change.
        if self.editor.is_none() {
            self.rebuild(widgets, &sender);
        }
        Flow::Continue(())
    }

    pub(super) fn on_add_directory_hit(
        &mut self,
        sender: ComponentSender<Self>,
        idx: usize,
    ) -> Flow {
        let Some(c) = idx
            .checked_sub(DIRECTORY)
            .and_then(|i| self.directory_hits.get(i))
        else {
            return Flow::Break(());
        };
        // A card of its own, not the directory's, whose UID is the
        // directory's name for the entry.
        let edit = ContactEdit {
            name: c.name.clone(),
            nickname: c.nickname.clone(),
            org: c.org.clone(),
            title: c.title.clone(),
            note: c.note.clone(),
            emails: c.emails.clone(),
            phones: c.phones.clone(),
            urls: c.urls.clone(),
            addresses: c.postal.clone(),
            birthday: c.birthday_raw.clone(),
            photo: c.photo_bytes().map_or(
                crate::contacts::PhotoEdit::Keep,
                crate::contacts::PhotoEdit::Set,
            ),
        };
        let _ = sender.output(ContactsPageOutput::CreateContact {
            book_uid: None,
            vcard: crate::contacts::new_vcard(&edit),
        });
        Flow::Continue(())
    }

    pub(super) fn on_set_sort(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
        sort: ContactSort,
    ) -> Flow {
        if self.sort != sort {
            self.sort = sort;
            self.sync_filter_menu();
            self.rebuild(widgets, &sender);
        }
        Flow::Continue(())
    }

    pub(super) fn on_set_scope(
        &mut self,
        widgets: &mut ContactsPageWidgets,
        sender: ComponentSender<Self>,
        scope: ContactScope,
    ) -> Flow {
        if self.scope != scope {
            self.scope = scope;
            // Another list abandons an open editor and what was
            // selected in this one.
            self.editor = None;
            self.editing_target = None;
            self.selected = None;
            self.multi.clear();
            self.rebuild(widgets, &sender);
        }
        Flow::Continue(())
    }

    pub(super) fn on_set_books(
        &mut self,
        sender: ComponentSender<Self>,
        books: Vec<crate::contacts::Book>,
    ) -> Flow {
        self.books = books;
        self.refresh_book_filter();
        self.publish_tree(&sender);
        Flow::Continue(())
    }

    pub(super) fn on_photos_decoded(&mut self) -> Flow {
        for row in self.rows.borrow().values() {
            row.show_photo();
        }
        self.decoding
            .retain(|key| crate::services::avatar::photo_thumbnail(*key).is_none());
        Flow::Continue(())
    }
}
