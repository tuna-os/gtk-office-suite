//! Native OAuth2 (XOAUTH2) sign-in for accounts added directly in Hylki.
//!
//! Runs the authorization-code flow with PKCE against the provider: opens the
//! system browser, captures the redirect on a loopback socket, and exchanges the
//! code for a refresh token. The refresh token is kept in the keyring; a fresh
//! access token is minted from it at connect time.
//!
//! Microsoft uses Hylki's built-in OAuth client; Google's client is bundled into
//! official builds at compile time (otherwise use GNOME Online Accounts, or your
//! own client in `~/.config/hylki/oauth.toml`). Advanced users can override any of
//! it or point "Custom OAuth" at another provider — see `provider_credentials`.

use serde::Deserialize;

use crate::config::OAuthSettings;

mod flow;
mod tokens;

pub(crate) use flow::{microsoft_prompt, open_uri, run_flow, run_flow_as, Authorization};
pub(crate) use tokens::{
    exchange_code, microsoft_whoami, refresh_access_token, refresh_access_token_as,
};

// Built-in OAuth client credentials.
//
// Microsoft is a public client (PKCE, no secret), and a client ID is just a
// public identifier, so it's embedded directly for one-click sign-in in every
// build.
//
// Google's client is deliberately kept OUT of the public source — GitHub push
// protection and Google's own secret scanning flag it, and Google may auto-revoke
// an exposed secret. It's read at COMPILE TIME from env vars, so a build *can*
// bundle a Google app by setting `HYLKI_GOOGLE_CLIENT_ID` / `HYLKI_GOOGLE_CLIENT_SECRET`
// — but the official builds ship empty, so Google sign-in goes through GNOME
// Online Accounts (or a client the user supplies). Runtime overrides —
// `~/.config/hylki/oauth.toml` or `HYLKI_*` env vars at runtime — still take
// precedence; see `provider_credentials`.
const GOOGLE_CLIENT_ID: &str = match option_env!("HYLKI_GOOGLE_CLIENT_ID") {
    Some(v) => v,
    None => "",
};
const GOOGLE_CLIENT_SECRET: &str = match option_env!("HYLKI_GOOGLE_CLIENT_SECRET") {
    Some(v) => v,
    None => "",
};
// Microsoft: Hylki's own app registration in Microsoft Entra (#329), a public
// client (PKCE, no secret) for personal and work accounts alike, verified
// against hylki.app. Mail runs over Graph, as it does for accounts imported
// from GNOME Online Accounts (#36), which some systems cannot add (GNOME 46's
// GOA fails on personal accounts) and some tenants refuse. A build can name
// another client via `HYLKI_MICROSOFT_CLIENT_ID`, and oauth.toml or the
// environment still override it at run time.
const MICROSOFT_CLIENT_ID: &str = match option_env!("HYLKI_MICROSOFT_CLIENT_ID") {
    Some(v) => v,
    None => "01cdc012-c8d8-4d03-822c-76696a01c14e",
};
const MICROSOFT_CLIENT_SECRET: &str = "";
// Dropbox (cloud attachments, #144): a public client with PKCE, so an app
// key alone is enough. A build can bundle one via `HYLKI_DROPBOX_CLIENT_ID`;
// otherwise the user makes an app in the Dropbox App Console and types its
// key into the account's settings.
const DROPBOX_CLIENT_ID: &str = match option_env!("HYLKI_DROPBOX_CLIENT_ID") {
    Some(v) => v,
    None => "",
};

/// Dropbox matches loopback redirect URIs exactly, port included, so the
/// listener for its sign-in is on this fixed port and the app's registered
/// redirect URI is `http://localhost:41597/`.
pub(crate) const DROPBOX_REDIRECT_PORT: u16 = 41597;

/// Server presets for a known provider (endpoints + IMAP/SMTP hosts). Client
/// credentials come from [`provider_credentials`], not here.
pub(crate) struct Preset {
    pub(crate) auth_url: &'static str,
    pub(crate) token_url: &'static str,
    pub(crate) scopes: &'static str,
    pub(crate) imap_host: &'static str,
    pub(crate) imap_port: u16,
    pub(crate) smtp_host: &'static str,
    pub(crate) smtp_port: u16,
}

