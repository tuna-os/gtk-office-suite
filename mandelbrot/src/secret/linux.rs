//! Linux API to store the data of a session, using the Secret Service or Secret
//! portal.

use std::{collections::HashMap, path::Path};

use futures_util::future::{self, Either};
use gettextrs::gettext;
use gtk::glib;
use matrix_sdk::authentication::oauth::ClientId;
use oo7::{Item, Keyring};
use ruma::UserId;
use serde::Deserialize;
use thiserror::Error;
use tokio::fs;
use tracing::{debug, error, info};
use url::Url;

use super::{SESSION_ID_LENGTH, SecretError, SecretExt, SessionTokens, StoredSession};
use crate::{APP_ID, PROFILE, gettext_f, prelude::*, spawn_tokio, utils::matrix};

/// The current version of the stored session.
const CURRENT_VERSION: u8 = 7;
/// The minimum supported version for the stored sessions.
///
/// Currently, this matches the version when Fractal 5 was released.
const MIN_SUPPORTED_VERSION: u8 = 4;

/// Keys used in the Linux secret backend.
mod keys {
    /// The attribute for the schema in the Secret Service.
    pub(super) const XDG_SCHEMA: &str = "xdg:schema";
    /// The attribute for the profile of the app.
    pub(super) const PROFILE: &str = "profile";
    /// The attribute for the version of the stored session.
    pub(super) const VERSION: &str = "version";
    /// The attribute for the URL of the homeserver.
    pub(super) const HOMESERVER: &str = "homeserver";
    /// The attribute for the user ID.
    pub(super) const USER: &str = "user";
    /// The attribute for the device ID.
    pub(super) const DEVICE_ID: &str = "device-id";
    /// The deprecated attribute for the database path.
    pub(super) const DB_PATH: &str = "db-path";
    /// The attribute for the session ID.
    pub(super) const ID: &str = "id";
    /// The attribute for the client ID.
    pub(super) const CLIENT_ID: &str = "client-id";
}

/// Secret API under Linux.
pub(crate) struct LinuxSecret;

impl SecretExt for LinuxSecret {
    async fn restore_sessions() -> Result<Vec<StoredSession>, SecretError> {
        /// The timeout, in seconds, for the session restore via the Secret
        /// portal.
        const RESTORE_TIMEOUT: u32 = 10;

        let handle = spawn_tokio!(async move { restore_sessions_inner().await });
        let timeout = std::pin::pin!(glib::timeout_future_seconds(RESTORE_TIMEOUT));

        match future::select(handle, timeout).await {
            Either::Left((result, _)) => match result.expect("task was not aborted") {
                Ok(sessions) => Ok(sessions),
                Err(error) => {
                    error!("Could not restore previous sessions: secret error: {error}");
                    Err(error.into())
                }
            },
            Either::Right(_) => {
                error!("Timeout after {RESTORE_TIMEOUT}s while restoring previous sessions");
                Err(SecretError::Service(gettext(
                    "The secret storage service did not respond in time. Make sure a keyring is available.",
                )))
            }
        }
    }

    async fn store_session(session: StoredSession) -> Result<(), SecretError> {
        let handle = spawn_tokio!(async move { store_session_inner(session).await });
        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not store session: {error}");
                Err(error.into())
            }
        }
    }

    async fn delete_session(session: &StoredSession) {
        let attributes = session.attributes();

        spawn_tokio!(async move {
            if let Err(error) = delete_item_with_attributes(&attributes).await {
                error!("Could not delete session data from secret backend: {error}");
            }
        })
        .await
        .expect("task was not aborted");
    }
}

async fn restore_sessions_inner() -> Result<Vec<StoredSession>, oo7::Error> {
    let keyring = Keyring::new().await?;

    keyring.unlock().await?;

    let items = keyring
        .search_items(&HashMap::from([
            (keys::XDG_SCHEMA, APP_ID),
            (keys::PROFILE, PROFILE.as_str()),
        ]))
        .await?;

    let mut sessions = Vec::with_capacity(items.len());

    for item in items {
        item.unlock().await?;

        match StoredSession::try_from_secret_item(item).await {
            Ok(session) => sessions.push(session),
            Err(LinuxSecretError::OldVersion {
                version,
                mut session,
                item,
                access_token,
            }) => {
                if version < MIN_SUPPORTED_VERSION {
                    info!(
                        "Found old session for user {} with version {version} that is no longer supported, removing…",
                        session.user_id
                    );

                    // Try to log it out.
                    if let Some(access_token) = access_token {
                        log_out_session(session.clone(), access_token).await;
                    }

                    // Delete the session from the secret backend.
                    LinuxSecret::delete_session(&session).await;

                    // Delete the session data folders.
                    spawn_tokio!(async move {
                        if let Err(error) = fs::remove_dir_all(session.data_path()).await {
                            error!("Could not remove session database: {error}");
                        }

                        if version >= 6
                            && let Err(error) = fs::remove_dir_all(session.cache_path()).await
                        {
                            error!("Could not remove session cache: {error}");
                        }
                    })
                    .await
                    .expect("task was not aborted");

                    continue;
                }

                info!(
                    "Found session {} for user {} with old version {}, applying migrations…",
                    session.id, session.user_id, version,
                );
                session.apply_migrations(version, item, access_token).await;

                sessions.push(session);
            }
            Err(LinuxSecretError::Field(LinuxSecretFieldError::Invalid)) => {
                // We already log the specific errors for this.
            }
            Err(error) => {
                error!("Could not restore previous session: secret error: {error}");
            }
        }
    }

    Ok(sessions)
}

