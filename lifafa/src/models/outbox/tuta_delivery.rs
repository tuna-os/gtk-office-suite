//! How a message from a Tuta account goes out: end-to-end encrypted
//! ("confidential"), with a password for each recipient outside Tuta, or
//! as ordinary mail the user agreed to.

use zeroize::Zeroizing;

/// The header Hylki writes into the MIME it makes of a Tuta mail (Tuta
/// keeps none), saying whether it is confidential: `yes` or `no`. A draft
/// opens with Confidential as it was saved.
pub(crate) const TUTA_CONFIDENTIAL_HEADER: &str = "X-Tuta-Confidential";

/// The delivery a Tuta send asks the helper for.
///
/// The default is what Hylki asked for before the composer had a choice,
/// and what a message nobody composed (a read receipt, a filter's forward)
/// still asks for: confidential when every recipient is on Tuta, ordinary
/// mail to everyone otherwise, as Tuta's own app sends when it is not told
/// to keep it confidential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TutaDelivery {
    /// End-to-end encrypted for every recipient: Tuta addresses through
    /// their keys, any other address behind a password, read in Tuta's web
    /// app. Off, mail to an outside address goes as ordinary mail.
    pub(crate) confidential: bool,
    /// The user agreed to ordinary delivery. Only asked for, and only read
    /// by the helper, when the message is not confidential and goes outside
    /// Tuta.
    pub(crate) ordinary_confirmed: bool,
    /// The password of each outside recipient of a confidential message.
    /// Kept in memory only, never in the Outbox, a draft or the cache: a
    /// queued message reads them again from the Tuta contacts the helper
    /// stores them in.
    pub(crate) passwords: TutaPasswords,
}

impl Default for TutaDelivery {
    fn default() -> Self {
        TutaDelivery {
            confidential: false,
            ordinary_confirmed: true,
            passwords: TutaPasswords::default(),
        }
    }
}

/// [`TutaDelivery`] as an Outbox row keeps it, passwords left out. Empty is
/// a row queued before Hylki asked, or from another kind of account, which
/// reads as the default.
impl TutaDelivery {
    pub(crate) fn stored(&self) -> &'static str {
        match (self.confidential, self.ordinary_confirmed) {
            (true, _) => "confidential",
            (false, true) => "ordinary",
            (false, false) => "unconfirmed",
        }
    }

    pub(crate) fn from_stored(stored: &str) -> TutaDelivery {
        match stored {
            "confidential" => TutaDelivery {
                confidential: true,
                ordinary_confirmed: false,
                passwords: TutaPasswords::default(),
            },
            "unconfirmed" => TutaDelivery {
                confidential: false,
                ordinary_confirmed: false,
                passwords: TutaPasswords::default(),
            },
            _ => TutaDelivery::default(),
        }
    }
}

/// Passwords by recipient address, wiped from memory when dropped and
/// never printed: a `{:?}` of a message shows how many there are.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct TutaPasswords(Vec<(String, Zeroizing<String>)>);

impl TutaPasswords {
    /// Set `address`'s password, matching the address in any case. Spaces
    /// at either end are dropped, as Tuta drops them before it uses or
    /// stores one, and what is left empty is no password.
    pub(crate) fn set(&mut self, address: &str, password: &str) {
        self.0.retain(|(a, _)| !a.eq_ignore_ascii_case(address));
        let password = password.trim();
        if !password.is_empty() {
            self.0
                .push((address.to_string(), Zeroizing::new(password.to_string())));
        }
    }

    /// `address`'s password, in any case.
    pub(crate) fn get(&self, address: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(address))
            .map(|(_, p)| p.as_str())
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(a, p)| (a.as_str(), p.as_str()))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for TutaPasswords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TutaPasswords({} hidden)", self.0.len())
    }
}

/// One recipient as the helper's `contact_passwords` describes it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TutaRecipient {
    pub(crate) address: String,
    /// Outside Tuta, so a confidential message reaches it behind a
    /// password. `None`: the server could not say.
    pub(crate) external: Option<bool>,
    /// The password the user's Tuta contacts keep for it, if any.
    pub(crate) password: Option<Zeroizing<String>>,
}

impl std::fmt::Debug for TutaRecipient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TutaRecipient")
            .field("address", &self.address)
            .field("external", &self.external)
            .field("password", &self.password.as_ref().map(|_| "hidden"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outbox_keeps_the_choice_and_never_a_password() {
        let mut passwords = TutaPasswords::default();
        passwords.set("ann@example.com", "s3cret-Pass!");
        for delivery in [
            TutaDelivery {
                confidential: true,
                ordinary_confirmed: false,
                passwords: passwords.clone(),
            },
            TutaDelivery {
                confidential: false,
                ordinary_confirmed: true,
                passwords: TutaPasswords::default(),
            },
            TutaDelivery {
                confidential: false,
                ordinary_confirmed: false,
                passwords: TutaPasswords::default(),
            },
        ] {
            let back = TutaDelivery::from_stored(delivery.stored());
            assert_eq!(back.confidential, delivery.confidential);
            assert_eq!(back.ordinary_confirmed, delivery.ordinary_confirmed);
            assert!(back.passwords.is_empty());
        }
        // A row from before, or from another kind of account.
        assert_eq!(TutaDelivery::from_stored(""), TutaDelivery::default());
    }

    #[test]
    fn passwords_match_any_case_and_never_print() {
        let mut passwords = TutaPasswords::default();
        passwords.set("Ann@Example.com", "first");
        passwords.set("ann@example.com", "second");
        assert_eq!(passwords.get("ANN@example.com"), Some("second"));
        assert_eq!(passwords.iter().count(), 1);
        passwords.set("ann@example.com", "");
        assert!(passwords.is_empty());
        passwords.set("bob@example.com", "hunter2");
        let shown = format!("{passwords:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        let recipient = TutaRecipient {
            address: "bob@example.com".into(),
            external: Some(true),
            password: Some(Zeroizing::new("hunter2".into())),
        };
        assert!(!format!("{recipient:?}").contains("hunter2"));
    }
}
