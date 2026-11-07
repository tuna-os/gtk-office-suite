//! p2panda-core 0.7.1 refuses to decode a header that holds a CBOR array, byte string or text string
//! longer than 512 elements (`operation/any.rs`: `length_limit(512)`). These tests measure that, and
//! that the chunked extension of this binding (`dcp_p2p::ext`) avoids it.

use dcp_p2p::ext::{DcpExt, CHUNK, MAX_CHUNKS};
use p2panda_core::{Header, SigningKey};
use p2panda_encryption::crypto::xchacha20::XAeadNonce;
use p2panda_spaces::SpacesArgs;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PlainVec {
    data: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PlainBytes {
    data: ByteBuf,
}

fn application(ciphertext_len: usize) -> SpacesArgs<()> {
    let nonce: XAeadNonce = [7u8; 24];
    SpacesArgs::Application {
        space_id: p2panda_core::Hash::digest(b"space"),
        space_dependencies: vec![p2panda_core::Hash::digest(b"dep")],
        group_secret_id: Default::default(),
        nonce,
        ciphertext: vec![0xAB; ciphertext_len],
    }
}

/// Encode a header carrying `ext` and decode it again; Err is the decode error.
fn roundtrip<E>(ext: E) -> Result<(), String>
where
    E: p2panda_core::Extensions,
{
    let key = SigningKey::generate();
    let header = Header::<E>::builder().build(&key, ext);
    let bytes = header.encode();
    Header::<E>::decode(&bytes).map(|_| ()).map_err(|e| e.to_string())
}

#[test]
fn a_plain_byte_vector_of_513_elements_cannot_be_read_back() {
    let ok = roundtrip(PlainVec { data: vec![1; 512] });
    let bad = roundtrip(PlainVec { data: vec![1; 513] });
    eprintln!("Vec<u8> of 512 elements: {ok:?}");
    eprintln!("Vec<u8> of 513 elements: {bad:?}");
    assert!(ok.is_ok(), "512 elements are within the limit: {ok:?}");
    assert!(bad.is_err(), "513 elements should exceed the limit");
}

#[test]
fn a_byte_string_longer_than_512_bytes_cannot_be_read_back() {
    let ok = roundtrip(PlainBytes { data: ByteBuf::from(vec![1; 512]) });
    let bad = roundtrip(PlainBytes { data: ByteBuf::from(vec![1; 513]) });
    eprintln!("byte string of 512: {ok:?}");
    eprintln!("byte string of 513: {bad:?}");
    assert!(ok.is_ok(), "{ok:?}");
    assert!(bad.is_err());
}

/// The p2panda-spaces application message as p2panda-spaces' own test utilities carry it (the
/// extension IS the `SpacesArgs`): fine for a short ciphertext, unreadable for a longer one.
#[test]
fn spaces_args_as_a_plain_extension_fail_past_a_few_hundred_bytes_of_ciphertext() {
    let mut results = Vec::new();
    for n in [100usize, 300, 450, 500, 512, 600, 2000] {
        let r = roundtrip(application(n));
        eprintln!("SpacesArgs::Application, ciphertext of {n} bytes as the plain extension: {r:?}");
        results.push((n, r.is_ok()));
    }
    assert!(results.first().unwrap().1, "a short ciphertext reads back");
    assert!(!results.last().unwrap().1, "a long ciphertext must not read back");
}

#[test]
fn the_chunked_extension_carries_what_the_plain_one_cannot() {
    for n in [100usize, 600, 2000, 34_000, 100_000] {
        let ext = DcpExt {
            args: application(n),
            ts_us: 1,
        };
        let r = roundtrip(ext.clone());
        eprintln!("chunked extension, ciphertext of {n} bytes: {r:?}");
        assert!(r.is_ok(), "{n}: {r:?}");
        // And it comes back identical.
        let key = SigningKey::generate();
        let header = Header::<DcpExt>::builder().build(&key, ext);
        let back = Header::<DcpExt>::decode(&header.encode()).unwrap();
        match back.extensions.args {
            SpacesArgs::Application { ciphertext, .. } => assert_eq!(ciphertext.len(), n),
            _ => panic!("wrong variant"),
        }
    }
}

#[test]
fn the_chunked_extension_has_its_own_ceiling() {
    // CHUNK * MAX_CHUNKS bytes of CBOR fit; `Vec<u8>` is encoded by cbor-core as an array of
    // integers (one or two bytes each), so the ciphertext ceiling is lower than that.
    let mut fits = 0usize;
    let mut fails_at = None;
    for n in [50_000usize, 100_000, 150_000, 200_000, 250_000, 300_000] {
        let ext = DcpExt {
            args: application(n),
            ts_us: 1,
        };
        let cbor = ext.cbor_len().expect("encodes");
        let key = SigningKey::generate();
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Header::<DcpExt>::builder().build(&key, ext.clone())
        }));
        eprintln!(
            "ciphertext {n} bytes -> {cbor} bytes of CBOR -> {} chunks (limit {MAX_CHUNKS} of {CHUNK}): {}",
            cbor.div_ceil(CHUNK),
            if attempt.is_ok() { "builds" } else { "panics in Header::build" }
        );
        if cbor.div_ceil(CHUNK) <= MAX_CHUNKS {
            fits = n;
        } else if fails_at.is_none() {
            fails_at = Some(n);
        }
    }
    assert!(fits >= 100_000, "an inline update (34 KB) and its envelope must fit: fits up to {fits}");
    assert!(fails_at.is_some(), "the ceiling must be reachable in this test");
}