async fn store_session_inner(session: StoredSession) -> Result<(), oo7::Error> {
    let keyring = Keyring::new().await?;

    let attributes = session.attributes();
    let secret = oo7::Secret::text(session.passphrase);

    keyring
        .create_item(
            &gettext_f(
                // Translators: Do NOT translate the content between '{' and '}', this is a
                // variable name.
                "Mandelbrot: Matrix credentials for {user_id}",
                &[("user_id", session.user_id.as_str())],
            ),
            &attributes,
            secret,
            true,
        )
        .await?;

    Ok(())
}

/// Create a client and log out the given session.
async fn log_out_session(session: StoredSession, access_token: String) {
    debug!("Logging out session");

    let tokens = SessionTokens {
        access_token,
        refresh_token: None,
    };

    spawn_tokio!(async move {
        match matrix::client_with_stored_session(session, tokens).await {
            Ok(client) => {
                if let Err(error) = client.logout().await {
                    error!("Could not log out session: {error}");
                }
            }
            Err(error) => {
                error!("Could not build client to log out session: {error}");
            }
        }
    })
    .await
    .expect("task was not aborted");
}

impl StoredSession {
    /// Build self from an item.
    #[allow(clippy::result_large_err)]
    async fn try_from_secret_item(item: Item) -> Result<Self, LinuxSecretError> {
        let attributes = item.attributes().await?;

        let version = parse_attribute(&attributes, keys::VERSION, str::parse::<u8>)?;
        if version > CURRENT_VERSION {
            return Err(LinuxSecretError::UnsupportedVersion(version));
        }

        let homeserver = parse_attribute(&attributes, keys::HOMESERVER, Url::parse)?;
        let user_id = parse_attribute(&attributes, keys::USER, |s| UserId::parse(s))?;
        let device_id = get_attribute(&attributes, keys::DEVICE_ID)?.as_str().into();

        let id = if version <= 5 {
            let string = get_attribute(&attributes, keys::DB_PATH)?;
            Path::new(string)
                .iter()
                .next_back()
                .and_then(|s| s.to_str())
                .expect("Session ID in db-path should be valid UTF-8")
                .to_owned()
        } else {
            get_attribute(&attributes, keys::ID)?.clone()
        };

        let client_id = attributes.get(keys::CLIENT_ID).cloned().map(ClientId::new);

        let (passphrase, access_token) = match item.secret().await {
            Ok(secret) => {
                if version <= 6 {
                    let secret_data = if version <= 4 {
                        match rmp_serde::from_slice::<V4SecretData>(&secret) {
                            Ok(secret) => secret,
                            Err(error) => {
                                error!("Could not parse secret in stored session: {error}");
                                return Err(LinuxSecretFieldError::Invalid.into());
                            }
                        }
                    } else {
                        match serde_json::from_slice(&secret) {
                            Ok(secret) => secret,
                            Err(error) => {
                                error!("Could not parse secret in stored session: {error:?}");
                                return Err(LinuxSecretFieldError::Invalid.into());
                            }
                        }
                    };

                    (secret_data.passphrase, Some(secret_data.access_token))
                } else {
                    // Even if we store the secret as plain text, the file backend always returns a
                    // blob so let's always treat it as a byte slice.
                    match String::from_utf8(secret.as_bytes().to_owned()) {
                        Ok(passphrase) => (passphrase.clone(), None),
                        Err(error) => {
                            error!("Could not get secret in stored session: {error}");
                            return Err(LinuxSecretFieldError::Invalid.into());
                        }
                    }
                }
            }
            Err(error) => {
                error!("Could not get secret in stored session: {error}");
                return Err(LinuxSecretFieldError::Invalid.into());
            }
        };

        let session = Self {
            homeserver,
            user_id,
            device_id,
            id,
            client_id,
            passphrase: passphrase.into(),
        };

        if version < CURRENT_VERSION {
            Err(LinuxSecretError::OldVersion {
                version,
                session,
                item,
                access_token,
            })
        } else {
            Ok(session)
        }
    }

