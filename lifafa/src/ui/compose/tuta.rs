//! Tuta's confidential mail in the composer, for a Tuta account only: the
//! Confidential toggle, which recipients are outside Tuta and the password
//! each of them needs, and what a send is asked before it goes.
//!
//! Confidential is end-to-end encrypted for everyone: a Tuta address
//! through its key, any other address behind a password, read in Tuta's
//! web app from a link Tuta mails to it. Off, a message that goes outside
//! Tuta goes as ordinary mail, which the user agrees to once per message.
//! Which recipients are outside Tuta, and the passwords the user's Tuta
//! contacts already keep for them, the account says (`contact_passwords`).

mod password;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use adw::prelude::*;
use crate::relm::prelude::*;

use super::messages::ComposeInput;
use super::{Compose, ComposeWidgets};
use crate::i18n::{i18n, ni18n_f};
use crate::models::{TutaDelivery, TutaPasswords, TutaRecipient};
use crate::ui::recipient_row::PillMark;
use crate::worker::TutaRecipientsAsk;
use password::{is_secure, strength, STRENGTH_MAX, STRENGTH_MEDIUM};

/// How long the recipient fields rest before the account is asked about
/// them, so typing one address is one question, not one per keystroke.
const LOOKUP_PAUSE: std::time::Duration = std::time::Duration::from_millis(500);

/// Where a recipient is, as the account said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reach {
    Tuta,
    Outside,
    /// The account could not say (offline, say). The helper decides again
    /// when the message goes.
    Unknown,
}

/// What a send has to do before it goes.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Check {
    Ready,
    /// The account is still to say where some recipients are.
    Wait,
    /// These outside recipients of a confidential message have no password.
    Missing(Vec<String>),
    /// These have one Tuta calls weak, which the user has not agreed to.
    Weak(Vec<String>),
    /// Not confidential and going outside Tuta, which the user has not
    /// agreed to.
    Consent,
}

/// The composer's Tuta state.
#[derive(Debug, Default)]
pub(super) struct TutaCompose {
    /// The From is a Tuta account; nothing here applies otherwise.
    pub(super) active: bool,
    /// That account, which is asked about the recipients.
    pub(super) account_id: u32,
    pub(super) confidential: bool,
    /// Confidential was set by hand (or by the message picked up again), so
    /// a change of From keeps it.
    pub(super) touched: bool,
    /// The state the composer itself last put the toggle in, until its
    /// `toggled` arrives: that one is not the user's.
    pub(super) expected: Option<bool>,
    /// The user agreed to send this message as ordinary mail.
    pub(super) ordinary_consent: bool,
    /// The weak passwords were agreed to at this [`Self::revision`].
    pub(super) weak_ok: Option<u64>,
    /// Bumped whenever a password changes.
    pub(super) revision: u64,
    /// Where each recipient is, by its address in lower case.
    pub(super) reach: HashMap<String, Reach>,
    /// The addresses asked about and not answered yet, in lower case.
    pub(super) asking: HashSet<String>,
    pub(super) passwords: TutaPasswords,
    /// Bumped when the From account changes, so an answer about the last
    /// one's recipients is dropped.
    pub(super) generation: u64,
    /// Bumped at every change to the recipients; only the last one's
    /// pause asks the account.
    pub(super) lookup_token: u64,
    /// A Send waits for the account's answer.
    pub(super) send_waiting: bool,
    /// The recipient fields report their changes (see
    /// [`Compose::wire_tuta_rows`]).
    pub(super) wired: bool,
}

fn key(address: &str) -> String {
    address.trim().to_lowercase()
}

/// Every address once, first spelling kept.
fn unique(addresses: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    addresses
        .iter()
        .filter(|a| !a.trim().is_empty() && seen.insert(key(a)))
        .cloned()
        .collect()
}

impl TutaCompose {
    /// Where `address` is, if the account has said.
    pub(super) fn reach_of(&self, address: &str) -> Option<Reach> {
        self.reach.get(&key(address)).copied()
    }

    /// The `addresses` outside Tuta, each once.
    pub(super) fn outside(&self, addresses: &[String]) -> Vec<String> {
        unique(addresses)
            .into_iter()
            .filter(|a| self.reach_of(a) == Some(Reach::Outside))
            .collect()
    }

    /// The `addresses` the account has not been asked about yet.
    pub(super) fn unasked(&self, addresses: &[String]) -> Vec<String> {
        unique(addresses)
            .into_iter()
            .filter(|a| self.reach_of(a).is_none() && !self.asking.contains(&key(a)))
            .collect()
    }

    /// Take in the account's answer about `asked`. A password the user
    /// typed stays: the contacts' only fills one not typed yet. An address
    /// the answer leaves out, or a failed answer, is [`Reach::Unknown`],
    /// never overwriting what an earlier answer said.
    pub(super) fn adopt(&mut self, asked: &[String], result: Result<Vec<TutaRecipient>, String>) {
        for a in asked {
            self.asking.remove(&key(a));
        }
        if let Ok(rows) = result {
            for row in rows {
                let reach = match row.external {
                    Some(true) => Reach::Outside,
                    Some(false) => Reach::Tuta,
                    None => Reach::Unknown,
                };
                self.reach.insert(key(&row.address), reach);
                if let Some(password) = row.password.filter(|_| reach != Reach::Tuta) {
                    if self.passwords.get(&row.address).is_none() {
                        self.passwords.set(&row.address, &password);
                        self.revision += 1;
                    }
                }
            }
        }
        for a in asked {
            self.reach.entry(key(a)).or_insert(Reach::Unknown);
        }
    }

