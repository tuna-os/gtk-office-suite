//! The header extension of every operation of this binding, and why it is chunked.
//!
//! p2panda-core 0.7.1 decodes a header with `cbor_core::DecodeOptions::length_limit(512)`
//! (`operation/any.rs`, "set rather low / pessimistic thresholds"): no CBOR array, map, byte string
//! or text string in a header may be longer than 512 elements. A `p2panda_spaces::SpacesArgs`
//! value carries its ciphertext (`Vec<u8>`) and its direct messages in the header extension, so an
//! encrypted entry of more than about 500 bytes is written to the database fine and then cannot be
//! read back: `Header::decode` refuses it, and so does every sync session. This binding therefore
//! serialises the spaces message itself to CBOR and stores it as byte strings of at most
//! [`CHUNK`] bytes. `tests/header_limit.rs` measures the limit and this workaround.

use std::borrow::Borrow;

use p2panda_core::cbor::{decode_cbor, encode_cbor};
use p2panda_spaces::SpacesArgs;
use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_bytes::ByteBuf;

/// Bytes per chunk. Below the 512 limit with room to spare.
pub const CHUNK: usize = 480;
/// The `chunks` array may not exceed 512 elements either, so an operation carries at most
/// `CHUNK * MAX_CHUNKS` bytes of CBOR: about 240 KB.
pub const MAX_CHUNKS: usize = 500;

#[derive(Clone, Debug)]
pub struct DcpExt {
    /// The p2panda-spaces message: key bundle, auth, space membership or encrypted application data.
    pub args: SpacesArgs<()>,
    /// The author's own wall clock in microseconds when it wrote the operation. Not authenticated
    /// time: the author chose it.
    pub ts_us: u64,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    ts_us: u64,
    chunks: Vec<ByteBuf>,
}

impl DcpExt {
    /// The size of the serialised spaces message (what is chunked).
    pub fn cbor_len(&self) -> Result<usize, String> {
        encode_cbor(&self.args)
            .map(|b| b.len())
            .map_err(|e| e.to_string())
    }
}

impl Serialize for DcpExt {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let bytes = encode_cbor(&self.args).map_err(S::Error::custom)?;
        let chunks: Vec<ByteBuf> = bytes
            .chunks(CHUNK)
            .map(|c| ByteBuf::from(c.to_vec()))
            .collect();
        if chunks.len() > MAX_CHUNKS {
            return Err(S::Error::custom(format!(
                "entry of {} bytes needs {} chunks, at most {MAX_CHUNKS} fit in a header",
                bytes.len(),
                chunks.len()
            )));
        }
        Wire {
            ts_us: self.ts_us,
            chunks,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for DcpExt {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(d)?;
        let mut bytes = Vec::with_capacity(wire.chunks.iter().map(|c| c.len()).sum());
        for c in &wire.chunks {
            bytes.extend_from_slice(c);
        }
        let args: SpacesArgs<()> = decode_cbor(&bytes[..]).map_err(D::Error::custom)?;
        Ok(DcpExt {
            args,
            ts_us: wire.ts_us,
        })
    }
}

impl Borrow<SpacesArgs<()>> for DcpExt {
    fn borrow(&self) -> &SpacesArgs<()> {
        &self.args
    }
}