    /// Get the attributes from `self`.
    fn attributes(&self) -> HashMap<&'static str, String> {
        let mut attributes = HashMap::from([
            (keys::HOMESERVER, self.homeserver.to_string()),
            (keys::USER, self.user_id.to_string()),
            (keys::DEVICE_ID, self.device_id.to_string()),
            (keys::ID, self.id.clone()),
            (keys::VERSION, CURRENT_VERSION.to_string()),
            (keys::PROFILE, PROFILE.to_string()),
            (keys::XDG_SCHEMA, APP_ID.to_owned()),
        ]);

        if let Some(client_id) = &self.client_id {
            attributes.insert(keys::CLIENT_ID, client_id.as_str().to_owned());
        }

        attributes
    }

    /// Migrate this session to the current version.
    async fn apply_migrations(
        &mut self,
        from_version: u8,
        item: Item,
        access_token: Option<String>,
    ) {
        // Version 5 changes the serialization of the secret from MessagePack to JSON.
        // We can ignore the migration because we changed the format of the secret again
        // in version 7.

        if from_version < 6 {
            // Version 6 truncates sessions IDs, changing the path of the databases, and
            // removes the `db-path` attribute to replace it with the `id` attribute.
            // Because we need to update the `version` in the attributes for version 7, we
            // only migrate the path here.
            info!("Migrating to version 6…");

            // Get the old path of the session.
            let old_path = self.data_path();

            // Truncate the session ID.
            self.id.truncate(SESSION_ID_LENGTH);
            let new_path = self.data_path();

            spawn_tokio!(async move {
                debug!(
                    "Renaming databases directory to: {}",
                    new_path.to_string_lossy()
                );

                if let Err(error) = fs::rename(old_path, new_path).await {
                    error!("Could not rename databases directory: {error}");
                }
            })
            .await
            .expect("task was not aborted");
        }

        if from_version < 7 {
            // Version 7 moves the access token to a separate file. Only the passphrase is
            // stored as the secret now.
            info!("Migrating to version 7…");

            let new_attributes = self.attributes();
            let new_secret = oo7::Secret::text(&self.passphrase);

            spawn_tokio!(async move {
                if let Err(error) = item.set_secret(new_secret).await {
                    error!("Could not store updated session secret: {error}");
                }

                if let Err(error) = item.set_attributes(&new_attributes).await {
                    error!("Could not store updated session attributes: {error}");
                }
            })
            .await
            .expect("task was not aborted");

            if let Some(access_token) = access_token {
                let session_tokens = SessionTokens {
                    access_token,
                    refresh_token: None,
                };
                self.store_tokens(session_tokens).await;
            }
        }
    }
}

/// Secret data that was stored in the secret backend from versions 4 through 6.
#[derive(Clone, Deserialize)]
struct V4SecretData {
    /// The access token to provide to the homeserver for authentication.
    access_token: String,
    /// The passphrase used to encrypt the local databases.
    passphrase: String,
}

/// Get the attribute with the given key in the given map.
fn get_attribute<'a>(
    attributes: &'a HashMap<String, String>,
    key: &'static str,
) -> Result<&'a String, LinuxSecretFieldError> {
    attributes
        .get(key)
        .ok_or(LinuxSecretFieldError::Missing(key))
}

/// Parse the attribute with the given key, using the given parsing function in
/// the given map.
fn parse_attribute<F, V, E>(
    attributes: &HashMap<String, String>,
    key: &'static str,
    parse: F,
) -> Result<V, LinuxSecretFieldError>
where
    F: FnOnce(&str) -> Result<V, E>,
    E: std::fmt::Display,
{
    let string = get_attribute(attributes, key)?;
    match parse(string) {
        Ok(value) => Ok(value),
        Err(error) => {
            error!("Could not parse {key} in stored session: {error}");
            Err(LinuxSecretFieldError::Invalid)
        }
    }
}

/// Any error that can happen when retrieving an attribute from the secret
/// backends on Linux.
#[derive(Debug, Error)]
enum LinuxSecretFieldError {
    /// An attribute is missing.
    ///
    /// This should only happen if for some reason we get an item from a
    /// different application.
    #[error("Could not find {0} in stored session")]
    Missing(&'static str),

    /// An invalid attribute was found.
    ///
    /// This should only happen if for some reason we get an item from a
    /// different application.
    #[error("Invalid field in stored session")]
    Invalid,
}

/// Remove the item with the given attributes from the secret backend.
async fn delete_item_with_attributes(
    attributes: &impl oo7::AsAttributes,
) -> Result<(), oo7::Error> {
    let keyring = Keyring::new().await?;
    keyring.delete(attributes).await?;

    Ok(())
}

/// Any error that can happen when interacting with the secret backends on
/// Linux.
#[derive(Debug, Error)]
// Complains about StoredSession in OldVersion, but we need it.
#[allow(clippy::large_enum_variant)]
enum LinuxSecretError {
    /// A session with an unsupported version was found.
    #[error("Session found with unsupported version {0}")]
    UnsupportedVersion(u8),

