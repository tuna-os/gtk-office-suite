//! Mailbox hosts: domains where anyone can have an address, so a sender
//! there is a person and keeps their initials rather than the provider's
//! mark.

use super::domain_of;

/// Mailbox hosts: domains that host other people's mail rather than send
/// their own. An address at one of these belongs to a person, not to the
/// provider, so it gets initials like any individual — otherwise every
/// friend on Gmail wears the Gmail mark (and, through the site-icon step,
/// so would every other freemail sender).
///
/// The provider's mark still identifies an *account* in Settings — that is
/// `brand.rs`, a different question from who sent a message.
const MAILBOX_HOSTS: &[&str] = &[
    // Google
    "gmail.com",
    "googlemail.com",
    // Microsoft
    "outlook.com",
    "outlook.co.uk",
    "hotmail.com",
    "hotmail.co.uk",
    "hotmail.fr",
    "hotmail.it",
    "hotmail.es",
    "hotmail.de",
    "live.com",
    "live.co.uk",
    "live.fr",
    "live.nl",
    "live.ca",
    "msn.com",
    // Apple
    "icloud.com",
    "me.com",
    "mac.com",
    "privaterelay.appleid.com",
    // Yahoo / AOL
    "yahoo.com",
    "yahoo.co.uk",
    "yahoo.co.jp",
    "yahoo.fr",
    "yahoo.de",
    "yahoo.es",
    "yahoo.it",
    "yahoo.ca",
    "yahoo.com.au",
    "yahoo.com.br",
    "yahoo.co.in",
    "ymail.com",
    "rocketmail.com",
    "aol.com",
    "aol.co.uk",
    // Privacy-minded providers
    "proton.me",
    "protonmail.com",
    "protonmail.ch",
    "pm.me",
    "tutanota.com",
    "tutanota.de",
    "tuta.com",
    "tuta.io",
    "mailbox.org",
    "posteo.de",
    "posteo.net",
    "hushmail.com",
    "runbox.com",
    "startmail.com",
    "disroot.org",
    "riseup.net",
    // Other mailbox hosts
    "fastmail.com",
    "fastmail.fm",
    "zoho.com",
    "zoho.eu",
    "hey.com",
    "duck.com",
    "mail.com",
    "email.com",
    "usa.com",
    "gmx.com",
    "gmx.net",
    "gmx.de",
    "gmx.at",
    "gmx.ch",
    "web.de",
    "t-online.de",
    "freenet.de",
    "arcor.de",
    // Russia / Ukraine
    "yandex.com",
    "yandex.ru",
    "yandex.by",
    "yandex.kz",
    "mail.ru",
    "bk.ru",
    "list.ru",
    "inbox.ru",
    "internet.ru",
    "rambler.ru",
    "ukr.net",
    "i.ua",
    // Asia
    "qq.com",
    "foxmail.com",
    "163.com",
    "126.com",
    "yeah.net",
    "sina.com",
    "sina.cn",
    "sohu.com",
    "naver.com",
    "daum.net",
    "hanmail.net",
    "rediffmail.com",
    // Europe / Americas ISPs and portals
    "orange.fr",
    "wanadoo.fr",
    "free.fr",
    "sfr.fr",
    "laposte.net",
    "gmx.fr",
    "libero.it",
    "virgilio.it",
    "alice.it",
    "tiscali.it",
    "seznam.cz",
    "centrum.cz",
    "wp.pl",
    "o2.pl",
    "onet.pl",
    "interia.pl",
    "btinternet.com",
    "sky.com",
    "talktalk.net",
    "ntlworld.com",
    "bigpond.com",
    "comcast.net",
    "verizon.net",
    "att.net",
    "sbcglobal.net",
    "bellsouth.net",
    "cox.net",
    "charter.net",
    "earthlink.net",
    "juno.com",
    "optonline.net",
    "shaw.ca",
    "rogers.com",
    "sympatico.ca",
    "telus.net",
    "uol.com.br",
    "bol.com.br",
    "terra.com.br",
    "ig.com.br",
];

/// Whether this address is at a [`MAILBOX_HOSTS`] domain, so it names a
/// person rather than a brand.
pub(super) fn is_mailbox_host(email: &str) -> bool {
    domain_of(email).is_some_and(|d| is_mailbox_domain(&d))
}

/// Whether `domain` (lowercase) is one of the [`MAILBOX_HOSTS`]: anyone can
/// have a mailbox there, so it vouches for no organisation.
pub(crate) fn is_mailbox_domain(domain: &str) -> bool {
    MAILBOX_HOSTS.contains(&domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::logo::bundled::has_bundled;
    use crate::services::logo::{known_missing, wants_refresh};

    /// A person on a freemail provider is a person, not the provider: no
    /// mark from any source, and nothing to fetch or refresh.
    #[test]
    fn mailbox_hosts_get_no_logo() {
        for addr in [
            "jane@gmail.com",
            "jane.doe@googlemail.com",
            "bob@hotmail.co.uk",
            "bob@outlook.com",
            "sam@yahoo.co.uk",
            "sam@icloud.com",
            "kim@proton.me",
            "lee@qq.com",
            "ann@mail.ru",
            "joe@comcast.net",
        ] {
            assert!(is_mailbox_host(addr), "{addr} should be a mailbox host");
            assert!(!has_bundled(addr), "{addr} should get no bundled mark");
            assert!(known_missing(addr), "{addr} should count as answered");
            assert!(!wants_refresh(addr), "{addr} should never be refreshed");
        }
    }

    /// The brands themselves keep their marks — it is the mailbox hosts,
    /// not the companies behind them, that are excluded.
    #[test]
    fn brands_behind_mailbox_hosts_keep_their_marks() {
        for addr in [
            "no-reply@google.com",
            "news@apple.com",
            "billing@microsoft.com",
        ] {
            assert!(!is_mailbox_host(addr), "{addr} is the brand, not a mailbox");
            assert!(has_bundled(addr), "{addr} should keep its bundled mark");
        }
        // A subdomain still resolves to the brand.
        assert!(!is_mailbox_host("alerts@mail.google.com"));
    }
}
