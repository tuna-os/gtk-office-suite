//! A cloud storage account as `cloud.toml` stores it, and the services the
//! Service picker offers.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CloudKind {
    /// Nextcloud, ownCloud or OpenCloud: WebDAV + OCS.
    #[default]
    Nextcloud,
    Dropbox,
    Seafile,
    #[serde(rename = "onedrive")]
    OneDrive,
}

impl CloudKind {
    /// Signed in through GNOME Online Accounts: no secret of ours.
    pub(crate) fn via_goa(self) -> bool {
        self == CloudKind::OneDrive
    }

    /// GOA's `ProviderType` for the kinds that go through it.
    pub(crate) fn goa_provider(self) -> Option<&'static str> {
        match self {
            CloudKind::OneDrive => Some("ms_graph"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CloudAccount {
    /// How the account is shown ("Work Nextcloud").
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) kind: CloudKind,
    /// Which product a `Nextcloud`-kind server is: "nextcloud", "owncloud"
    /// or "opencloud" (they speak the same WebDAV + OCS). Chosen in the
    /// Service picker; found from the server's status page for accounts
    /// made before the picker told them apart. Empty = not known yet.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) product: String,
    /// The server's base URL, e.g. `https://cloud.example.com` (not used
    /// by Dropbox).
    #[serde(default)]
    pub(crate) url: String,
    /// The login (for Dropbox, the account's e-mail, filled in by the
    /// sign-in).
    #[serde(default)]
    pub(crate) user: String,
    /// The folder uploads go to, under the user's files (for Seafile,
    /// inside the library).
    #[serde(default = "default_folder")]
    pub(crate) folder: String,
    /// Seafile: the library uploads go to, made when missing.
    #[serde(default = "default_folder")]
    pub(crate) library: String,
    /// Dropbox: the app key of the Dropbox app the sign-in runs as; empty
    /// means the build's own (see `oauth::provider_credentials`).
    #[serde(default)]
    pub(crate) client_id: String,
    /// OneDrive: the GNOME Online Accounts account id.
    #[serde(default)]
    pub(crate) goa_id: String,
    /// Links expire this many days after upload; 0 keeps them.
    #[serde(default)]
    pub(crate) expire_days: u32,
    /// Off, the account stays configured but is not offered in the
    /// composer, like a paused mail account.
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    /// Protect every link with a generated download password.
    #[serde(default)]
    pub(crate) password: bool,
    /// What the service allows on a link, found out when the account was
    /// checked or saved (`probe_link_terms`); unknown means assume yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) link_expiry: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) link_password: Option<bool>,
    /// Why, in a phrase for the greyed-out rows.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) link_note: String,
    /// A self-hosted server (Nextcloud-kind, Seafile) reached through
    /// Cloudflare, whose proxy refuses a request body over 100 MB: uploads
    /// go in `CLOUDFLARE_CHUNK` pieces the server reassembles.
    #[serde(default)]
    pub(crate) cloudflare: bool,
}

/// One entry of the Service picker: a kind, and for the WebDAV kinds the
/// product, with the name shown and the brand id of its mark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Service {
    pub(crate) kind: CloudKind,
    pub(crate) product: &'static str,
    pub(crate) name: &'static str,
}

impl Service {
    /// The brand id of its mark (`brand::image`).
    pub(crate) fn brand(&self) -> &'static str {
        match self.kind {
            CloudKind::Nextcloud => self.product,
            CloudKind::Dropbox => "dropbox",
            CloudKind::Seafile => "seafile",
            CloudKind::OneDrive => "onedrive",
        }
    }
}

/// The services offered, in picker order. Names are the services' own
/// and are not translated.
pub(crate) const SERVICES: [Service; 6] = [
    Service {
        kind: CloudKind::Nextcloud,
        product: "nextcloud",
        name: "Nextcloud",
    },
    Service {
        kind: CloudKind::Nextcloud,
        product: "owncloud",
        name: "ownCloud",
    },
    Service {
        kind: CloudKind::Nextcloud,
        product: "opencloud",
        name: "OpenCloud",
    },
    Service {
        kind: CloudKind::OneDrive,
        product: "",
        name: "OneDrive",
    },
    Service {
        kind: CloudKind::Dropbox,
        product: "",
        name: "Dropbox",
    },
    Service {
        kind: CloudKind::Seafile,
        product: "",
        name: "Seafile",
    },
];

/// What a service lets a public link carry, as found by
/// [`probe_link_terms`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LinkTerms {
    pub(super) expiry: bool,
    pub(super) password: bool,
    /// Why something is off, for the rows that show it.
    pub(super) note: String,
}

fn default_folder() -> String {
    "Hylki".to_string()
}

fn default_true() -> bool {
    true
}

