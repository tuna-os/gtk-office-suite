//! Wire objects, spec section 4. Every object rejects unknown fields, as the
//! schemas do (`additionalProperties: false`).

use serde::{Deserialize, Serialize};

/// Spec section 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Commenter,
    Editor,
    Owner,
}

/// Spec 4.1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub dcp: u8,
    pub title: String,
    pub mime: String,
    pub levels: Vec<Level>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithms: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_services: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Level {
    L0,
    L1,
    L2,
    L3,
}

/// Spec 4.2.1. `mime`, `size` and `sha256` describe the plaintext file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub mime: String,
    pub size: u64,
    pub sha256: String,
    pub locator: Locator,
}

/// Spec 4.2.1 locator table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Locator {
    #[serde(rename = "matrix-encrypted-media")]
    MatrixEncryptedMedia { file: serde_json::Value },
    #[serde(rename = "matrix-media")]
    MatrixMedia { url: String },
    #[serde(rename = "webdav")]
    Webdav { href: String },
    #[serde(rename = "url")]
    Url { href: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Author {
    pub principal: String,
    pub device: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Covers {
    pub algorithm: String,
    pub through: String,
}

/// Spec 4.2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub dcp: u8,
    pub parents: Vec<String>,
    pub file: FileRef,
    pub created_by: Author,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covers: Option<Covers>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseState {
    Held,
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Holder {
    pub principal: String,
    pub session: String,
}

/// Spec 4.3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub dcp: u8,
    pub state: LeaseState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<Holder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Omitted when the document has no version yet (never a placeholder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_version: Option<String>,
    /// Required when held (lease rule 8); absent on a released record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renewal: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Spec 4.4.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub dcp: u8,
    pub algorithm: String,
    pub doc_model: String,
    pub base_version: String,
    pub seq: u64,
    pub payload: Payload,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding")]
pub enum Payload {
    #[serde(rename = "base64")]
    Base64 { data: String, size: u64 },
    #[serde(rename = "file")]
    File { file: FileRef },
}

/// A DCP object together with what the binding stamped on it: the identifier
/// the binding assigned (a Matrix event ID, a WOPI file version string, ...),
/// the binding's own timestamp (spec 4.3 lease rule 1) and the sender it
/// authenticated (spec 11: authorship comes from the binding).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamped<T> {
    pub id: String,
    pub ts_ms: u64,
    pub sender: String,
    pub body: T,
    /// The device the binding says sent it, when it can say (a Matrix
    /// encrypted event carries the sending device; WOPI has none). Updates are
    /// sequenced per `(principal, device)` (spec 4.4), so the update stream
    /// needs it; `None` makes the principal's updates one stream.
    pub device: Option<String>,
}

impl<T> Stamped<T> {
    pub fn new(id: impl Into<String>, ts_ms: u64, sender: impl Into<String>, body: T) -> Self {
        Stamped {
            id: id.into(),
            ts_ms,
            sender: sender.into(),
            body,
            device: None,
        }
    }

    pub fn with_device(mut self, device: impl Into<String>) -> Self {
        self.device = Some(device.into());
        self
    }
}
