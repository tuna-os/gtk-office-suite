//! The notification cards: pushing, dismissing and collapsing them, and the
//! connectivity card.

use super::*;

impl NotificationCenter {
    pub(super) fn on_push(
        &mut self,
        text: String,
        error: bool,
        connectivity: bool,
        sender: &ComponentSender<Self>,
    ) {
        self.transient_text = text.clone();
        self.transient_error = error;
        self.transient_visible = true;
        self.epoch = self.epoch.wrapping_add(1);

        if error {
            // Keep at most one connectivity error card around.
            if connectivity {
                self.remove_connectivity_cards();
            }
            let id = self.next_id;
            self.next_id += 1;
            self.ids.insert(0, id);
            self.cards.guard().push_front(Notification {
                id,
                text,
                connectivity,
            });
            let _ = sender.output(NotifyOutput::CountChanged(self.ids.len()));
        }

        // Auto-collapse the transient bar after a few seconds, unless a
        // newer message (or the expanded panel) supersedes it.
        let epoch = self.epoch;
        let sender = sender.clone();
        glib::timeout_add_local_once(Duration::from_secs(TRANSIENT_SECS), move || {
            sender.input(NotifyInput::CollapseTransient(epoch));
        });
    }

    pub(super) fn on_clear_connectivity(&mut self, sender: &ComponentSender<Self>) {
        let before = self.ids.len();
        self.remove_connectivity_cards();
        if self.ids.len() != before {
            if self.ids.is_empty() {
                self.panel_open = false;
            }
            let _ = sender.output(NotifyOutput::CountChanged(self.ids.len()));
        }
    }

    pub(super) fn on_collapse_transient(&mut self, epoch: u32) {
        if epoch == self.epoch {
            self.transient_visible = false;
        }
    }

    pub(super) fn on_dismiss(&mut self, id: u32, sender: &ComponentSender<Self>) {
        if let Some(pos) = self.ids.iter().position(|x| *x == id) {
            self.ids.remove(pos);
            self.cards.guard().remove(pos);
            if self.ids.is_empty() {
                self.panel_open = false;
            }
            let _ = sender.output(NotifyOutput::CountChanged(self.ids.len()));
        }
    }

    pub(super) fn on_clear_all(&mut self, sender: &ComponentSender<Self>) {
        self.cards.guard().clear();
        self.ids.clear();
        self.panel_open = false;
        self.transient_visible = false;
        let _ = sender.output(NotifyOutput::CountChanged(0));
    }
}
