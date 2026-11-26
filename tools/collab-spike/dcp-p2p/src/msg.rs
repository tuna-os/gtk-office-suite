//! The message type p2panda-spaces is generic over, and the forge that signs and stores it.

use std::borrow::Borrow;
use std::sync::{Arc, Mutex};

use p2panda_core::traits::{Digest, Provenance};
use p2panda_core::{AnyOperation, Hash, Header, Operation, SeqNum, SigningKey, VerifyingKey};
use p2panda_spaces::{Forge, SpacesArgs};
use p2panda_store::logs::LogStore;
use p2panda_store::operations::OperationStore;
use p2panda_store::{SqliteError, SqliteStore};

use crate::clock::Clock;
use crate::ext::{DcpExt, MAX_CHUNKS};

/// A p2panda operation seen as a spaces message. `Operation` and `Borrow` are both foreign, so the
/// orphan rule needs this local wrapper (p2panda-spaces' own tests use its local `SpacesArgs`).
#[derive(Clone, Debug)]
pub struct DcpMsg {
    pub op: Operation<DcpExt>,
}

impl Borrow<SpacesArgs<()>> for DcpMsg {
    fn borrow(&self) -> &SpacesArgs<()> {
        &self.op.header.extensions.args
    }
}

/// p2panda-spaces 0.8 reads messages back from the store (`SpacesMessageStore`), which needs this.
impl From<Operation<DcpExt>> for DcpMsg {
    fn from(op: Operation<DcpExt>) -> Self {
        DcpMsg { op }
    }
}

impl Provenance<VerifyingKey> for DcpMsg {
    fn author(&self) -> VerifyingKey {
        self.op.header.verifying_key
    }

    /// The signature was checked when the header was decoded (`Header::decode`), and the replica
    /// calls `validate_operation` before it hands a message to the manager.
    fn verify(&self) -> bool {
        true
    }
}

impl Digest<Hash> for DcpMsg {
    fn hash(&self) -> Hash {
        self.op.hash
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error(transparent)]
    Sqlite(#[from] SqliteError),
    #[error("the forge was used outside a document context")]
    NoContext,
    #[error("{0}")]
    TooLarge(String),
}

/// Signs operations with the principal's key and appends them to the author's log of the
/// document named by the context (the log id is the document id). The caller sets the context
/// before it calls the manager; the replica serialises all manager calls, so one context is enough.
#[derive(Clone)]
pub struct DcpForge {
    signing_key: Arc<SigningKey>,
    store: SqliteStore,
    ctx: Arc<Mutex<Option<String>>>,
    clock: Arc<dyn Clock>,
}

pub type LogId = String;

/// The log that holds only the author's key bundle (the "contact card").
pub const KEYS_LOG: &str = "keys";

impl DcpForge {
    pub fn new(signing_key: SigningKey, store: SqliteStore, clock: Arc<dyn Clock>) -> Self {
        DcpForge {
            signing_key: Arc::new(signing_key),
            store,
            ctx: Arc::new(Mutex::new(None)),
            clock,
        }
    }

    pub fn set_context(&self, log: &str) {
        *self.ctx.lock().expect("forge context lock") = Some(log.to_owned());
    }

    pub fn clear_context(&self) {
        *self.ctx.lock().expect("forge context lock") = None;
    }

    /// Sign an operation with an arbitrary extension, as a hostile member would (tests only use
    /// this through `P2pStore::forge_version`).
    pub async fn append(&self, log: &str, args: SpacesArgs<()>) -> Result<Operation<DcpExt>, ForgeError> {
        let ext = DcpExt {
            args,
            ts_us: self.clock.now_us(),
        };
        let chunks = ext
            .cbor_len()
            .map_err(ForgeError::TooLarge)?
            .div_ceil(crate::ext::CHUNK);
        if chunks > MAX_CHUNKS {
            return Err(ForgeError::TooLarge(format!(
                "an entry of {chunks} chunks does not fit in an operation header (at most {MAX_CHUNKS})"
            )));
        }
        let me = self.signing_key.verifying_key();
        let log = log.to_owned();
        // p2panda-spaces 0.8 calls the forge inside its own state transaction on this same store
        // (`StateTransaction`, "make sure the forge itself is running within the same
        // transaction"), and commits or rolls back with the state. So no transaction here: a
        // second `begin` would wait on the first for ever.
        let latest = <SqliteStore as LogStore<AnyOperation, VerifyingKey, LogId, SeqNum, Hash>>::get_latest_entry_tx(
            &self.store, &me, &log,
        )
        .await?;
        let (seq_num, backlink) = latest
            .map(|o| (o.header.seq_num + 1, Some(o.hash)))
            .unwrap_or((0, None));
        let header = Header::<DcpExt>::builder()
            .seq_num(seq_num)
            .backlink(backlink)
            .build(&*self.signing_key, ext);
        let op = Operation::from_parts(header, None);
        self.store.insert_operation_tx(&op.hash, &op, &log).await?;
        Ok(op)
    }
}

impl Forge<()> for DcpForge {
    type Message = DcpMsg;
    type Error = ForgeError;

    fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    async fn forge(&self, args: SpacesArgs<()>) -> Result<DcpMsg, ForgeError> {
        let log = self
            .ctx
            .lock()
            .expect("forge context lock")
            .clone()
            .ok_or(ForgeError::NoContext)?;
        let op = self.append(&log, args).await?;
        Ok(DcpMsg { op })
    }
}