    /// The strength of `address`'s password, scored against the words that
    /// would make it guessable: the sender's address and name, and the
    /// recipient's.
    pub(super) fn strength_of(&self, address: &str, name: &str, sender: &[&str]) -> u32 {
        let password = self.passwords.get(address).unwrap_or_default();
        let mut bad: Vec<&str> = sender.to_vec();
        bad.push(address);
        bad.push(name);
        strength(password, &bad)
    }

    /// What a send of a message to `recipients` (address, name) from
    /// `sender` (address, name) still needs.
    pub(super) fn check(&self, recipients: &[(String, String)], sender: &[&str]) -> Check {
        if !self.active {
            return Check::Ready;
        }
        let addresses: Vec<String> = recipients.iter().map(|(_, a)| a.clone()).collect();
        if !self.unasked(&addresses).is_empty()
            || addresses.iter().any(|a| self.asking.contains(&key(a)))
        {
            return Check::Wait;
        }
        let outside = self.outside(&addresses);
        if self.confidential {
            let missing: Vec<String> = outside
                .iter()
                .filter(|a| self.passwords.get(a).is_none())
                .cloned()
                .collect();
            if !missing.is_empty() {
                return Check::Missing(missing);
            }
            let name_of = |a: &str| {
                recipients
                    .iter()
                    .find(|(_, r)| r.eq_ignore_ascii_case(a))
                    .map(|(n, _)| n.clone())
                    .unwrap_or_default()
            };
            let weak: Vec<String> = outside
                .iter()
                .filter(|a| !is_secure(self.strength_of(a, &name_of(a), sender)))
                .cloned()
                .collect();
            if !weak.is_empty() && self.weak_ok != Some(self.revision) {
                return Check::Weak(weak);
            }
            return Check::Ready;
        }
        let unknown = addresses
            .iter()
            .any(|a| self.reach_of(a) == Some(Reach::Unknown));
        if (!outside.is_empty() || unknown) && !self.ordinary_consent {
            return Check::Consent;
        }
        Check::Ready
    }

    /// How the message goes. `with_passwords` names its recipients, whose
    /// passwords go with it: a draft or a template carries none.
    pub(super) fn delivery(&self, with_passwords: Option<&[String]>) -> TutaDelivery {
        if !self.active {
            return TutaDelivery::default();
        }
        let mut passwords = TutaPasswords::default();
        if self.confidential {
            for a in with_passwords.unwrap_or_default() {
                if self.reach_of(a) == Some(Reach::Tuta) {
                    continue;
                }
                if let Some(p) = self.passwords.get(a) {
                    passwords.set(a, p);
                }
            }
        }
        TutaDelivery {
            confidential: self.confidential,
            ordinary_confirmed: self.ordinary_consent,
            passwords,
        }
    }

    /// The mark on `address`'s pill: whether an outside recipient of a
    /// confidential message has a good password, a weak one or none, and
    /// that one of an ordinary message gets ordinary mail. A Tuta address
    /// and one the account could not place are left alone.
    pub(super) fn mark(&self, address: &str, name: &str, sender: &[&str]) -> Option<PillMark> {
        if !self.active || self.reach_of(address) != Some(Reach::Outside) {
            return None;
        }
        Some(if !self.confidential {
            PillMark {
                icon: "changes-allow-symbolic",
                words: i18n("Outside Tuta: gets ordinary email, not end-to-end encrypted"),
                class: "pill-mark-ordinary",
            }
        } else if self.passwords.get(address).is_none() {
            PillMark {
                icon: "dialog-password-symbolic",
                words: i18n("Outside Tuta: needs a password"),
                class: "pill-mark-missing",
            }
        } else if !is_secure(self.strength_of(address, name, sender)) {
            PillMark {
                icon: "dialog-warning-symbolic",
                words: i18n("Outside Tuta: the password is weak"),
                class: "pill-mark-weak",
            }
        } else {
            PillMark {
                icon: "channel-secure-symbolic",
                words: i18n("Outside Tuta: reads it in Tuta's web app with the password"),
                class: "pill-mark-ok",
            }
        })
    }

