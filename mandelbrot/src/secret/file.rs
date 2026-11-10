//! API to store the session tokens in a file on the system.

use std::path::Path;

use matrix_sdk_store_encryption::{EncryptedValue, StoreCipher};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::fs;

/// An API to read from or write to a file that encodes its content.
pub(super) struct SecretFile;

impl SecretFile {
    /// Read a secret from the file at the given path.
    pub(super) async fn read<T: DeserializeOwned>(
        path: &Path,
        passphrase: &str,
    ) -> Result<T, SecretFileError> {
        let (cipher, encrypted_secret) = Self::read_inner(path, passphrase).await?;
        let serialized_secret = cipher.decrypt_value_data(encrypted_secret)?;
        Ok(rmp_serde::from_slice(&serialized_secret)?)
    }

    async fn read_inner(
        path: &Path,
        passphrase: &str,
    ) -> Result<(StoreCipher, EncryptedValue), SecretFileError> {
        let bytes = fs::read(&path).await?;
        let content = rmp_serde::from_slice::<SecretFileContent>(&bytes)?;
        let cipher = StoreCipher::import(passphrase, &content.encrypted_cipher)?;
        Ok((cipher, content.encrypted_secret))
    }

    /// Get the existing cipher at the given path, or create a new one.
    async fn get_or_create_cipher(
        path: &Path,
        passphrase: &str,
    ) -> Result<StoreCipher, SecretFileError> {
        let cipher = match Self::read_inner(path, passphrase).await {
            Ok((cipher, _)) => cipher,
            Err(_) => StoreCipher::new()?,
        };
        Ok(cipher)
    }

    /// Write a secret to the file at the given path.
    pub(super) async fn write<T: Serialize>(
        path: &Path,
        passphrase: &str,
        secret: &T,
    ) -> Result<(), SecretFileError> {
        let cipher = Self::get_or_create_cipher(path, passphrase).await?;
        // `StoreCipher::encrypt_value()` uses JSON to serialize the data, which shows
        // in the content of the file. To have a more opaque format, we use
        // `rmp_serde::to_vec()` which will not show the fields of
        // `EncryptedValue`.
        let encrypted_secret = cipher.encrypt_value_data(rmp_serde::to_vec_named(secret)?)?;
        let encrypted_cipher = cipher.export(passphrase)?;
        let bytes = rmp_serde::to_vec(&SecretFileContent {
            encrypted_cipher,
            encrypted_secret,
        })?;
        fs::write(path, bytes).await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct SecretFileContent {
    #[serde(with = "serde_bytes")]
    encrypted_cipher: Vec<u8>,
    encrypted_secret: EncryptedValue,
}

/// All errors that can occur when interacting with a secret file.
#[derive(Debug, thiserror::Error)]
pub(super) enum SecretFileError {
    /// An error occurred when accessing the file.
    #[error(transparent)]
    File(#[from] std::io::Error),

    /// An error occurred when decoding the content.
    #[error(transparent)]
    Decode(#[from] rmp_serde::decode::Error),

    /// An error occurred when encoding the content.
    #[error(transparent)]
    Encode(#[from] rmp_serde::encode::Error),

    /// An error occurred when encrypting or decrypting the content.
    #[error(transparent)]
    Encryption(#[from] matrix_sdk_store_encryption::Error),
}
