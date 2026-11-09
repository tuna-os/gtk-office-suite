//! Which folders feed the gallery: the per-kind defaults, the two master
//! switches, the user's own ticks, and the footer's folder list that shows
//! them.

use std::collections::HashMap;

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;

use crate::models::FolderKind;

use super::{AttachmentsGallery, GalleryInput};

/// One folder offered in the gallery's folder list.
#[derive(Debug, Clone)]
pub(crate) struct GalleryFolder {
    pub(crate) path: String,
    /// Display name, as the sidebar spells it.
    pub(crate) name: String,
    pub(crate) kind: FolderKind,
}

/// One account's share of the folder list: the account's own label and the
/// folders whose attachments the gallery could draw on.
#[derive(Debug, Clone)]
pub(crate) struct GalleryAccount {
    pub(crate) id: u32,
    pub(crate) label: String,
    pub(crate) folders: Vec<GalleryFolder>,
}

/// Whether a folder of this kind feeds the gallery unless the user says
/// otherwise. Sent is the one eligible kind that starts off: what you sent is
/// rarely what you are looking for, and its attachments are usually copies of
/// files you already have. Drafts, Junk and Trash never reach the gallery at
/// all (the cache query drops them), so they are not offered here.
fn kind_default(kind: FolderKind) -> bool {
    !matches!(kind, FolderKind::Sent)
}