/// Preset for a provider key ("google" / "microsoft"), or `None` for custom.
pub(crate) fn preset(provider: &str) -> Option<Preset> {
    match provider {
        "google" => Some(Preset {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            scopes: "https://mail.google.com/",
            imap_host: "imap.gmail.com",
            imap_port: 993,
            smtp_host: "smtp.gmail.com",
            smtp_port: 465,
        }),
        "microsoft" => Some(Preset {
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            // Graph, not IMAP: many work tenants turn IMAP off, and Graph is
            // what Microsoft accounts from GNOME Online Accounts use. The
            // mailbox settings carry the categories tags map to (#71).
            scopes: "https://graph.microsoft.com/Mail.ReadWrite \
                     https://graph.microsoft.com/Mail.Send \
                     https://graph.microsoft.com/MailboxSettings.ReadWrite \
                     https://graph.microsoft.com/User.Read offline_access",
            imap_host: "outlook.office365.com",
            imap_port: 993,
            smtp_host: "smtp.office365.com",
            smtp_port: 587,
        }),
        _ => None,
    }
}

/// What searching the organization's directory needs on top of the preset
/// (#410): the people the user works with, and everyone's name and address.
/// Asked for only when the account's directory switch is turned on, since a
/// tenant that lets users approve mail access may still want an
/// administrator for these.
const MICROSOFT_DIRECTORY_SCOPES: [&str; 2] = [
    "https://graph.microsoft.com/People.Read",
    "https://graph.microsoft.com/User.ReadBasic.All",
];

/// What reading the names of the organization's sensitivity labels needs
/// (#411), when a message's label leaves its name out. Asked for only when
/// the account's label switch is turned on, for the same reason as the
/// directory's.
pub(crate) const MICROSOFT_LABEL_SCOPES: [&str; 1] =
    ["https://graph.microsoft.com/InformationProtectionPolicy.Read"];

/// Every scope Hylki adds to its own sign-in on request.
const MICROSOFT_EXTRA_SCOPES: [&[&str]; 2] = [&MICROSOFT_DIRECTORY_SCOPES, &MICROSOFT_LABEL_SCOPES];

/// Whether `scope` is the scope `full`, written out or as Graph's short
/// name, the way the Advanced row takes them.
fn is_scope(scope: &str, full: &str) -> bool {
    scope.eq_ignore_ascii_case(full)
        || full
            .rsplit('/')
            .next()
            .is_some_and(|short| scope.eq_ignore_ascii_case(short))
}

/// Whether a sign-in with `scopes` was granted all of `set`.
pub(crate) fn has_scopes(scopes: &str, set: &[&str]) -> bool {
    set.iter()
        .all(|full| scopes.split_whitespace().any(|s| is_scope(s, full)))
}

/// `scopes` with those of `set` added where missing.
pub(crate) fn with_scopes(scopes: &str, set: &[&str]) -> String {
    let mut list: Vec<&str> = scopes.split_whitespace().collect();
    for full in set {
        if !list.iter().any(|s| is_scope(s, full)) {
            list.push(full);
        }
    }
    list.join(" ")
}

/// Whether a sign-in with `scopes` was granted the directory search.
pub(crate) fn has_directory_scopes(scopes: &str) -> bool {
    has_scopes(scopes, &MICROSOFT_DIRECTORY_SCOPES)
}

/// `scopes` with the directory's added where missing.
pub(crate) fn with_directory_scopes(scopes: &str) -> String {
    with_scopes(scopes, &MICROSOFT_DIRECTORY_SCOPES)
}

