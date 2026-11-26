//! The gallery's message handlers: the arms of `update_with_view` that do
//! more than call one method.

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;

use crate::models::{GalleryItem, GallerySort};

use super::cell::fade_out;
use super::{
    key_of, thumb_cache_key, AttachmentsGallery, AttachmentsGalleryWidgets, FileKey,
    GalleryAccount, GalleryInput, GalleryOutput, SEARCH_DEBOUNCE_MS,
};

impl AttachmentsGallery {
    /// A page came back from the cache: `offset` 0 replaces the list, the
    /// next offset appends to it.
    pub(super) fn on_page(
        &mut self,
        items: Vec<GalleryItem>,
        total: u32,
        offset: u32,
        sender: &ComponentSender<Self>,
    ) {
        if offset == 0 {
            // The first page of a fresh query replaces everything.
            self.all_items = items;
        } else if offset as usize == self.all_items.len() {
            self.all_items.extend(items);
        } else {
            // A page for a query that has since changed: its offset no
            // longer lines up with what is loaded, so appending it
            // would interleave rows from two different orderings. Drop
            // it and wait for the page the current query asked for.
            self.loading_more = false;
            return;
        }
        self.total = total;
        self.loading = false;
        self.loading_more = false;
        self.rebuild_view(sender);
    }

    /// A message's attachments arrived: fill in the bytes of every loaded
    /// row of that message.
    pub(super) fn on_fetched(
        &mut self,
        account_id: u32,
        path: String,
        uid: u32,
        items: Vec<crate::models::Attachment>,
        sender: &ComponentSender<Self>,
    ) {
        let mut touched = false;
        for row in self.all_items.iter_mut().filter(|i| {
            i.account_id == account_id && i.folder_path == path && i.uid == uid && i.data.is_none()
        }) {
            // Match on the name rather than the index: the scan numbers
            // parts as the server describes them and the fetch numbers
            // them as the parser finds them, which need not agree.
            if let Some(found) = items.iter().find(|a| a.name == row.name) {
                row.data = Some(found.data.clone());
                row.size = found.data.len() as u64;
                row.downloaded = true;
                touched = true;
            }
        }
        if touched {
            self.rebuild_view(sender);
            if self.preview.is_some() {
                self.refresh_preview(sender);
            }
        }
    }

    pub(super) fn on_set_deleting(
        &mut self,
        key: FileKey,
        on: bool,
        sender: &ComponentSender<Self>,
    ) {
        self.deleting.retain(|k| *k != key);
        if on {
            self.deleting.push(key);
        }
        self.rebuild_view(sender);
    }

    /// Fade the removed file's cell or row out, then take it away.
    pub(super) fn on_removed(&mut self, key: FileKey, sender: &ComponentSender<Self>) {
        let at = self.all_items.iter().position(|i| key_of(i) == key);
        let shown: Option<gtk::Widget> = at.and_then(|at| {
            if self.view_table {
                self.table.row_at_index(at as i32).map(|r| r.upcast())
            } else {
                self.flow.child_at_index(at as i32).map(|c| c.upcast())
            }
        });
        let s = sender.clone();
        let done = move || s.input(GalleryInput::RemoveNow(key.clone()));
        match shown {
            Some(w) => fade_out(&w, done),
            None => done(),
        }
    }

    /// Take a removed file out of the list. False when it was no longer
    /// there: the list is rebuilt and the caller skips the view update.
    pub(super) fn on_remove_now(&mut self, key: FileKey, sender: &ComponentSender<Self>) -> bool {
        self.deleting.retain(|k| *k != key);
        // In place, rather than a reload that would lose the user's
        // place in a long list.
        let Some(at) = self.all_items.iter().position(|i| key_of(i) == key) else {
            self.rebuild_view(sender);
            return false;
        };
        self.all_items.remove(at);
        self.total = self.total.saturating_sub(1);
        self.preview = match self.preview {
            Some(p) if p == at => None,
            Some(p) if p > at => Some(p - 1),
            p => p,
        };
        self.rebuild_view(sender);
        true
    }

