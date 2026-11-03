//! Length-prefixed postcard frames over a byte stream.
//!
//! This is p2panda-net 0.7.1's `codec.rs` (MIT OR Apache-2.0, https://github.com/p2panda/p2panda)
//! reduced to what the sync sessions need. It is copied, not depended on, because p2panda-net's
//! other modules (actors, discovery, gossip) are not used by this binding.

use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;
use tokio_util::bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

#[derive(Debug)]
pub struct Codec<M> {
    max_frame_len: usize,
    _phantom: PhantomData<M>,
}

impl<M> Codec<M> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<M> Default for Codec<M> {
    fn default() -> Self {
        Self {
            max_frame_len: 64 * 1024 * 1024,
            _phantom: PhantomData,
        }
    }
}

#[derive(Debug, Error)]
pub enum CodecError {
    #[error(transparent)]
    Postcard(#[from] postcard::Error),
    #[error("frame of {0} bytes exceeds the limit of {1}")]
    TooLarge(usize, usize),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl<M: Serialize> Encoder<M> for Codec<M> {
    type Error = CodecError;

    fn encode(&mut self, item: M, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = postcard::to_allocvec(&item)?;
        if bytes.len() > self.max_frame_len {
            return Err(CodecError::TooLarge(bytes.len(), self.max_frame_len));
        }
        dst.reserve(4 + bytes.len());
        dst.put_u32(bytes.len() as u32);
        dst.put_slice(&bytes);
        Ok(())
    }
}

impl<M: DeserializeOwned> Decoder for Codec<M> {
    type Item = M;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;
        if len > self.max_frame_len {
            return Err(CodecError::TooLarge(len, self.max_frame_len));
        }
        if src.len() < 4 + len {
            src.reserve(4 + len - src.len());
            return Ok(None);
        }
        let item: M = postcard::from_bytes(&src[4..4 + len])?;
        src.advance(4 + len);
        Ok(Some(item))
    }
}
