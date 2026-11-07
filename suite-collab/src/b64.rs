// SPDX-License-Identifier: GPL-3.0-or-later
//! Standard-alphabet base64 with padding (RFC 4648 section 4), the encoding of
//! every byte payload on the wire.
//!
//! Written here, not taken from a crate, because this crate's whole point is
//! to add nothing to the product lockfile; the test vectors are RFC 4648
//! section 10.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B64Error {
    /// The length is not a multiple of four.
    Length,
    /// A byte outside the alphabet, or padding anywhere but the end.
    Character(char),
    /// Padding bits that are not zero: two texts would decode to the same bytes.
    NonCanonical,
}

impl std::fmt::Display for B64Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            B64Error::Length => write!(f, "base64 length is not a multiple of 4"),
            B64Error::Character(c) => write!(f, "{c:?} is not valid base64 here"),
            B64Error::NonCanonical => write!(f, "base64 padding bits are not zero"),
        }
    }
}

impl std::error::Error for B64Error {}

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

fn value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

pub fn decode(text: &str) -> Result<Vec<u8>, B64Error> {
    let t = text.as_bytes();
    if !t.len().is_multiple_of(4) {
        return Err(B64Error::Length);
    }
    let mut out = Vec::with_capacity(t.len() / 4 * 3);
    let quads = t.len() / 4;
    for (i, q) in t.chunks(4).enumerate() {
        let last = i + 1 == quads;
        let pad = if last { q.iter().rev().take_while(|&&c| c == b'=').count() } else { 0 };
        if pad > 2 {
            return Err(B64Error::Character('='));
        }
        let mut n = 0u32;
        for (j, &c) in q.iter().enumerate() {
            let v = if j >= 4 - pad {
                0
            } else {
                value(c).ok_or(B64Error::Character(c as char))?
            };
            n = (n << 6) | v;
        }
        // Whatever the padding left over must be zero, or the encoding is not
        // the one `encode` would have produced.
        if (pad == 1 && n & 0xFF != 0) || (pad == 2 && n & 0xFFFF != 0) {
            return Err(B64Error::NonCanonical);
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

/// `#[serde(with = "b64::bytes")]` for a `Vec<u8>` field.
pub mod bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        super::decode(&text).map_err(serde::de::Error::custom)
    }
}

/// `#[serde(with = "b64::option")]` for an `Option<Vec<u8>>` field; pair it with
/// `default` and `skip_serializing_if = "Option::is_none"`.
pub mod option {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_str(&super::encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        match Option::<String>::deserialize(d)? {
            Some(text) => super::decode(&text).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_4648_vectors() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded).unwrap(), plain.as_bytes());
        }
    }

    #[test]
    fn every_byte_value_round_trips() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)).unwrap(), all);
    }

    #[test]
    fn bad_input_is_refused() {
        assert_eq!(decode("Zg="), Err(B64Error::Length));
        assert!(decode("Zg=a").is_err(), "padding before data");
        assert!(decode("Zm9v\n").is_err());
        assert!(decode("Zm 9").is_err());
        assert!(decode("Zm9-").is_err(), "the URL-safe alphabet is not this encoding");
        assert_eq!(decode("Zh=="), Err(B64Error::NonCanonical));
        assert_eq!(decode("Zm9="), Err(B64Error::NonCanonical));
        assert!(decode("====").is_err());
        assert!(decode("Zm==Zm9v").is_err(), "padding in the middle");
    }
}