    /// The search text changed: run it once the typing settles.
    pub(super) fn on_set_query(&mut self, q: String, sender: &ComponentSender<Self>) {
        if self.query != q {
            self.query = q;
            if let Some(t) = self.query_timer.take() {
                t.remove();
            }
            let s = sender.clone();
            self.query_timer = Some(glib::timeout_add_local_once(
                std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS as u64),
                move || s.input(GalleryInput::ApplyQuery),
            ));
        }
    }

    pub(super) fn on_set_sort(&mut self, i: u32, sender: &ComponentSender<Self>) {
        let sort = GallerySort::from_index(i);
        if self.sort != sort {
            self.sort = sort;
            crate::config::save_gallery_sort(i);
            self.reload(sender);
        }
    }

    /// A table column header was clicked: sort by it, or flip its direction.
    pub(super) fn on_sort_column(
        &mut self,
        col: u8,
        widgets: &AttachmentsGalleryWidgets,
        sender: &ComponentSender<Self>,
    ) {
        // First click sorts a column its natural way; a second flips it.
        let sort = match (col, self.sort) {
            (0, GallerySort::Name) => GallerySort::NameDesc,
            (0, _) => GallerySort::Name,
            (1, GallerySort::Sender) => GallerySort::SenderDesc,
            (1, _) => GallerySort::Sender,
            (2, GallerySort::Newest) => GallerySort::Oldest,
            (2, _) => GallerySort::Newest,
            (3, GallerySort::Largest) => GallerySort::Smallest,
            (3, _) => GallerySort::Largest,
            (4, GallerySort::Type) => GallerySort::TypeDesc,
            (4, _) | (_, _) => GallerySort::Type,
        };
        // The dropdown follows; its notify handler sees the same value
        // and does nothing further.
        widgets.sort_dropdown.set_selected(sort.index());
        if self.sort != sort {
            self.sort = sort;
            crate::config::save_gallery_sort(sort.index());
            self.reload(sender);
        }
    }

    pub(super) fn on_set_view_table(&mut self, table: bool, sender: &ComponentSender<Self>) {
        if self.view_table != table {
            self.view_table = table;
            self.rebuild_view(sender);
            crate::config::save_gallery_table_view(table);
        }
    }

    /// The size slider moved: one rebuild once the drag settles, not one
    /// per pixel.
    pub(super) fn on_set_thumb_width(&mut self, v: f64, sender: &ComponentSender<Self>) {
        let width = (v.round() as i32).clamp(140, 380);
        if width != self.thumb_width {
            self.thumb_width = width;
            if let Some(id) = self.resize_timer.take() {
                id.remove();
            }
            let s = sender.clone();
            self.resize_timer = Some(glib::timeout_add_local_once(
                std::time::Duration::from_millis(150),
                move || s.input(GalleryInput::ApplyThumbWidth),
            ));
        }
    }

    pub(super) fn on_apply_thumb_width(&mut self, sender: &ComponentSender<Self>) {
        self.resize_timer = None;
        if !self.view_table {
            self.rebuild_view(sender);
        }
        crate::config::save_gallery_thumb_width(self.thumb_width);
    }

    pub(super) fn on_set_accounts(
        &mut self,
        accounts: Vec<GalleryAccount>,
        widgets: &AttachmentsGalleryWidgets,
        sender: &ComponentSender<Self>,
    ) {
        self.accounts = accounts;
        self.rebuild_account_names();
        // The dropdown's rows just changed under it; start from "All
        // accounts" rather than whichever row the old list had there.
        self.account_filter = None;
        widgets.account_dropdown.set_selected(0);
        self.recompute_scope();
        self.rebuild_folder_list(sender);
        self.reload(sender);
    }

    pub(super) fn on_set_account_filter(
        &mut self,
        row: u32,
        widgets: &AttachmentsGalleryWidgets,
        sender: &ComponentSender<Self>,
    ) {
        // Row 0 is "All accounts"; the rest index `accounts` in order.
        let filter = (row as usize)
            .checked_sub(1)
            .and_then(|i| self.accounts.get(i))
            .map(|a| a.id);
        if self.account_filter != filter {
            self.account_filter = filter;
            // Keeps the dropdown honest when the message came from
            // somewhere other than the dropdown itself. Setting it to
            // the row it already holds emits nothing, so this cannot
            // loop back round.
            widgets.account_dropdown.set_selected(row);
            self.reload(sender);
        }
    }

    pub(super) fn on_set_folder(
        &mut self,
        account_id: u32,
        path: String,
        on: bool,
        sender: &ComponentSender<Self>,
    ) {
        let entry = self.overrides.entry(account_id).or_default();
        if entry.insert(path, on) != Some(on) {
            // The list already draws this tick; rebuilding it here
            // would tear down the check button mid-signal.
            self.recompute_scope();
            self.save_scope();
            self.reload(sender);
        }
    }

    /// A lightbox-size PDF render landed: show it if that PDF is still the
    /// one on show.
    pub(super) fn on_preview_rendered(&mut self, key: u64, sender: &ComponentSender<Self>) {
        let still_current = self
            .current()
            .and_then(|i| i.data.as_ref())
            .is_some_and(|d| thumb_cache_key(d) == key);
        if still_current {
            self.refresh_preview(sender);
        }
    }

    /// Ask the app to take item `i` off the server (it asks the user first).
    pub(super) fn on_delete_item(&self, i: usize, sender: &ComponentSender<Self>) {
        if let Some(item) = self.item_at(i) {
            let _ = sender.output(GalleryOutput::DeleteFromServer {
                account_id: item.account_id,
                folder_path: item.folder_path.clone(),
                uid: item.uid,
                name: item.name.clone(),
                size: item.size,
            });
        }
    }

    pub(super) fn on_showcase_delete(&self, name: String, sender: &ComponentSender<Self>) {
        match self.all_items.iter().position(|i| i.name == name) {
            Some(i) => sender.input(GalleryInput::DeleteItem(i)),
            None => tracing::warn!("showcase: no {name:?} in the gallery"),
        }
    }

    pub(super) fn on_showcase_search(&self, text: String, widgets: &AttachmentsGalleryWidgets) {
        let entry = widgets.search_entry.clone();
        entry.grab_focus();
        // Goes through `search-changed`, so this is the same path a
        // typed character takes, debounce and all.
        entry.set_text(&text);
        glib::timeout_add_seconds_local_once(2, move || {
            // `has_focus` also needs the window to be the active one,
            // which it need not be under a private bus; `is_focus` is
            // the question actually being asked — is this still the
            // toplevel's focus widget.
            let focus = entry
                .root()
                .and_downcast::<gtk::Window>()
                .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w));
            // A GtkSearchEntry is composite: the focus widget is the
            // GtkText inside it, so ask whether the focus is anywhere
            // within the entry rather than whether it *is* the entry.
            let kept = focus
                .as_ref()
                .is_some_and(|f| f == entry.upcast_ref::<gtk::Widget>() || f.is_ancestor(&entry));
            tracing::info!(
                target: "hylki::showcase",
                "gallery search focus: kept={kept} holder={} toolbar_visible={} text={:?}",
                focus.map(|w| w.type_().name().to_string()).unwrap_or_else(|| "none".into()),
                entry.parent().is_some_and(|p| p.is_visible()),
                entry.text()
            );
        });
    }

    pub(super) fn on_showcase_folders(&self, widgets: &AttachmentsGalleryWidgets) {
        widgets.folders_button.popup();
        // A popover is its own surface, so the window snapshot never
        // has it: capture its contents directly, as the context menu
        // does (see `ui::context_menu`).
        if let Ok(path) = std::env::var("HYLKI_SHOWCASE") {
            let content = widgets.folders_box.clone();
            glib::timeout_add_seconds_local_once(1, move || {
                crate::ui::showcase::showcase_capture(content.upcast_ref(), &path);
            });
        }
    }
}