impl CloudAccount {
    pub(crate) fn empty() -> Self {
        CloudAccount {
            name: String::new(),
            kind: CloudKind::Nextcloud,
            product: "nextcloud".to_string(),
            url: String::new(),
            user: String::new(),
            folder: default_folder(),
            library: default_folder(),
            client_id: String::new(),
            goa_id: String::new(),
            expire_days: 7,
            enabled: true,
            password: false,
            link_expiry: None,
            link_password: None,
            link_note: String::new(),
            cloudflare: false,
        }
    }

    /// The brand id of the service this account is on (`brand::image`):
    /// the product for a Nextcloud-kind server, else the kind's own name.
    /// Empty while a pre-picker server's product is still unknown.
    pub(crate) fn brand(&self) -> &str {
        match self.kind {
            CloudKind::Nextcloud => self.product.as_str(),
            CloudKind::Dropbox => "dropbox",
            CloudKind::Seafile => "seafile",
            CloudKind::OneDrive => "onedrive",
        }
    }

    /// The Service picker entry this account matches, if any.
    pub(crate) fn service_index(&self) -> Option<usize> {
        SERVICES.iter().position(|s| {
            s.kind == self.kind && (self.kind != CloudKind::Nextcloud || s.product == self.product)
        })
    }

    /// Whether the service lets this account's links expire (assumed
    /// until a probe said otherwise).
    pub(crate) fn expiry_allowed(&self) -> bool {
        self.link_expiry.unwrap_or(true)
    }

    /// Whether the service lets this account's links take a password.
    pub(crate) fn password_allowed(&self) -> bool {
        self.link_password.unwrap_or(true)
    }

    /// Record what a probe found.
    pub(crate) fn set_link_terms(&mut self, terms: Option<&LinkTerms>) {
        match terms {
            Some(t) => {
                self.link_expiry = Some(t.expiry);
                self.link_password = Some(t.password);
                self.link_note = t.note.clone();
            }
            None => {
                self.link_expiry = None;
                self.link_password = None;
                self.link_note.clear();
            }
        }
        if !self.expiry_allowed() {
            self.expire_days = 0;
        }
        if !self.password_allowed() {
            self.password = false;
        }
    }

    /// The base URL as the requests want it: a scheme, no trailing slash.
    pub(crate) fn base(&self) -> String {
        match self.kind {
            CloudKind::Dropbox => return "https://www.dropbox.com".to_string(),
            CloudKind::OneDrive => return "https://onedrive.live.com".to_string(),
            _ => {}
        }
        let u = self.url.trim().trim_end_matches('/');
        if u.contains("://") {
            u.to_string()
        } else {
            format!("https://{u}")
        }
    }

    /// The keyring key for this account's secret: the app password, the
    /// Seafile password or token, or the Dropbox refresh token.
    pub(crate) fn key(&self) -> String {
        match self.kind {
            CloudKind::Dropbox => format!("cloud:dropbox|{}", self.user.trim()),
            CloudKind::OneDrive => format!("cloud:goa|{}", self.goa_id.trim()),
            _ => format!("cloud:{}|{}", self.base(), self.user.trim()),
        }
    }

    /// Whether the keyring holds a secret for this account: not for one
    /// signed in through GOA, which keeps the sign-in itself.
    pub(crate) fn has_secret(&self) -> bool {
        !self.kind.via_goa()
    }

    /// Where the account is, for a list row: the server, or the service.
    pub(crate) fn where_shown(&self) -> String {
        match self.kind {
            CloudKind::Dropbox => "Dropbox".to_string(),
            CloudKind::OneDrive => "OneDrive".to_string(),
            _ => self.base(),
        }
    }

    pub(super) fn folder_clean(&self) -> String {
        let f = self.folder.trim().trim_matches('/');
        if f.is_empty() {
            default_folder()
        } else {
            f.to_string()
        }
    }

    pub(super) fn library_clean(&self) -> String {
        let l = self.library.trim();
        if l.is_empty() {
            default_folder()
        } else {
            l.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_gets_a_scheme_and_loses_its_slash() {
        let mut a = CloudAccount::empty();
        a.url = "cloud.example.com/".into();
        assert_eq!(a.base(), "https://cloud.example.com");
        a.url = "http://localhost:8080/nextcloud/".into();
        assert_eq!(a.base(), "http://localhost:8080/nextcloud");
    }

    #[test]
    fn cloudflare_flag_defaults_off_and_round_trips() {
        let a: CloudAccount = toml::from_str("name = \"x\"\n").unwrap();
        assert!(!a.cloudflare);
        let b: CloudAccount = toml::from_str("name = \"x\"\ncloudflare = true\n").unwrap();
        assert!(b.cloudflare);
    }
}