    /// The bar over the message: what happens to the outside recipients,
    /// and whether it offers the passwords. `None` while nobody is outside.
    pub(super) fn bar(&self, addresses: &[String]) -> Option<(&'static str, String, bool)> {
        if !self.active {
            return None;
        }
        let outside = self.outside(addresses);
        if outside.is_empty() {
            return None;
        }
        let count = outside.len() as u32;
        if !self.confidential {
            return Some((
                "changes-allow-symbolic",
                ni18n_f(
                    "Not confidential: {count} recipient outside Tuta gets ordinary email.",
                    "Not confidential: {count} recipients outside Tuta get ordinary email.",
                    count,
                    &[("count", &count.to_string())],
                ),
                false,
            ));
        }
        let missing = outside
            .iter()
            .filter(|a| self.passwords.get(a).is_none())
            .count() as u32;
        let text = if missing == 0 {
            ni18n_f(
                "{count} recipient outside Tuta reads this in Tuta's web app with the password you set.",
                "{count} recipients outside Tuta read this in Tuta's web app with the passwords you set.",
                count,
                &[("count", &count.to_string())],
            )
        } else {
            ni18n_f(
                "{missing} recipient outside Tuta needs a password to read this in Tuta's web app.",
                "{missing} recipients outside Tuta need a password to read this in Tuta's web app.",
                missing,
                &[("missing", &missing.to_string())],
            )
        };
        Some(("dialog-password-symbolic", text, true))
    }
}

/// The toggle's icon: a closed lock while confidential, as in Tuta.
pub(super) fn lock_icon(confidential: bool) -> &'static str {
    if confidential {
        "changes-prevent-symbolic"
    } else {
        "changes-allow-symbolic"
    }
}

pub(super) fn confidential_tooltip(confidential: bool) -> String {
    if confidential {
        i18n("Confidential: end-to-end encrypted. Recipients outside Tuta need a password.")
    } else {
        i18n("Not confidential: recipients outside Tuta get ordinary email.")
    }
}

/// The bar that says what happens to the recipients outside Tuta, with
/// the Passwords button.
#[derive(Debug, Clone)]
pub(super) struct TutaBar {
    pub(super) root: gtk::Box,
    icon: gtk::Image,
    label: gtk::Label,
    button: gtk::Button,
}

impl TutaBar {
    pub(super) fn new(sender: &ComponentSender<Compose>) -> TutaBar {
        add_style();
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("tuta-bar");
        root.set_margin_start(12);
        root.set_margin_end(12);
        root.set_margin_top(4);
        root.set_margin_bottom(4);
        root.set_visible(false);
        let icon = gtk::Image::new();
        let label = gtk::Label::new(None);
        label.set_hexpand(true);
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_wrap(true);
        let button = gtk::Button::with_label(&i18n("Passwords"));
        button.add_css_class("flat");
        button.set_valign(gtk::Align::Center);
        let s = sender.input_sender().clone();
        button.connect_clicked(move |_| s.emit(ComposeInput::TutaPasswords));
        root.append(&icon);
        root.append(&label);
        root.append(&button);
        TutaBar {
            root,
            icon,
            label,
            button,
        }
    }

