//! Sending from a Tuta account. The helper does not take MIME: Tuta builds
//! the mail itself from its parts (sender, recipients, subject, HTML body
//! and attachments), so the message `build_email` produces is read back
//! into those parts (see [`crate::worker::helper::send`]). Tuta files the
//! Sent copy itself.
//!
//! Delivery is chosen as Tuta's own app chooses it. A message only to Tuta
//! addresses goes end-to-end encrypted ("confidential"), each recipient's
//! key loaded by the helper through Tuta's SDK. A message that also goes
//! outside Tuta goes as the composer chose: confidential, each outside
//! recipient reading it in Tuta's web app behind a password, or ordinary
//! mail (encrypted in transit only) to everyone, which the user agreed to.
//! A Tuta address the server has no key for is refused, never sent
//! unencrypted instead.
//!
//! The passwords never reach the Outbox: a queued confidential message
//! reads them again from the Tuta contacts the helper keeps them in, and
//! one that finds a recipient without goes back to the composer.
//!
//! A reply or a forward names the message it answers: the helper creates
//! the draft in that message's conversation, as Tuta's own app does, and
//! marks the original replied or forwarded once it went out.

use serde_json::Value;

use crate::cache::Cache;
use crate::config::AccountConfig;
use crate::i18n::{i18n, i18n_f};
use crate::models::{FolderKind, OutboxItem, TutaDelivery, TutaPasswords};
use crate::worker::compose::build::build_email;
use crate::worker::helper::send::{
    addresses_of, exact_forwards, message_parts, parse, send_outcome, send_parameters, sendable,
    thread_parameters, SendOutcome,
};
use crate::worker::helper::{HelperClient, HelperError, Object};
use crate::worker::outbox::{due_outbox_items, emit_outbox, uncertain_mark, uncertain_text};
use crate::worker::tuta::index::{run, tuta_load_folder, TutaState};
use crate::worker::tuta::ops::emit_folder;
use crate::worker::tuta::wire::{contact_passwords, password_map};
use crate::worker::{offline_text, OutgoingMessage, WorkerEvent};

/// The helper's `send` parameters for a built message, its envelope and
/// how it is to go: the shared parts and threading (see
/// [`send_parameters`]) and Tuta's delivery. `forwards` is
/// [`exact_forwards`] of the message's `forwards`.
pub(super) fn tuta_send_parameters(
    raw: &[u8],
    envelope: &[String],
    forwards: &str,
    delivery: &TutaDelivery,
) -> Result<Object, HelperError> {
    let mut parameters = send_parameters(raw, envelope, forwards)?;
    // The helper decides who is outside Tuta. Confidential with someone
    // outside needs each one's password, and is refused rather than sent
    // without; not confidential with someone outside needs the user's
    // consent, and is refused without it.
    parameters.insert("confidential".into(), delivery.confidential.into());
    parameters.insert(
        "ordinary_confirmed".into(),
        delivery.ordinary_confirmed.into(),
    );
    if delivery.confidential && !delivery.passwords.is_empty() {
        parameters.extend(password_map(&delivery.passwords));
    }
    Ok(parameters)
}

/// The helper's `save_draft` parameters for a built draft, which keeps its
/// Bcc in a header, as a draft is the user's own copy, and whether it is
/// to go confidential, so Tuta's own apps open it the same way. A draft
/// never carries a password.
///
/// `forwards` is [`exact_forwards`] of the message's `forwards`.
pub(super) fn draft_parameters(
    raw: &[u8],
    forwards: &str,
    confidential: bool,
) -> Result<Object, HelperError> {
    let message = parse(raw)?;
    let mut parameters = message_parts(&message, |_| addresses_of(message.bcc()))?;
    parameters.insert("confidential".into(), confidential.into());
    let recipients: Vec<String> = [message.to(), message.cc(), message.bcc()]
        .into_iter()
        .flat_map(addresses_of)
        .map(|(_, address)| address)
        .collect();
    parameters.extend(thread_parameters(&message, &recipients, forwards));
    Ok(parameters)
}

/// Send built bytes. Any reply with a result means the mail went. Its
/// fields are the server's ids, which nothing here needs; whether it went
/// confidentially, with each recipient's key version and fingerprint as
/// the helper loaded them (Tuta verifies no key unless the user has pinned
/// it, and Hylki pins none yet, so those only go to the debug log); and
/// whether the passwords reached the Tuta contacts.
pub(super) async fn tuta_send_raw(
    client: HelperClient,
    raw: Vec<u8>,
    envelope: Vec<String>,
    forwards: String,
    delivery: TutaDelivery,
) -> SendOutcome {
    let sent = run(move || {
        let parameters = tuta_send_parameters(&raw, &envelope, &forwards, &delivery)?;
        client.request("send", parameters)
    })
    .await;
    match sent {
        Ok(reply) => {
            let keys = reply
                .get("recipients")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let outside = reply
                .get("secure_external")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            match reply.get("confidential").and_then(Value::as_bool) {
                Some(true) => tracing::debug!(
                    "tuta: sent end-to-end encrypted to {keys} key(s) and {outside} password(s)"
                ),
                _ => tracing::debug!("tuta: sent as ordinary mail"),
            }
            // The mail went; only the next message to these recipients
            // misses the passwords, and asks for them again.
            if reply.get("contacts_saved").and_then(Value::as_bool) == Some(false) {
                tracing::warn!("tuta: sent, but the passwords were not saved to the contacts");
            }
            SendOutcome::Sent
        }
        Err(e) => {
            if e.code != "send_uncertain" {
                tracing::warn!("tuta: send failed: {}", e.code);
            }
            send_outcome(&e)
        }
    }
}

