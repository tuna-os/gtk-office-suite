//! A composer's question to a Tuta account: which recipients are outside
//! Tuta, and the passwords its contacts keep for them. It goes straight to
//! the account's worker on a channel of its own, which the worker registers
//! here while it runs, rather than as a [`crate::worker::MailRequest`] that
//! every other kind of account would have to refuse (fork/MAINTAINING.md:
//! each such arm was a line in an upstream file).

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use tokio::sync::mpsc;

use crate::models::TutaRecipient;

/// What a composer asks, and where the answer goes: each address described,
/// in the order asked, or why the account could not say. An ask dropped
/// unanswered reads as "could not say".
#[derive(Debug)]
pub(crate) struct TutaRecipientsAsk {
    pub(crate) addresses: Vec<String>,
    pub(crate) answer: tokio::sync::oneshot::Sender<Result<Vec<TutaRecipient>, String>>,
}

impl TutaRecipientsAsk {
    /// Say that this account cannot answer, and why.
    pub(crate) fn refuse(self, why: impl Into<String>) {
        let _ = self.answer.send(Err(why.into()));
    }
}

/// The running Tuta workers' channels, by account id.
static WORKERS: Mutex<BTreeMap<u32, mpsc::UnboundedSender<TutaRecipientsAsk>>> =
    Mutex::new(BTreeMap::new());

/// Hand `ask` to the Tuta worker of `account_id`. With no such worker
/// running it goes unanswered, which the composer reads as "could not say";
/// the demo answers for itself (HYLKI_SHOWCASE_TUTA).
pub(crate) fn ask_recipients(account_id: u32, ask: TutaRecipientsAsk) {
    let worker = WORKERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&account_id)
        .cloned();
    match worker {
        Some(worker) => {
            let _ = worker.send(ask);
        }
        None if showcase() => demo_answer(ask),
        None => {}
    }
}

/// A worker's registration, and the questions it is asked; it unregisters
/// when dropped, unless a newer worker of the account has taken its place.
pub(super) struct Asks {
    account_id: u32,
    sender: mpsc::UnboundedSender<TutaRecipientsAsk>,
    pub(super) rx: mpsc::UnboundedReceiver<TutaRecipientsAsk>,
}

/// Register the worker of `account_id`, replacing any earlier one.
pub(super) fn register(account_id: u32) -> Asks {
    let (sender, rx) = mpsc::unbounded_channel();
    WORKERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(account_id, sender.clone());
    Asks {
        account_id,
        sender,
        rx,
    }
}

impl Asks {
    /// Questions no composer can reach, for a worker a test builds.
    #[cfg(test)]
    pub(super) fn detached() -> Asks {
        let (sender, rx) = mpsc::unbounded_channel();
        Asks {
            account_id: 0,
            sender,
            rx,
        }
    }
}

impl Drop for Asks {
    fn drop(&mut self) {
        let mut workers = WORKERS.lock().unwrap_or_else(PoisonError::into_inner);
        if workers
            .get(&self.account_id)
            .is_some_and(|s| s.same_channel(&self.sender))
        {
            workers.remove(&self.account_id);
        }
    }
}

/// Whether the demo made one of its accounts a Tuta account, so its
/// composer offers Confidential (see `app::demo`).
fn showcase() -> bool {
    std::env::var_os("HYLKI_DEMO").is_some() && std::env::var_os("HYLKI_SHOWCASE_TUTA").is_some()
}

/// The demo's answer: Tuta's own domains are on Tuta, anyone else is
/// outside, and the contacts keep a password for Priya alone, so the
/// composer shows a recipient with one and others without.
fn demo_answer(ask: TutaRecipientsAsk) {
    const TUTA_DOMAINS: [&str; 6] = [
        "tuta.com",
        "tuta.io",
        "tutanota.com",
        "tutanota.de",
        "tutamail.com",
        "keemail.me",
    ];
    let TutaRecipientsAsk { addresses, answer } = ask;
    let recipients = addresses
        .into_iter()
        .map(|address| {
            let domain = address
                .rsplit('@')
                .next()
                .unwrap_or_default()
                .to_lowercase();
            let external = !TUTA_DOMAINS.contains(&domain.as_str());
            let password = (external && address.to_lowercase().starts_with("priya"))
                .then(|| zeroize::Zeroizing::new("Lantern-Orbit-42!".to_string()));
            TutaRecipient {
                address,
                external: Some(external),
                password,
            }
        })
        .collect();
    let _ = answer.send(Ok(recipients));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(
        addresses: &[&str],
    ) -> (
        TutaRecipientsAsk,
        tokio::sync::oneshot::Receiver<Result<Vec<TutaRecipient>, String>>,
    ) {
        let (answer, answered) = tokio::sync::oneshot::channel();
        let addresses = addresses.iter().map(|a| a.to_string()).collect();
        (TutaRecipientsAsk { addresses, answer }, answered)
    }

    /// An ask reaches the worker registered for its account, and none once
    /// that worker is gone; a replaced worker's leaving does not unregister
    /// the one that took its place.
    #[test]
    fn an_ask_reaches_the_running_worker_of_its_account() {
        let id = 9_001;
        let mut first = register(id);
        let (q, mut answered) = ask(&["ann@example.com"]);
        ask_recipients(id, q);
        let got = first.rx.try_recv().expect("the worker is asked");
        assert_eq!(got.addresses, ["ann@example.com"]);
        got.refuse("no");
        assert!(matches!(answered.try_recv(), Ok(Err(why)) if why == "no"));

        let mut second = register(id);
        drop(first);
        let (q, _answered) = ask(&["bob@example.com"]);
        ask_recipients(id, q);
        assert!(
            second.rx.try_recv().is_ok(),
            "the newer worker stays registered"
        );

        drop(second);
        let (q, mut answered) = ask(&["cy@example.com"]);
        ask_recipients(id, q);
        assert!(
            answered.try_recv().is_err(),
            "with no worker the ask is let go unanswered"
        );
    }

    /// The demo says who is on Tuta, and knows Priya's password alone.
    #[test]
    fn the_demo_answers_who_is_outside_tuta() {
        let (q, mut answered) = ask(&["me@tuta.com", "priya@example.com", "sam@example.com"]);
        demo_answer(q);
        let found = answered.try_recv().unwrap().unwrap();
        let outside = found.iter().map(|r| r.external).collect::<Vec<_>>();
        assert_eq!(outside, [Some(false), Some(true), Some(true)]);
        assert!(found[1].password.is_some() && found[2].password.is_none());
    }
}