    fn show(&self, bar: Option<(&'static str, String, bool)>) {
        let Some((icon, text, passwords)) = bar else {
            self.root.set_visible(false);
            return;
        };
        self.icon.set_icon_name(Some(icon));
        self.label.set_label(&text);
        self.button.set_visible(passwords);
        self.root.set_visible(true);
    }
}

/// The pills' marks and the strength bar's colors (`tuta/style.css`), added
/// once beside the app's stylesheet so that `styles.css` stays upstream's.
fn add_style() {
    thread_local!(static ADDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if ADDED.get() {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("tuta/style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    ADDED.set(true);
}

/// Every recipient of the fields, `(name, address)`, typed ones too.
fn recipients_of(widgets: &ComposeWidgets) -> Vec<(String, String)> {
    [&widgets.to_row, &widgets.cc_row, &widgets.bcc_row]
        .iter()
        .flat_map(|row| crate::worker::parse_recipients(&row.text()))
        .filter(|(_, a)| a.contains('@'))
        .collect()
}

fn addresses_of(recipients: &[(String, String)]) -> Vec<String> {
    recipients.iter().map(|(_, a)| a.clone()).collect()
}

impl Compose {
    /// The From's address and name, which a password must not be made of.
    fn sender_words(&self, widgets: &ComposeWidgets) -> Vec<String> {
        let idx = widgets.from_row.selected() as usize;
        self.accounts
            .get(idx)
            .map(|a| vec![a.email.clone(), crate::config::split_identity(&a.label).0])
            .unwrap_or_default()
    }

    /// Follow the From: Confidential applies to a Tuta account only, and
    /// starts as that account says unless it was set for this message. The
    /// recipients are asked about again, of the account now sending.
    pub(super) fn follow_tuta_account(
        &mut self,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        self.wire_tuta_rows(widgets, sender);
        let idx = widgets.from_row.selected() as usize;
        let Some(account) = self.accounts.get(idx) else {
            return;
        };
        let (active, id, default) = (account.tuta, account.id, account.confidential_default);
        let changed = id != self.tuta.account_id || active != self.tuta.active;
        self.tuta.active = active;
        if !changed {
            return;
        }
        self.tuta.account_id = id;
        self.tuta.generation += 1;
        self.tuta.reach.clear();
        self.tuta.asking.clear();
        if active {
            let on = if self.tuta.touched {
                self.tuta.confidential
            } else {
                default
            };
            self.set_confidential(on, widgets);
        }
        self.refresh_tuta(widgets);
        self.lookup_soon(sender);
    }

    /// Have the recipient fields report each change as
    /// [`ComposeInput::RecipientsChanged`], once, beside the composer's own
    /// handlers: the first [`Self::follow_tuta_account`] runs after the
    /// fields are filled, so the prefill is not reported.
    fn wire_tuta_rows(&mut self, widgets: &ComposeWidgets, sender: &ComponentSender<Self>) {
        if std::mem::replace(&mut self.tuta.wired, true) {
            return;
        }
        for row in [&widgets.to_row, &widgets.cc_row, &widgets.bcc_row] {
            let s = sender.input_sender().clone();
            row.connect_changed(move |_| s.emit(ComposeInput::RecipientsChanged));
        }
    }

    /// Put the toggle in `on` from the composer's side.
    pub(super) fn set_confidential(&mut self, on: bool, widgets: &ComposeWidgets) {
        self.tuta.confidential = on;
        if widgets.confidential_btn.is_active() != on {
            self.tuta.expected = Some(on);
            widgets.confidential_btn.set_active(on);
        }
    }

    /// Handles [`ComposeInput::ToggleConfidential`].
    pub(super) fn on_toggle_confidential(&mut self, on: bool, widgets: &ComposeWidgets) {
        if self.tuta.expected.take() != Some(on) {
            self.tuta.touched = true;
            self.fields_dirty = true;
        }
        self.tuta.confidential = on;
        self.refresh_tuta(widgets);
    }

    /// Handles [`ComposeInput::RecipientsChanged`]: the marks at once, the
    /// question once the typing rests.
    pub(super) fn on_recipients_changed(
        &mut self,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        if !self.tuta.active {
            return;
        }
        self.refresh_tuta(widgets);
        self.lookup_soon(sender);
    }

    fn lookup_soon(&mut self, sender: &ComponentSender<Self>) {
        if !self.tuta.active {
            return;
        }
        self.tuta.lookup_token += 1;
        let token = self.tuta.lookup_token;
        let s = sender.input_sender().clone();
        gtk::glib::timeout_add_local_once(LOOKUP_PAUSE, move || {
            s.emit(ComposeInput::TutaLookup(token));
        });
    }

    /// Handles [`ComposeInput::TutaLookup`]: ask, unless the fields changed
    /// again since.
    pub(super) fn on_tuta_lookup(
        &mut self,
        token: u64,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        if token == self.tuta.lookup_token {
            let addresses = [&widgets.to_row, &widgets.cc_row, &widgets.bcc_row]
                .iter()
                .flat_map(|row| row.pill_addresses())
                .collect::<Vec<_>>();
            self.ask_tuta(&addresses, sender);
        }
    }

    /// Ask the From account about whichever of `addresses` it has not been
    /// asked about. The answer comes back as [`ComposeInput::TutaKnown`].
    fn ask_tuta(&mut self, addresses: &[String], sender: &ComponentSender<Self>) {
        if !self.tuta.active {
            return;
        }
        // The helper refuses the whole question over one address that is
        // none, so such a one is not asked about: it is not known.
        let (unasked, invalid): (Vec<String>, Vec<String>) = self
            .tuta
            .unasked(addresses)
            .into_iter()
            .partition(|a| crate::worker::invalid_recipient(a).is_none());
        for a in invalid {
            self.tuta.reach.insert(key(&a), Reach::Unknown);
        }
        if unasked.is_empty() {
            return;
        }
        for a in &unasked {
            self.tuta.asking.insert(key(a));
        }
        let (answer, answered) = tokio::sync::oneshot::channel();
        let ask = TutaRecipientsAsk {
            addresses: unasked.clone(),
            answer,
        };
        // With no worker for the account, the question is let go
        // unanswered, which reads below as "could not say".
        crate::worker::ask_recipients(self.tuta.account_id, ask);
        let generation = self.tuta.generation;
        let s = sender.input_sender().clone();
        gtk::glib::spawn_future_local(async move {
            let result = answered
                .await
                .unwrap_or_else(|_| Err(i18n("The account did not answer.")));
            s.emit(ComposeInput::TutaKnown {
                generation,
                asked: unasked,
                result,
            });
        });
    }

    /// Handles [`ComposeInput::TutaKnown`], and sends again when a Send was
    /// waiting for it.
    pub(super) fn on_tuta_known(
        &mut self,
        generation: u64,
        asked: Vec<String>,
        result: Result<Vec<TutaRecipient>, String>,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        if generation != self.tuta.generation {
            return;
        }
        self.tuta.adopt(&asked, result);
        self.refresh_tuta(widgets);
        if self.tuta.send_waiting && self.tuta.asking.is_empty() {
            self.tuta.send_waiting = false;
            sender.input_sender().emit(ComposeInput::Send);
        }
    }

    /// The pill marks and the bar, as the state now stands.
    pub(super) fn refresh_tuta(&self, widgets: &ComposeWidgets) {
        let sender_words = self.sender_words(widgets);
        let sender: Vec<&str> = sender_words.iter().map(String::as_str).collect();
        let recipients = recipients_of(widgets);
        for row in [&widgets.to_row, &widgets.cc_row, &widgets.bcc_row] {
            row.mark_pills(|address| {
                let name = recipients
                    .iter()
                    .find(|(_, a)| a.eq_ignore_ascii_case(address))
                    .map(|(n, _)| n.as_str())
                    .unwrap_or_default();
                self.tuta.mark(address, name, &sender)
            });
        }
        self.tuta_bar
            .show(self.tuta.bar(&addresses_of(&recipients)));
    }

    /// Handles [`ComposeInput::TutaPassword`]: what was typed for
    /// `address`, held in `typed`; none takes its password away.
    pub(super) fn on_tuta_password(
        &mut self,
        address: &str,
        typed: &TutaPasswords,
        widgets: &ComposeWidgets,
    ) {
        self.tuta
            .passwords
            .set(address, typed.get(address).unwrap_or_default());
        self.tuta.revision += 1;
        self.fields_dirty = true;
        self.refresh_tuta(widgets);
    }

    /// Before a send: true while it has to wait, ask or stop. A message
    /// from any other account passes.
    pub(super) fn tuta_send_pending(
        &mut self,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) -> bool {
        if !self.tuta.active {
            return false;
        }
        let recipients = recipients_of(widgets);
        let sender_words = self.sender_words(widgets);
        let words: Vec<&str> = sender_words.iter().map(String::as_str).collect();
        let parent = widgets.to_row.root().and_downcast::<gtk::Window>();
        match self.tuta.check(&recipients, &words) {
            Check::Ready => false,
            Check::Wait => {
                self.ask_tuta(&addresses_of(&recipients), sender);
                if self.tuta.asking.is_empty() {
                    // Nothing left to ask: check again at once.
                    sender.input_sender().emit(ComposeInput::Send);
                } else {
                    self.tuta.send_waiting = true;
                }
                true
            }
            Check::Missing(missing) => {
                ask_for_passwords(parent.as_ref(), &missing, sender);
                true
            }
            Check::Weak(weak) => {
                ask_weak(parent.as_ref(), &weak, sender);
                true
            }
            Check::Consent => {
                ask_consent(parent.as_ref(), ComposeInput::Send, sender);
                true
            }
        }
    }

    /// Before a mail merge: a confidential message would need a password
    /// for every row's outside recipient, which the merge has no way to
    /// ask for; one that is not confidential asks for consent as Send does.
    /// True when the merge stops here.
    pub(super) fn tuta_merge_pending(
        &mut self,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) -> bool {
        if !self.tuta.active {
            return false;
        }
        let parent = widgets.to_row.root().and_downcast::<gtk::Window>();
        if self.tuta.confidential {
            crate::ui::dialog::alert(
                parent.as_ref(),
                &i18n("Cannot merge a confidential message"),
                &i18n("Each recipient outside Tuta would need a password of their own. Turn off Confidential to merge: mail to Tuta addresses is still end-to-end encrypted."),
            );
            return true;
        }
        if !self.tuta.ordinary_consent {
            ask_consent(parent.as_ref(), ComposeInput::MailMerge, sender);
            return true;
        }
        false
    }

    /// The delivery of the message about to be sent to `to`, `cc` and
    /// `bcc`, with their passwords.
    pub(super) fn tuta_delivery_for(&self, fields: [&str; 3]) -> TutaDelivery {
        let addresses: Vec<String> = fields
            .iter()
            .flat_map(|f| crate::worker::parse_recipients(f))
            .map(|(_, a)| a)
            .collect();
        self.tuta.delivery(Some(&addresses))
    }

    /// Handles [`ComposeInput::TutaPasswords`]: the passwords of the
    /// recipients outside Tuta, each with its strength, in a window over
    /// the composer.
    pub(super) fn on_tuta_passwords(
        &self,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        let recipients = recipients_of(widgets);
        let outside = self.tuta.outside(&addresses_of(&recipients));
        if outside.is_empty() {
            return;
        }
        let sender_words = self.sender_words(widgets);
        let parent = widgets.to_row.root().and_downcast::<gtk::Window>();
        let rows: Vec<(String, String, String)> = outside
            .iter()
            .map(|a| {
                let name = recipients
                    .iter()
                    .find(|(_, r)| r.eq_ignore_ascii_case(a))
                    .map(|(n, _)| n.clone())
                    .unwrap_or_default();
                let password = self.tuta.passwords.get(a).unwrap_or_default().to_string();
                (a.clone(), name, password)
            })
            .collect();
        password_window(parent.as_ref(), rows, sender_words, sender);
    }
}

/// A recipient's display in the passwords window: "Name <address>", or the
/// address alone.
fn shown(address: &str, name: &str) -> String {
    if name.trim().is_empty() {
        address.to_string()
    } else {
        format!("{} <{address}>", name.trim())
    }
}

/// Set the strength bar and its word for `score`.
fn show_strength(bar: &gtk::LevelBar, word: &gtk::Label, score: u32, empty: bool) {
    bar.set_value(f64::from(score));
    word.set_label(&if empty {
        i18n("Missing")
    } else if is_secure(score) {
        i18n("Strong")
    } else {
        i18n("Weak")
    });
    for class in ["error", "warning", "success"] {
        word.remove_css_class(class);
    }
    word.add_css_class(if empty || score < STRENGTH_MEDIUM {
        "error"
    } else if is_secure(score) {
        "success"
    } else {
        "warning"
    });
}

/// A strength bar coloured as Tuta colours its own: the error color below
/// 50, the warning color below 64, the success color from 64.
fn strength_bar() -> gtk::LevelBar {
    let bar = gtk::LevelBar::for_interval(0.0, f64::from(STRENGTH_MAX));
    for name in ["low", "high", "full"] {
        bar.remove_offset_value(Some(name));
    }
    bar.add_offset_value("password-weak", f64::from(STRENGTH_MEDIUM) - 0.5);
    bar.add_offset_value("password-fair", f64::from(password::STRENGTH_SECURE) - 0.5);
    bar.add_offset_value("password-strong", f64::from(STRENGTH_MAX));
    bar.set_width_request(72);
    bar.set_valign(gtk::Align::Center);
    bar
}

/// The window that holds the passwords. Each change goes back to the
/// composer as it is typed; the window keeps nothing of its own.
fn password_window(
    parent: Option<&gtk::Window>,
    rows: Vec<(String, String, String)>,
    sender_words: Vec<String>,
    sender: &ComponentSender<Compose>,
) {
    let window = adw::Window::builder()
        .modal(true)
        .title(i18n("Passwords"))
        .default_width(520)
        .build();
    window.set_transient_for(parent);
    window.add_css_class("tuta-passwords");
    window.add_controller(crate::ui::close_key());
    let header = adw::HeaderBar::new();
    let done = gtk::Button::with_label(&i18n("Done"));
    done.add_css_class("suggested-action");
    {
        let window = window.clone();
        done.connect_clicked(move |_| window.close());
    }
    header.pack_end(&done);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(12);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let intro = gtk::Label::new(Some(&i18n(
        "Recipients outside Tuta get an email with a link to Tuta's web app, where they enter the password to read this message and answer it securely.",
    )));
    let warning = gtk::Label::new(Some(&i18n(
        "Tell each of them the password another way, in person or on the phone, never by email. Tuta keeps it with the recipient in your Tuta contacts.",
    )));
    for label in [&intro, &warning] {
        label.set_wrap(true);
        label.set_xalign(0.0);
        content.append(label);
    }
    warning.add_css_class("dim-label");
    let group = adw::PreferencesGroup::new();
    let mut first: Option<adw::PasswordEntryRow> = None;
    for (address, name, password) in rows {
        let row = adw::PasswordEntryRow::new();
        row.set_title(&shown(&address, &name));
        row.set_text(&password);
        let bar = strength_bar();
        let word = gtk::Label::new(None);
        word.add_css_class("caption");
        word.set_width_chars(7);
        word.set_xalign(1.0);
        let words: Vec<String> = sender_words
            .iter()
            .cloned()
            .chain([address.clone(), name.clone()])
            .collect();
        let score = move |text: &str| {
            let bad: Vec<&str> = words.iter().map(String::as_str).collect();
            strength(text, &bad)
        };
        show_strength(&bar, &word, score(&password), password.is_empty());
        row.add_suffix(&bar);
        row.add_suffix(&word);
        let s = sender.input_sender().clone();
        let (bar_c, word_c) = (bar.clone(), word.clone());
        row.connect_changed(move |row| {
            let text = row.text();
            show_strength(&bar_c, &word_c, score(&text), text.is_empty());
            // Empty, it holds none, which takes the old one away.
            let mut typed = TutaPasswords::default();
            typed.set(&address, &text);
            s.emit(ComposeInput::TutaPassword {
                address: address.clone(),
                typed,
            });
        });
        if first.is_none() && password.is_empty() {
            first = Some(row.clone());
        }
        group.add(&row);
    }
    content.append(&group);
    let clamp = adw::Clamp::new();
    clamp.set_child(Some(&content));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&clamp));
    window.set_content(Some(&toolbar));
    window.present();
    if let Some(row) = first {
        row.grab_focus();
    }
}

/// The passwords are missing: say so, and offer to enter them.
#[allow(deprecated)] // adw::MessageDialog, as in crate::ui::dialog
fn ask_for_passwords(
    parent: Option<&gtk::Window>,
    missing: &[String],
    sender: &ComponentSender<Compose>,
) {
    let dialog = adw::MessageDialog::new(
        parent,
        Some(&i18n("Passwords Needed")),
        Some(&ni18n_f(
            "{names} is outside Tuta and needs a password to read this confidential message. Enter one, or turn off Confidential to send ordinary email.",
            "{names} are outside Tuta and each need a password to read this confidential message. Enter them, or turn off Confidential to send ordinary email.",
            missing.len() as u32,
            &[("names", &missing.join(", "))],
        )),
    );
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("passwords", &i18n("Enter Passwords"));
    dialog.set_default_response(Some("passwords"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("passwords", adw::ResponseAppearance::Suggested);
    let s = sender.input_sender().clone();
    dialog.connect_response(None, move |_, resp| {
        if resp == "passwords" {
            s.emit(ComposeInput::TutaPasswords);
        }
    });
    dialog.present();
}

/// A password is weak: Tuta's own question, Send Anyway or change it.
#[allow(deprecated)] // adw::MessageDialog, as in crate::ui::dialog
fn ask_weak(parent: Option<&gtk::Window>, weak: &[String], sender: &ComponentSender<Compose>) {
    let dialog = adw::MessageDialog::new(
        parent,
        Some(&i18n("Send With a Weak Password?")),
        Some(&ni18n_f(
            "The password for {names} is not secure enough. Anyone who guesses it can read the message.",
            "The passwords for {names} are not secure enough. Anyone who guesses one can read the message.",
            weak.len() as u32,
            &[("names", &weak.join(", "))],
        )),
    );
    dialog.add_response("change", &i18n("Change Password"));
    dialog.add_response("send", &i18n("Send Anyway"));
    dialog.set_default_response(Some("change"));
    dialog.set_close_response("change");
    dialog.set_response_appearance("send", adw::ResponseAppearance::Destructive);
    let s = sender.input_sender().clone();
    dialog.connect_response(None, move |_, resp| {
        s.emit(if resp == "send" {
            ComposeInput::ConfirmWeakPasswords
        } else {
            ComposeInput::TutaPasswords
        });
    });
    dialog.present();
}

/// Not confidential and going outside Tuta: asked once per message, in
/// Tuta's words, before `then` goes on.
#[allow(deprecated)] // adw::MessageDialog, as in crate::ui::dialog
fn ask_consent(
    parent: Option<&gtk::Window>,
    then: ComposeInput,
    sender: &ComponentSender<Compose>,
) {
    let dialog = adw::MessageDialog::new(
        parent,
        Some(&i18n("Send Without End-to-End Encryption?")),
        Some(&i18n(
            "This message will not be sent end-to-end encrypted. Recipients outside Tuta get it as ordinary email, which their mail provider can read. Turn on Confidential to protect it with a password instead.",
        )),
    );
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("send", &i18n("Send Not Confidential"));
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("send", adw::ResponseAppearance::Destructive);
    let s = sender.input_sender().clone();
    let then = RefCell::new(Some(then));
    dialog.connect_response(None, move |_, resp| {
        if resp != "send" {
            return;
        }
        s.emit(ComposeInput::ConsentOrdinary);
        if let Some(then) = then.borrow_mut().take() {
            s.emit(then);
        }
    });
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipient(address: &str, external: Option<bool>, password: Option<&str>) -> TutaRecipient {
        TutaRecipient {
            address: address.into(),
            external,
            password: password.map(|p| zeroize::Zeroizing::new(p.to_string())),
        }
    }

    fn state(confidential: bool) -> TutaCompose {
        TutaCompose {
            active: true,
            confidential,
            ..Default::default()
        }
    }

    fn rcpts(addresses: &[&str]) -> Vec<(String, String)> {
        addresses
            .iter()
            .map(|a| (String::new(), a.to_string()))
            .collect()
    }

    const STRONG: &str = "Lantern-Orbit-42!";

    #[test]
    fn a_send_waits_until_the_account_has_said() {
        let mut t = state(true);
        let to = rcpts(&["ann@example.com", "bo@tuta.com"]);
        assert_eq!(t.check(&to, &[]), Check::Wait);
        t.asking.insert("ann@example.com".into());
        t.asking.insert("bo@tuta.com".into());
        assert_eq!(t.unasked(&["ann@example.com".into()]), Vec::<String>::new());
        assert_eq!(t.check(&to, &[]), Check::Wait);
        t.adopt(
            &["ann@example.com".into(), "bo@tuta.com".into()],
            Ok(vec![
                recipient("ann@example.com", Some(true), None),
                recipient("bo@tuta.com", Some(false), None),
            ]),
        );
        assert!(t.asking.is_empty());
        assert_eq!(
            t.check(&to, &[]),
            Check::Missing(vec!["ann@example.com".into()])
        );
    }

    #[test]
    fn confidential_needs_a_password_and_asks_about_a_weak_one() {
        let mut t = state(true);
        let to = rcpts(&["Ann@Example.com"]);
        t.adopt(
            &["ann@example.com".into()],
            Ok(vec![recipient("ann@example.com", Some(true), None)]),
        );
        assert_eq!(
            t.check(&to, &[]),
            Check::Missing(vec!["Ann@Example.com".into()])
        );
        t.passwords.set("ann@example.com", "abc");
        t.revision += 1;
        assert_eq!(
            t.check(&to, &[]),
            Check::Weak(vec!["Ann@Example.com".into()])
        );
        // Agreed to, until the password changes again.
        t.weak_ok = Some(t.revision);
        assert_eq!(t.check(&to, &[]), Check::Ready);
        t.passwords.set("ann@example.com", "abcd");
        t.revision += 1;
        assert_eq!(
            t.check(&to, &[]),
            Check::Weak(vec!["Ann@Example.com".into()])
        );
        t.passwords.set("ann@example.com", STRONG);
        assert_eq!(t.check(&to, &[]), Check::Ready);
        // The recipient's own address is no password.
        t.passwords.set("ann@example.com", "ann@example.com");
        assert!(matches!(t.check(&to, &[]), Check::Weak(_)));
    }

    #[test]
    fn ordinary_mail_outside_tuta_needs_consent_once() {
        let mut t = state(false);
        let to = rcpts(&["ann@example.com", "bo@tuta.com"]);
        t.adopt(
            &["ann@example.com".into(), "bo@tuta.com".into()],
            Ok(vec![
                recipient("ann@example.com", Some(true), None),
                recipient("bo@tuta.com", Some(false), None),
            ]),
        );
        assert_eq!(t.check(&to, &[]), Check::Consent);
        t.ordinary_consent = true;
        assert_eq!(t.check(&to, &[]), Check::Ready);
        let d = t.delivery(Some(&["ann@example.com".into()]));
        assert!(!d.confidential && d.ordinary_confirmed && d.passwords.is_empty());
        // All on Tuta: confidential whatever the toggle, nothing to ask.
        let mut t = state(false);
        t.adopt(
            &["bo@tuta.com".into()],
            Ok(vec![recipient("bo@tuta.com", Some(false), None)]),
        );
        assert_eq!(t.check(&rcpts(&["bo@tuta.com"]), &[]), Check::Ready);
    }

    #[test]
    fn an_unknown_recipient_asks_consent_but_not_for_a_password() {
        let mut t = state(false);
        let to = rcpts(&["ann@example.com"]);
        t.adopt(&["ann@example.com".into()], Err("offline".into()));
        assert_eq!(t.reach_of("ann@example.com"), Some(Reach::Unknown));
        assert_eq!(t.check(&to, &[]), Check::Consent);
        // Confidential: the helper decides at the send.
        t.confidential = true;
        assert_eq!(t.check(&to, &[]), Check::Ready);
    }

    #[test]
    fn contacts_fill_a_password_but_never_over_a_typed_one() {
        let mut t = state(true);
        t.passwords.set("bo@example.com", "typed-by-me-1A!");
        t.adopt(
            &[
                "ann@example.com".into(),
                "bo@example.com".into(),
                "cy@tuta.com".into(),
            ],
            Ok(vec![
                recipient("ann@example.com", Some(true), Some("from-Contacts-9!")),
                recipient("bo@example.com", Some(true), Some("stored")),
                recipient("cy@tuta.com", Some(false), Some("never-used")),
            ]),
        );
        assert_eq!(t.passwords.get("ann@example.com"), Some("from-Contacts-9!"));
        assert_eq!(t.passwords.get("bo@example.com"), Some("typed-by-me-1A!"));
        assert_eq!(t.passwords.get("cy@tuta.com"), None);
        // A failed answer later does not forget what was said.
        t.adopt(&["ann@example.com".into()], Err("offline".into()));
        assert_eq!(t.reach_of("ann@example.com"), Some(Reach::Outside));
    }

    #[test]
    fn only_the_recipients_passwords_go_and_never_a_tuta_ones() {
        let mut t = state(true);
        t.adopt(
            &["ann@example.com".into(), "bo@tuta.com".into()],
            Ok(vec![
                recipient("ann@example.com", Some(true), None),
                recipient("bo@tuta.com", Some(false), None),
            ]),
        );
        t.passwords.set("ann@example.com", STRONG);
        t.passwords.set("gone@example.com", STRONG);
        t.passwords.set("bo@tuta.com", STRONG);
        let d = t.delivery(Some(&["ann@example.com".into(), "bo@tuta.com".into()]));
        assert!(d.confidential);
        assert_eq!(
            d.passwords.iter().map(|(a, _)| a).collect::<Vec<_>>(),
            ["ann@example.com"]
        );
        // A draft carries none.
        assert!(t.delivery(None).passwords.is_empty());
        // Nor does any other kind of account's message.
        t.active = false;
        assert_eq!(
            t.delivery(Some(&["ann@example.com".into()])),
            TutaDelivery::default()
        );
    }

    #[test]
    fn the_marks_and_the_bar_follow_the_state() {
        let mut t = state(true);
        let all: Vec<String> = vec!["ann@example.com".into(), "bo@tuta.com".into()];
        assert_eq!(t.bar(&all), None);
        t.adopt(
            &all,
            Ok(vec![
                recipient("ann@example.com", Some(true), None),
                recipient("bo@tuta.com", Some(false), None),
            ]),
        );
        assert_eq!(t.mark("bo@tuta.com", "", &[]), None);
        let class = |t: &TutaCompose| t.mark("ann@example.com", "Ann", &[]).map(|m| m.class);
        assert_eq!(class(&t), Some("pill-mark-missing"));
        let (_, _, passwords) = t.bar(&all).unwrap();
        assert!(passwords);
        t.passwords.set("ann@example.com", "short");
        assert_eq!(class(&t), Some("pill-mark-weak"));
        t.passwords.set("ann@example.com", STRONG);
        assert_eq!(class(&t), Some("pill-mark-ok"));
        t.confidential = false;
        assert_eq!(class(&t), Some("pill-mark-ordinary"));
        let (_, _, passwords) = t.bar(&all).unwrap();
        assert!(!passwords);
        t.active = false;
        assert_eq!(class(&t), None);
        assert_eq!(t.bar(&all), None);
    }
}
