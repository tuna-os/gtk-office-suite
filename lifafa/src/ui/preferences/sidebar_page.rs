//! Settings → Sidebar: the unified section, the placed sections, the icon
//! rail's fold-ups and Focus Mode.

use super::choices::{section_style_at, start_view_at};
use super::*;

impl Preferences {
    pub(super) fn on_toggle_show_unified(&mut self, on: bool, sender: ComponentSender<Self>) {
        self.show_unified = on;
        let _ = sender.output(PrefOutput::SetShowUnified(on));
    }

    pub(super) fn on_toggle_show_accounts(&mut self, on: bool, sender: ComponentSender<Self>) {
        if self.show_accounts != on {
            self.show_accounts = on;
            let _ = sender.output(PrefOutput::SetShowAccounts(on));
        }
    }

    pub(super) fn on_toggle_focus(
        &mut self,
        part: crate::config::FocusPart,
        on: bool,
        sender: ComponentSender<Self>,
    ) {
        if self.focus.get(part) != on {
            self.focus.set(part, on);
            let _ = sender.output(PrefOutput::SetFocusMode(self.focus));
        }
    }

    pub(super) fn on_change_section_style(&mut self, idx: u32, sender: ComponentSender<Self>) {
        let style = section_style_at(idx);
        let _ = sender.output(PrefOutput::SetSectionStyle(style));
    }

    pub(super) fn on_change_contacts_placement(&mut self, idx: u32, sender: ComponentSender<Self>) {
        let placement = contacts_placement_from_index(idx);
        if let Some(c) = &self.contacts {
            c.emit(crate::ui::contact_sources::ContactSourcesInput::SetPlacement(placement));
        }
        let _ = sender.output(PrefOutput::SetContactsPlacement(placement));
    }

    pub(super) fn on_change_start_view(&mut self, idx: u32, sender: ComponentSender<Self>) {
        let view = start_view_at(idx);
        let _ = sender.output(PrefOutput::SetStartView(view));
    }

    pub(super) fn on_change_folder_sort(&mut self, idx: u32, sender: ComponentSender<Self>) {
        let sort = crate::config::FolderSort::ALL
            .get(idx as usize)
            .copied()
            .unwrap_or_default();
        let _ = sender.output(PrefOutput::SetFolderSort(sort));
    }
}

impl Preferences {
    /// Flip one of the unified rows' unread-chip switches and hand the app the
    /// whole set.
    pub(super) fn set_unified_chip(
        &mut self,
        switch: fn(&mut crate::config::UnifiedChips) -> &mut bool,
        on: bool,
        sender: ComponentSender<Self>,
    ) {
        *switch(&mut self.unified_chips) = on;
        let _ = sender.output(PrefOutput::SetUnifiedChips(self.unified_chips));
    }
}

impl Preferences {
    /// Flip one of the unified Starred / Sent / Drafts / Archive / Junk / Trash
    /// switches and hand the app the whole set.
    pub(super) fn set_unified_kind(
        &mut self,
        switch: fn(&mut crate::config::UnifiedKinds) -> &mut bool,
        on: bool,
        sender: ComponentSender<Self>,
    ) {
        *switch(&mut self.unified_kinds) = on;
        let _ = sender.output(PrefOutput::SetUnifiedKinds(self.unified_kinds));
    }
}

impl Preferences {
    /// Flip one of the icon rail's fold-up switches and hand the app the whole
    /// set.
    pub(super) fn set_rail_fold(
        &mut self,
        switch: fn(&mut crate::config::RailFold) -> &mut bool,
        on: bool,
        sender: ComponentSender<Self>,
    ) {
        *switch(&mut self.rail_fold) = on;
        let _ = sender.output(PrefOutput::SetRailFold(self.rail_fold));
    }
}
