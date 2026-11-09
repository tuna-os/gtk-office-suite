//! The allowed senders and the blocklist.

use super::*;

impl AccountsWindow {
    pub(super) fn on_search_senders(&mut self, text: String) {
        *self.senders_query.borrow_mut() = text.to_lowercase();
        self.senders.widget().invalidate_filter();
        self.blacklist.widget().invalidate_filter();
    }

    pub(super) fn on_add_sender_text(&mut self, text: String, sender: ComponentSender<Self>) {
        let addr = text.trim().to_lowercase();
        if !addr.is_empty() && !self.sender_addrs.contains(&addr) {
            self.sender_addrs.push(addr.clone());
            self.senders.guard().push_back(addr.clone());
            self.senders.widget().set_visible(true);
            let _ = sender.output(AccountsOutput::AddSender(addr));
        }
    }

    pub(super) fn on_remove_sender_row(&mut self, addr: String, sender: ComponentSender<Self>) {
        if let Some(pos) = self.sender_addrs.iter().position(|a| *a == addr) {
            self.sender_addrs.remove(pos);
            self.senders.guard().remove(pos);
            self.senders
                .widget()
                .set_visible(!self.sender_addrs.is_empty());
            let _ = sender.output(AccountsOutput::RemoveSender(addr));
        }
    }

    pub(super) fn on_add_blacklist_text(&mut self, text: String, sender: ComponentSender<Self>) {
        let addr = text.trim().to_lowercase();
        if !addr.is_empty() && !self.blacklist_addrs.contains(&addr) {
            self.blacklist_addrs.push(addr.clone());
            self.blacklist.guard().push_back(addr.clone());
            self.blacklist.widget().set_visible(true);
            let _ = sender.output(AccountsOutput::AddBlacklist(addr));
        }
    }

    pub(super) fn on_remove_blacklist_row(&mut self, addr: String, sender: ComponentSender<Self>) {
        if let Some(pos) = self.blacklist_addrs.iter().position(|a| *a == addr) {
            self.blacklist_addrs.remove(pos);
            self.blacklist.guard().remove(pos);
            self.blacklist
                .widget()
                .set_visible(!self.blacklist_addrs.is_empty());
            let _ = sender.output(AccountsOutput::RemoveBlacklist(addr));
        }
    }
}