/// Send a composed message. `forwards` is [`exact_forwards`] of its
/// `forwards`.
pub(super) async fn tuta_send_message(
    account: &AccountConfig,
    state: &TutaState,
    message: &OutgoingMessage,
    forwards: String,
) -> SendOutcome {
    // Tuta rebuilds the mail from its parts, which would leave an OpenPGP
    // signature or encryption behind: refused, not sent without it.
    if message.sign || message.encrypt {
        return SendOutcome::Refused(i18n(
            "OpenPGP signing and encryption aren't available for Tuta accounts. Turn them off to send.",
        ));
    }
    let email = match build_email(account, message) {
        Ok(email) => email,
        Err(e) => return SendOutcome::Failed(e.to_string()),
    };
    if !crate::desktop::power::reaches_internet() {
        return SendOutcome::Failed(offline_text());
    }
    let client = match state.client(account) {
        Ok(client) => client,
        Err(e) => return SendOutcome::Failed(e.message()),
    };
    let envelope = email
        .envelope()
        .to()
        .iter()
        .map(|a| a.to_string())
        .collect();
    tuta_send_raw(
        client,
        email.formatted(),
        envelope,
        forwards,
        message.tuta.clone(),
    )
    .await
}

/// List the Sent folder, where Tuta filed the copy, so it joins the
/// conversation it answers (#199).
pub(super) async fn tuta_list_sent(
    account: &AccountConfig,
    account_id: u32,
    cache: Option<&Cache>,
    state: &mut TutaState,
    emit: &impl Fn(WorkerEvent),
) {
    let Some(path) = state.path_of(FolderKind::Sent) else {
        return;
    };
    let Some((folder_id, _)) = state.folders.get(&path).cloned() else {
        return;
    };
    if let Ok(messages) =
        tuta_load_folder(account, account_id, folder_id, &path, cache, state, emit).await
    {
        emit_folder(folder_id, messages, state, emit);
    }
}

/// How a queued message goes: as it was queued, and, when confidential,
/// with the passwords the user's Tuta contacts keep for its recipients,
/// read now since the Outbox keeps none. Which recipients are outside Tuta
/// is the helper's to say again at the send.
async fn queued_delivery(
    client: &HelperClient,
    item: &OutboxItem,
) -> Result<TutaDelivery, HelperError> {
    let mut delivery = item.delivery.clone();
    if !delivery.confidential {
        return Ok(delivery);
    }
    let lookup = client.clone();
    let rcpts = item.rcpts.clone();
    let recipients = run(move || contact_passwords(&lookup, &rcpts)).await?;
    let mut passwords = TutaPasswords::default();
    for r in recipients.iter().filter(|r| r.external != Some(false)) {
        if let Some(password) = &r.password {
            passwords.set(&r.address, password);
        }
    }
    delivery.passwords = passwords;
    Ok(delivery)
}

/// The Outbox for a Tuta account: each due message goes as it was queued.
/// One that may have gone out is marked and kept, never retried. One that
/// is confidential and finds an outside recipient without a password in
/// the contacts stays queued, and opens in the composer for it.
pub(super) async fn tuta_flush_outbox(
    cache: Option<&Cache>,
    account_id: u32,
    account: &AccountConfig,
    id: Option<u32>,
    state: &mut TutaState,
    emit: &impl Fn(WorkerEvent),
) {
    let Some(c) = cache else { return };
    let items = sendable(due_outbox_items(c, account_id, id), id, emit);
    if items.is_empty() {
        emit_outbox(cache, account_id, emit);
        return;
    }
    let client = match state.client(account) {
        Ok(client) => client,
        Err(e) => {
            emit(WorkerEvent::error(e.message()));
            return;
        }
    };
    emit(WorkerEvent::Status(i18n("Sending…")));
    let mut sent_any = false;
    for item in items {
        let forwards = exact_forwards(cache, &item.forwards);
        let outcome = match queued_delivery(&client, &item).await {
            Ok(delivery) => {
                tuta_send_raw(
                    client.clone(),
                    item.raw.clone(),
                    item.rcpts.clone(),
                    forwards,
                    delivery,
                )
                .await
            }
            Err(e) => {
                tracing::warn!(
                    "[account {account_id}] could not read the passwords of queued message {} from the Tuta contacts: {}",
                    item.id,
                    e.code
                );
                SendOutcome::Failed(e.message())
            }
        };
        match outcome {
            SendOutcome::Sent => {
                sent_any = true;
                c.delete_outbox(item.id);
            }
            SendOutcome::Uncertain => {
                let why = uncertain_text();
                c.record_outbox_failure(item.id, &uncertain_mark(&why));
                emit(WorkerEvent::error(i18n_f(
                    "“{subject}”: {e}",
                    &[("subject", &item.subject), ("e", &why)],
                )));
                break;
            }
            SendOutcome::PasswordRequired(e) => {
                c.record_outbox_failure(item.id, &e);
                emit(WorkerEvent::error(i18n_f(
                    "Could not send “{subject}”: {e}",
                    &[("subject", &item.subject), ("e", &e)],
                )));
                emit(WorkerEvent::EditQueued { id: item.id });
                break;
            }
            SendOutcome::Failed(e) | SendOutcome::Refused(e) => {
                c.record_outbox_failure(item.id, &e);
                emit(WorkerEvent::error(i18n_f(
                    "Still could not send “{subject}”: {e}",
                    &[("subject", &item.subject), ("e", &e)],
                )));
                break;
            }
        }
    }
    emit(WorkerEvent::Status(String::new()));
    if sent_any {
        tuta_list_sent(account, account_id, cache, state, emit).await;
        emit(WorkerEvent::Sent);
    }
    emit_outbox(cache, account_id, emit);
}