    /// A session with an old version was found.
    #[error("Session found with old version")]
    OldVersion {
        /// The version that was found.
        version: u8,
        /// The session that was found.
        session: StoredSession,
        /// The item for the session.
        item: Item,
        /// The access token that was found.
        ///
        /// It needs to be stored outside of the secret backend now.
        access_token: Option<String>,
    },

    /// An error occurred while retrieving a field of the session.
    ///
    /// This should only happen if for some reason we get an item from a
    /// different application.
    #[error(transparent)]
    Field(#[from] LinuxSecretFieldError),

    /// An error occurred while interacting with the secret backend.
    #[error(transparent)]
    Oo7(#[from] oo7::Error),
}

impl From<oo7::Error> for SecretError {
    fn from(value: oo7::Error) -> Self {
        Self::Service(value.to_user_facing())
    }
}

impl UserFacingError for oo7::Error {
    fn to_user_facing(&self) -> String {
        match self {
            oo7::Error::File(error) => error.to_user_facing(),
            oo7::Error::DBus(error) => error.to_user_facing(),
        }
    }
}

impl UserFacingError for oo7::file::Error {
    fn to_user_facing(&self) -> String {
        use oo7::file::Error;

        match self {
            Error::FileHeaderMismatch(_)
            | Error::VersionMismatch(_)
            | Error::NoData
            | Error::MacError
            | Error::HashedAttributeMac(_)
            | Error::GVariantDeserialization(_)
            | Error::SaltSizeMismatch(..)
            | Error::ChecksumMismatch
            | Error::AlgorithmMismatch(_)
            | Error::IncorrectSecret
            | Error::Crypto(_)
            | Error::Utf8(_)
            | Error::PartiallyCorruptedKeyring { .. } => {
                gettext("The secret storage file is corrupted.")
            }
            Error::NoParentDir(_) | Error::NoDataDir => {
                gettext("Could not access the secret storage file location.")
            }
            Error::Io(_) => {
                gettext("An unexpected error occurred when accessing the secret storage file.")
            }
            Error::TargetFileChanged(_) => {
                gettext("The secret storage file has been changed by another process.")
            }
            Error::Portal(ashpd::Error::Portal(ashpd::PortalError::Cancelled(_))) => gettext(
                "The request to the Flatpak Secret Portal was cancelled. Make sure to accept any prompt asking to access it.",
            ),
            Error::Portal(ashpd::Error::PortalNotFound(_)) => gettext(
                "The Flatpak Secret Portal is not available. Make sure xdg-desktop-portal is installed, and it is at least at version 1.5.0.",
            ),
            Error::Portal(_) => gettext(
                "An unexpected error occurred when interacting with the D-Bus Secret Portal backend.",
            ),
            Error::WeakKey(_) => {
                gettext("The Flatpak Secret Portal provided a key that is too weak to be secure.")
            }
            Error::Locked => gettext("The collection or item is locked."),
            // Can only occur when using APIs that we do not use.
            Error::InvalidItemIndex(_) | Error::InvalidKeyLength { .. } => unreachable!(),
        }
    }
}

impl UserFacingError for oo7::dbus::Error {
    fn to_user_facing(&self) -> String {
        use oo7::dbus::{Error, ServiceError};

        match self {
            Error::Deleted => gettext("The item was deleted."),
            Error::Service(s) => match s {
                ServiceError::ZBus(_) => gettext(
                    "An unexpected error occurred when interacting with the D-Bus Secret Service.",
                ),
                ServiceError::IsLocked(_) => gettext("The collection or item is locked."),
                ServiceError::NoSession(_) => {
                    gettext("The D-Bus Secret Service session does not exist.")
                }
                ServiceError::NoSuchObject(_) => gettext("The collection or item does not exist."),
            },
            Error::Dismissed => gettext(
                "The request to the D-Bus Secret Service was cancelled. Make sure to accept any prompt asking to access it.",
            ),
            Error::NotFound(_) => gettext(
                "Could not access the default collection. Make sure a keyring was created and set as default.",
            ),
            Error::ZBus(_) | Error::Crypto(_) | Error::IO(_) => gettext(
                "An unexpected error occurred when interacting with the D-Bus Secret Service.",
            ),
        }
    }
}
