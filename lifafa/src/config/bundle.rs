//! Export and import of every setting as one TOML bundle (#50).

use std::path::PathBuf;

use super::accounts::{load, save, AccountConfig};
use super::filters::{load_filters, save_filters, FilterRule};
use super::privacy::{load_privacy, privacy_path, PrivacyFile};
use super::state::{state_path, StateFile};
use super::tags::{load_tags, save_tags, Tag};
use super::window::{sidebar_path, window_path, SidebarFile, WindowFile};
use super::{read_toml, write_private};

/// A portable settings bundle (#50): every configuration file Hylki keeps —
/// preferences, accounts (colors, emoji, labels, aliases, folder roles and
/// per-account push included), filters, tags, cloud storage accounts,
/// sidebar layout, window/pane state (the app icon choice with it), and
/// the words taught to the spell checker. Passwords and tokens never
/// appear — their fields are skip_serializing, and they live in the
/// keyring, not on disk; OpenPGP keys live in GnuPG's keyring, and the
/// optional oauth.toml (a client id and secret the user supplied by hand)
/// stays out for the same reason.
///
/// Sections added after the first release are optional, so a bundle from
/// before them imports without wiping what it does not mention.
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct SettingsBundle {
    pub(super) version: u32,
    pub(super) privacy: PrivacyFile,
    #[serde(default)]
    pub(super) accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub(super) filters: Vec<FilterRule>,
    #[serde(default)]
    pub(super) tags: Vec<Tag>,
    #[serde(default)]
    pub(super) sidebar: Option<SidebarFile>,
    #[serde(default)]
    pub(super) window: Option<WindowFile>,
    #[serde(default)]
    pub(super) state: Option<StateFile>,
    /// Cloud storage accounts (#144 onward); their sign-ins stay in the
    /// keyring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) cloud: Option<Vec<crate::services::cloud::CloudAccount>>,
    /// The spell checker's personal words, per language (#114).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) spell_words: Option<std::collections::BTreeMap<String, Vec<String>>>,
}

/// Serialize a bundle section back to its config file.
fn write_file_struct<T: serde::Serialize>(
    path: Option<PathBuf>,
    value: &Option<T>,
) -> Result<(), String> {
    let (Some(path), Some(value)) = (path, value) else {
        return Ok(());
    };
    let toml = toml::to_string_pretty(value).map_err(|e| e.to_string())?;
    write_private(&path, &toml).map_err(|e| e.to_string())
}

/// The current configuration as a TOML bundle for Export Settings.
pub(crate) fn export_bundle() -> Result<String, String> {
    let bundle = SettingsBundle {
        version: 1,
        privacy: load_privacy(),
        accounts: load().unwrap_or_default(),
        filters: load_filters(),
        tags: load_tags(),
        sidebar: read_toml(sidebar_path()),
        window: read_toml(window_path()),
        state: read_toml(state_path()),
        cloud: Some(crate::services::cloud::load_accounts()),
        spell_words: Some(crate::desktop::spell::all_personal_words()),
    };
    toml::to_string_pretty(&bundle).map_err(|e| e.to_string())
}

/// Parse and persist an exported bundle: privacy.toml and accounts.toml are
/// replaced (via the same writers the app uses; the keyring is untouched
/// since imported accounts carry no secrets). Returns the account count.
pub(crate) fn import_bundle(text: &str) -> Result<usize, String> {
    let bundle: SettingsBundle = toml::from_str(text).map_err(|e| e.to_string())?;
    if bundle.version != 1 {
        return Err(format!("unsupported bundle version {}", bundle.version));
    }
    let path = privacy_path().ok_or("no config directory")?;
    let toml = toml::to_string_pretty(&bundle.privacy).map_err(|e| e.to_string())?;
    write_private(&path, &toml).map_err(|e| e.to_string())?;
    save(&bundle.accounts).map_err(|e| e.to_string())?;
    save_filters(&bundle.filters);
    save_tags(&bundle.tags);
    write_file_struct(sidebar_path(), &bundle.sidebar)?;
    write_file_struct(window_path(), &bundle.window)?;
    write_file_struct(state_path(), &bundle.state)?;
    if let Some(cloud) = &bundle.cloud {
        crate::services::cloud::save_accounts(cloud);
    }
    // Words are merged, never removed: a backup can only teach the
    // checker more.
    if let Some(words) = &bundle.spell_words {
        for (lang, list) in words {
            crate::desktop::spell::merge_personal_words(lang, list);
        }
    }
    Ok(bundle.accounts.len())
}