/// `scopes` without the ones Hylki adds as it adds them, so a sign-in that
/// has them still reads as Hylki's own and the Advanced row stays empty.
/// Short names typed into that row are the user's and stay.
fn without_extra_scopes(scopes: &str) -> String {
    scopes
        .split_whitespace()
        .filter(|s| {
            !MICROSOFT_EXTRA_SCOPES
                .iter()
                .flat_map(|set| set.iter())
                .any(|full| s.eq_ignore_ascii_case(full))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hylki's Microsoft sign-in made with another app registration (#329):
/// its client ID, its tenant (`common`, `organizations`, `consumers`, a
/// directory ID or domain), its scopes and its redirect. Empty values keep
/// what `settings` has. Scopes are Graph's short names, as Evolution lists
/// them; `offline_access` is added when missing, or no refresh token comes
/// back.
pub(crate) fn microsoft_advanced(
    settings: &mut OAuthSettings,
    client_id: &str,
    tenant: &str,
    scopes: &str,
    redirect: &str,
) {
    if !client_id.is_empty() {
        settings.client_id = client_id.to_string();
    }
    let tenant = tenant.trim().trim_matches('/');
    if !tenant.is_empty() && !tenant.contains(['/', '?', '#', ' ']) {
        settings.auth_url = settings
            .auth_url
            .replacen("/common/", &format!("/{tenant}/"), 1);
        settings.token_url = settings
            .token_url
            .replacen("/common/", &format!("/{tenant}/"), 1);
    }
    if !scopes.trim().is_empty() {
        let mut list: Vec<&str> = scopes.split_whitespace().collect();
        if !list
            .iter()
            .any(|s| s.eq_ignore_ascii_case("offline_access"))
        {
            list.push("offline_access");
        }
        settings.scopes = list.join(" ");
    }
    settings.redirect_uri = redirect.trim().to_string();
}

/// [`microsoft_advanced`] read back from a saved account's settings, for
/// the form: `(client_id, tenant, scopes, redirect)`, each empty where it
/// is Hylki's own.
pub(crate) fn microsoft_advanced_of(settings: &OAuthSettings) -> (String, String, String, String) {
    let Some(p) = preset("microsoft") else {
        return Default::default();
    };
    if !settings.token_url.contains("microsoftonline") {
        return Default::default();
    }
    let client = if settings.client_id == MICROSOFT_CLIENT_ID {
        String::new()
    } else {
        settings.client_id.clone()
    };
    let tenant = settings
        .auth_url
        .strip_prefix("https://login.microsoftonline.com/")
        .and_then(|rest| rest.split('/').next())
        .filter(|t| *t != "common")
        .unwrap_or_default()
        .to_string();
    let own = without_extra_scopes(&settings.scopes);
    let scopes = if own == without_extra_scopes(p.scopes) {
        String::new()
    } else {
        own
    };
    (client, tenant, scopes, settings.redirect_uri.clone())
}

/// Resolve a provider's OAuth client credentials `(client_id, client_secret)`,
/// preferring the user's own over the built-in fallback. Order:
///   1. Environment: `HYLKI_GOOGLE_CLIENT_ID` / `HYLKI_GOOGLE_CLIENT_SECRET`
///      (or `HYLKI_MICROSOFT_*`).
///   2. `~/.config/hylki/oauth.toml` — `[google]` / `[microsoft]` with
///      `client_id` and `client_secret`. Keeps secrets out of the source repo.
///   3. The built-in Thunderbird fallback.
///
/// A user override only takes effect once complete (both id and secret), so a
/// half-configured `oauth.toml` (client_id but no secret yet) keeps working on the
/// fallback instead of breaking sign-in.
pub(crate) fn provider_credentials(provider: &str) -> (String, String) {
    // `secret_required`: Google desktop clients always have a secret and its token
    // endpoint demands it, so the override only counts as complete with both. Azure
    // desktop apps are public clients (PKCE, no secret), so a client_id alone is a
    // valid override there.
    let (env_id, env_secret, default_id, default_secret, secret_required) = match provider {
        "google" => (
            "HYLKI_GOOGLE_CLIENT_ID",
            "HYLKI_GOOGLE_CLIENT_SECRET",
            GOOGLE_CLIENT_ID,
            GOOGLE_CLIENT_SECRET,
            true,
        ),
        "microsoft" => (
            "HYLKI_MICROSOFT_CLIENT_ID",
            "HYLKI_MICROSOFT_CLIENT_SECRET",
            MICROSOFT_CLIENT_ID,
            MICROSOFT_CLIENT_SECRET,
            false,
        ),
        "dropbox" => (
            "HYLKI_DROPBOX_CLIENT_ID",
            "HYLKI_DROPBOX_CLIENT_SECRET",
            DROPBOX_CLIENT_ID,
            "",
            false,
        ),
        _ => ("", "", "", "", true),
    };

    let usable = |id: String, secret: String| -> Option<(String, String)> {
        let ok = !id.trim().is_empty() && (!secret_required || !secret.trim().is_empty());
        ok.then_some((id, secret))
    };

    if let Some(c) = usable(
        std::env::var(env_id).unwrap_or_default(),
        std::env::var(env_secret).unwrap_or_default(),
    ) {
        return c;
    }
    if let Some((id, secret)) = creds_from_file(provider) {
        if let Some(c) = usable(id, secret) {
            return c;
        }
    }
    (default_id.to_string(), default_secret.to_string())
}

#[derive(Deserialize, Default)]
struct OAuthFile {
    #[serde(default)]
    google: Option<FileCreds>,
    #[serde(default)]
    microsoft: Option<FileCreds>,
    #[serde(default)]
    dropbox: Option<FileCreds>,
}

#[derive(Deserialize, Default)]
struct FileCreds {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: String,
}

fn creds_from_file(provider: &str) -> Option<(String, String)> {
    let file: OAuthFile = crate::config::read_toml(crate::config::config_file("oauth.toml"))?;
    let creds = match provider {
        "google" => file.google,
        "microsoft" => file.microsoft,
        "dropbox" => file.dropbox,
        _ => None,
    }?;
    Some((creds.client_id, creds.client_secret))
}

#[cfg(test)]
mod tests {

    use super::*;

    /// #329: Evolution's app registration, typed into the Advanced rows.
    #[test]
    fn microsoft_advanced_rows_make_the_sign_in() {
        let p = preset("microsoft").unwrap();
        let mut s = OAuthSettings {
            auth_url: p.auth_url.into(),
            token_url: p.token_url.into(),
            client_id: MICROSOFT_CLIENT_ID.into(),
            scopes: p.scopes.into(),
            ..Default::default()
        };
        assert_eq!(microsoft_advanced_of(&s), Default::default());
        microsoft_advanced(
            &mut s,
            "20460e5d-ce91-49af-a3a5-70b6be7486d1",
            "organizations",
            "Mail.ReadWrite Mail.Send User.Read",
            "https://login.microsoftonline.com/common/oauth2/nativeclient",
        );
        assert_eq!(s.client_id, "20460e5d-ce91-49af-a3a5-70b6be7486d1");
        assert_eq!(
            s.auth_url,
            "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize"
        );
        assert_eq!(
            s.token_url,
            "https://login.microsoftonline.com/organizations/oauth2/v2.0/token"
        );
        assert_eq!(
            s.scopes,
            "Mail.ReadWrite Mail.Send User.Read offline_access"
        );
        let back = microsoft_advanced_of(&s);
        assert_eq!(back.1, "organizations");
        assert_eq!(
            back.3,
            "https://login.microsoftonline.com/common/oauth2/nativeclient"
        );
        // The redirect is caught, with its code and the request's state.
        let req = Authorization::new(&s, &s.redirect_uri, "").unwrap();
        assert!(
            req.url
                .contains("redirect_uri=https%3A%2F%2Flogin.microsoftonline.com"),
            "{}",
            req.url
        );
        let ok = format!("{}?code=abc&state={}", s.redirect_uri, req.state);
        assert_eq!(req.answer(&ok, &s), Some(Ok("abc".into())));
        assert!(req
            .answer(
                "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?x",
                &s
            )
            .is_none());
        let refused = format!(
            "{}?error=access_denied&error_description=AADSTS65004%3A+declined",
            s.redirect_uri
        );
        assert!(matches!(req.answer(&refused, &s), Some(Err(_))));
    }

    /// #410: the directory's scopes ride along without reading as a
    /// registration of the user's own.
    #[test]
    fn directory_scopes_stay_out_of_the_advanced_row() {
        let p = preset("microsoft").unwrap();
        assert!(!has_directory_scopes(p.scopes));
        let with = with_directory_scopes(p.scopes);
        assert!(has_directory_scopes(&with));
        assert_eq!(with_directory_scopes(&with), with, "added once");
        assert!(with.ends_with("offline_access https://graph.microsoft.com/People.Read https://graph.microsoft.com/User.ReadBasic.All"));
        let s = OAuthSettings {
            auth_url: p.auth_url.into(),
            token_url: p.token_url.into(),
            client_id: MICROSOFT_CLIENT_ID.into(),
            scopes: with,
            ..Default::default()
        };
        assert_eq!(microsoft_advanced_of(&s), Default::default());
        // A registration's own short names count, and stay where typed.
        let own = OAuthSettings {
            scopes: "Mail.ReadWrite people.read User.ReadBasic.All offline_access".into(),
            ..s
        };
        assert!(has_directory_scopes(&own.scopes));
        assert_eq!(microsoft_advanced_of(&own).2, own.scopes);
        // The label scope (#411) rides along the same way.
        let both = with_scopes(&with_directory_scopes(p.scopes), &MICROSOFT_LABEL_SCOPES);
        assert!(has_scopes(&both, &MICROSOFT_LABEL_SCOPES) && has_directory_scopes(&both));
        assert!(!has_scopes(p.scopes, &MICROSOFT_LABEL_SCOPES));
        assert_eq!(
            microsoft_advanced_of(&OAuthSettings {
                scopes: both,
                ..own.clone()
            }),
            Default::default()
        );
        assert_eq!(microsoft_prompt(None), "&prompt=select_account");
        assert_eq!(microsoft_prompt(Some(" ")), "&prompt=select_account");
        assert_eq!(
            microsoft_prompt(Some("ann+x@contoso.com")),
            "&login_hint=ann%2Bx%40contoso.com"
        );
    }
}
