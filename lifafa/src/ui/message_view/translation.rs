//! A card's translation (#327): the banner, the subject it shows, and
//! patching both in place.

use super::types::TrState;
use super::MessageView;
use crate::encoding::escape_html;
use crate::i18n::{i18n, i18n_f};
use crate::models::Message;

impl MessageView {
    /// A message's subject as shown: translated while its translation is
    /// (#327).
    pub(super) fn shown_subject(&self, m: &Message) -> String {
        match self.translations.get(&(m.account_id, m.id)) {
            Some(TrState::Shown(t)) => t.subject.clone().unwrap_or_else(|| m.subject.clone()),
            _ => self.real_subject(m),
        }
    }

    /// A message's subject, the one inside it once it is decrypted (#493):
    /// the outer one of an encrypted message can be a placeholder.
    pub(super) fn real_subject(&self, m: &Message) -> String {
        self.member_checks
            .get(&(m.account_id, m.id))
            .and_then(|c| c.pgp.as_ref())
            .and_then(|p| p.subject.clone())
            .unwrap_or_else(|| m.subject.clone())
    }

    /// What a card's translation banner holds right now (#327): where a
    /// translation stands, or, with the offer switched on, a Translate
    /// button on a message in another language. Nothing otherwise.
    pub(super) fn translate_inner_html(
        &self,
        m: &Message,
        settings: &crate::services::translate::Settings,
    ) -> String {
        let key = (m.account_id, m.id);
        let button = |label: &str| {
            format!(
                "<button type=\"button\" class=\"hylki-tr-btn\" data-key=\"{}:{}\">{}</button>",
                key.0,
                key.1,
                escape_html(label),
            )
        };
        let line = |text: &str, btn: String| {
            format!(
                "<span class=\"hylki-tr-text\">{}</span>{btn}",
                escape_html(text)
            )
        };
        let name = crate::services::translate::language_name;
        match self.translations.get(&key) {
            Some(TrState::Working) => line(&i18n("Translating…"), String::new()),
            Some(TrState::Failed(why)) => line(
                &i18n_f("Could not translate: {why}", &[("why", why)]),
                button(&i18n("Try Again")),
            ),
            Some(TrState::Shown(t)) => {
                let text = match &t.from {
                    Some(from) => i18n_f(
                        "Translated from {language} by {service}.",
                        &[("language", &name(from)), ("service", &t.service.name())],
                    ),
                    None => i18n_f(
                        "Translated by {service}.",
                        &[("service", &t.service.name())],
                    ),
                };
                line(&text, button(&i18n("Show Original")))
            }
            Some(TrState::Original(_)) => line(
                &i18n("Showing the original."),
                button(&i18n("Show Translation")),
            ),
            None => {
                if !(settings.enabled() && settings.offer)
                    || self.encrypted(key)
                    || m.body.trim().is_empty()
                {
                    return String::new();
                }
                match self.detected_language(m) {
                    Some(lang)
                        if crate::services::translate::differs(
                            &lang,
                            &settings.target_language(),
                        ) =>
                    {
                        line(
                            &i18n_f(
                                "This message is in {language}.",
                                &[("language", &name(&lang))],
                            ),
                            button(&i18n("Translate")),
                        )
                    }
                    _ => String::new(),
                }
            }
        }
    }

    /// Which language a card's message is in, remembered per body.
    fn detected_language(&self, m: &Message) -> Option<String> {
        let key = (m.account_id, m.id);
        if let Some((len, lang)) = self.detected.borrow().get(&key) {
            if *len == m.body.len() {
                return lang.clone();
            }
        }
        let lang = crate::services::translate::detect(&crate::mail::reader::extract(&m.body));
        self.detected
            .borrow_mut()
            .insert(key, (m.body.len(), lang.clone()));
        lang
    }

    /// Whether a card's message came encrypted: its text is never sent to
    /// a translation service.
    pub(super) fn encrypted(&self, key: (u32, u32)) -> bool {
        self.member_checks
            .get(&key)
            .and_then(|c| c.pgp.as_ref())
            .is_some_and(|p| p.encrypted)
            || self
                .thread
                .iter()
                .find(|m| (m.account_id, m.id) == key)
                .is_some_and(|m| crate::models::preview_is_encrypted(&m.preview))
    }

    /// Redraw one card's translation banner in the live document.
    pub(super) fn patch_translate(&self, account_id: u32, id: u32) {
        if !self.webview_ready || self.current.is_none() {
            return;
        }
        let Some(m) = self
            .thread
            .iter()
            .find(|m| m.account_id == account_id && m.id == id)
        else {
            return;
        };
        let html = self.translate_inner_html(m, &crate::services::translate::load());
        let js = format!(
            "(function(){{var d=document.querySelector('.hylki-tr[data-key=\"{account_id}:{id}\"]');\
             if(d)d.innerHTML={};}})()",
            serde_json::to_string(&html).unwrap_or_else(|_| "''".into()),
        );
        self.run_js(&js);
    }
}