/// Which master switch governs a folder kind: Archive folders follow "Include
/// Archive", anything that is neither an inbox nor an archive follows "Include
/// other folders", and inboxes follow neither (they are always on).
fn kind_master(kind: FolderKind) -> Option<Master> {
    match kind {
        FolderKind::Inbox => None,
        FolderKind::Archive => Some(Master::Archive),
        _ => Some(Master::Other),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Master {
    Archive,
    Other,
}

/// Whether a folder's attachments are in scope: its master switch has to be on
/// *and* it has to be ticked. A master that is off wins over a tick rather than
/// clearing it, so turning the master back on restores each folder to whatever
/// the user had chosen. `tick` is the user's own choice, or `None` when they
/// have not touched this folder and its kind's default stands.
fn in_scope(
    kind: FolderKind,
    include_archive: bool,
    include_other: bool,
    tick: Option<bool>,
) -> bool {
    let master_on = match kind_master(kind) {
        Some(Master::Archive) => include_archive,
        Some(Master::Other) => include_other,
        None => true,
    };
    master_on && tick.unwrap_or_else(|| kind_default(kind))
}

impl AttachmentsGallery {
    /// Whether attachments from this folder are in scope. A folder the app has
    /// not listed (a new one on the server, or the list not having arrived yet)
    /// counts as in scope, matching the worker's own default.
    pub(super) fn folder_included(&self, account_id: u32, path: &str) -> bool {
        self.included
            .get(&account_id)
            .and_then(|m| m.get(path))
            .copied()
            .unwrap_or(true)
    }

    /// Recompute the effective per-folder verdict from the two master switches
    /// and the user's own ticks. A master switch that is off wins over a tick,
    /// so turning "Include Archive" back on restores each archive folder to
    /// whatever the user had chosen for it.
    pub(super) fn recompute_scope(&mut self) {
        self.included = self
            .accounts
            .iter()
            .map(|acct| {
                let ticks = self.overrides.get(&acct.id);
                let folders = acct
                    .folders
                    .iter()
                    .map(|f| {
                        let tick = ticks.and_then(|m| m.get(&f.path)).copied();
                        let on = in_scope(f.kind, self.include_archive, self.include_other, tick);
                        (f.path.clone(), on)
                    })
                    .collect();
                (acct.id, folders)
            })
            .collect();
    }

    /// The user's own tick for a folder, before the master switches have their
    /// say — what the folder list shows in its check box.
    fn folder_ticked(&self, account_id: u32, path: &str, kind: FolderKind) -> bool {
        self.overrides
            .get(&account_id)
            .and_then(|m| m.get(path))
            .copied()
            .unwrap_or_else(|| kind_default(kind))
    }

    /// Rebuild the footer popover's folder list: every account's folders, each
    /// with a check box, under the account's own label when there is more than
    /// one account. Rows whose master switch is off are shown insensitive, so
    /// it is clear the switch and not the tick is what is holding them back.
    pub(super) fn rebuild_folder_list(&self, sender: &ComponentSender<Self>) {
        while let Some(row) = self.folder_list.first_child() {
            self.folder_list.remove(&row);
        }
        let multi = self.accounts.len() > 1;
        for acct in &self.accounts {
            if multi {
                let header = gtk::ListBoxRow::new();
                header.set_activatable(false);
                header.set_selectable(false);
                header.add_css_class("gallery-folder-heading");
                let label = gtk::Label::new(Some(&acct.label));
                label.set_xalign(0.0);
                label.add_css_class("heading");
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                header.set_child(Some(&label));
                self.folder_list.append(&header);
            }
            for folder in &acct.folders {
                let row = adw::ActionRow::new();
                row.set_title(&glib::markup_escape_text(&folder.name));
                row.add_prefix(&gtk::Image::from_icon_name(folder.kind.icon()));
                let check = gtk::CheckButton::new();
                check.set_active(self.folder_ticked(acct.id, &folder.path, folder.kind));
                check.set_valign(gtk::Align::Center);
                let s = sender.clone();
                let account_id = acct.id;
                let path = folder.path.clone();
                check.connect_toggled(move |c| {
                    s.input(GalleryInput::SetFolder {
                        account_id,
                        path: path.clone(),
                        on: c.is_active(),
                    });
                });
                row.add_suffix(&check);
                row.set_activatable_widget(Some(&check));
                // An inbox always counts; the rest answer to a master switch.
                row.set_sensitive(match kind_master(folder.kind) {
                    Some(Master::Archive) => self.include_archive,
                    Some(Master::Other) => self.include_other,
                    None => true,
                });
                self.folder_list.append(&row);
            }
        }
    }

    /// Refill the account dropdown's rows from `accounts`, keeping "All
    /// accounts" first.
    pub(super) fn rebuild_account_names(&self) {
        while self.account_names.n_items() > 1 {
            self.account_names.remove(self.account_names.n_items() - 1);
        }
        for acct in &self.accounts {
            self.account_names.append(&acct.label);
        }
    }

    /// Persist the folder scope, dropping ticks that agree with their kind's
    /// default so the file only records genuine choices.
    pub(super) fn save_scope(&self) {
        let kinds: HashMap<(u32, &str), FolderKind> = self
            .accounts
            .iter()
            .flat_map(|a| {
                a.folders
                    .iter()
                    .map(move |f| ((a.id, f.path.as_str()), f.kind))
            })
            .collect();
        let mut folders: Vec<(u32, String, bool)> = self
            .overrides
            .iter()
            .flat_map(|(id, m)| m.iter().map(move |(path, on)| (*id, path.clone(), *on)))
            .filter(|(id, path, on)| {
                // Keep a tick for a folder we have not been told about: it may
                // belong to an account that is offline right now.
                kinds
                    .get(&(*id, path.as_str()))
                    .is_none_or(|k| kind_default(*k) != *on)
            })
            .collect();
        folders.sort();
        crate::config::save_gallery_scope(self.include_archive, self.include_other, &folders);
    }

    /// Re-apply the scope to the loaded items and repaint both the grid/table
    /// and the folder list's tick marks.
    pub(super) fn scope_changed(&mut self, sender: &ComponentSender<Self>) {
        self.recompute_scope();
        self.save_scope();
        self.rebuild_folder_list(sender);
        self.reload(sender);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gallery's default catch: inboxes, archives and custom folders, but
    /// not Sent.
    #[test]
    fn sent_is_the_only_kind_off_by_default() {
        for kind in [
            FolderKind::Inbox,
            FolderKind::Starred,
            FolderKind::Archive,
            FolderKind::Custom,
        ] {
            assert!(
                in_scope(kind, true, true, None),
                "{kind:?} should be on by default"
            );
        }
        assert!(!in_scope(FolderKind::Sent, true, true, None));
    }

    #[test]
    fn a_tick_overrides_the_kind_default_either_way() {
        assert!(in_scope(FolderKind::Sent, true, true, Some(true)));
        assert!(!in_scope(FolderKind::Custom, true, true, Some(false)));
    }

    #[test]
    fn each_master_switch_governs_only_its_own_kinds() {
        // Archive off takes the archive down and leaves the rest alone.
        assert!(!in_scope(FolderKind::Archive, false, true, None));
        assert!(in_scope(FolderKind::Inbox, false, true, None));
        assert!(in_scope(FolderKind::Custom, false, true, None));
        // "Other" off takes everything that is neither inbox nor archive.
        assert!(!in_scope(FolderKind::Custom, true, false, None));
        assert!(!in_scope(FolderKind::Starred, true, false, None));
        assert!(in_scope(FolderKind::Inbox, true, false, None));
        assert!(in_scope(FolderKind::Archive, true, false, None));
    }

    /// An inbox answers to neither switch: there is always something to show.
    #[test]
    fn an_inbox_survives_both_switches_being_off() {
        assert!(in_scope(FolderKind::Inbox, false, false, None));
    }

    /// A master switch that is off beats a tick, but does not erase it: the
    /// tick is what comes back when the switch returns.
    #[test]
    fn a_master_switch_outranks_a_tick_without_clearing_it() {
        assert!(!in_scope(FolderKind::Archive, false, true, Some(true)));
        assert!(in_scope(FolderKind::Archive, true, true, Some(true)));
    }
}
