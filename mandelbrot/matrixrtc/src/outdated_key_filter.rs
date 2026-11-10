// SPDX-License-Identifier: GPL-3.0-or-later

//! Detection of out-of-order (outdated) media keys.

use std::collections::HashMap;

use crate::key_transport::{CallMembershipIdentity, encryption_key_map_key};

/// An encryption key received from another participant.
#[derive(Clone, Debug)]
pub struct InboundEncryptionSession {
    /// The key material.
    pub key: Vec<u8>,
    /// The claimed identity of the sender.
    pub membership: CallMembershipIdentity,
    /// The index (id) of the key.
    pub key_index: u32,
    /// The creation timestamp of the key, in milliseconds.
    pub creation_ts: u64,
}

/// Detects when a key for a given index is outdated.
///
/// There is a possibility that keys arrive in the wrong order. For example,
/// after a quick join/leave/join there will be two keys of index 0
/// distributed, and if they are received in the wrong order the stream won't
/// be decryptable. For that reason we keep a small buffer of key timestamps
/// to disambiguate.
#[derive(Debug, Default)]
pub struct OutdatedKeyFilter {
    /// Map of participant map key -> key index -> timestamp.
    ts_buffer: HashMap<String, HashMap<u32, u64>>,
}

impl OutdatedKeyFilter {
    /// Construct an empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there is a more recent key with the same key index and use
    /// the creation timestamp to decide what to do with the key. If the key
    /// received is older than the one already in the buffer, it is outdated
    /// and should be ignored.
    pub fn is_outdated(
        &mut self,
        membership: &CallMembershipIdentity,
        item: &InboundEncryptionSession,
    ) -> bool {
        let map_key = encryption_key_map_key(membership);
        let buffer = self.ts_buffer.entry(map_key).or_default();

        if let Some(latest_timestamp) = buffer.get(&item.key_index)
            && *latest_timestamp > item.creation_ts
        {
            // The existing key is more recent, ignore this one.
            return true;
        }
        buffer.insert(item.key_index, item.creation_ts);
        false
    }
}
