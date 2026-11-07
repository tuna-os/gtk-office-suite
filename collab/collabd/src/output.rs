//! The single writer of stdout.
//!
//! Every task holds a clone of [`Output`] and sends structured messages; one
//! writer drains them, so lines never interleave and the protocol stays one
//! object per line. A test reads the channel instead of a pipe.

use suite_collab::{Event, Message};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Output(mpsc::UnboundedSender<Message>);

impl Output {
    pub fn channel() -> (Output, mpsc::UnboundedReceiver<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Output(tx), rx)
    }

    /// `req` is the shell's request number when this event answers a command.
    pub fn send(&self, req: Option<u64>, event: Event) {
        // The receiver is gone only when the process is shutting down.
        let _ = self.0.send(Message { req, event });
    }
}

/// Drain `rx` to `out` as JSON lines until every [`Output`] is dropped. Blocking
/// writes: run it with `spawn_blocking`.
pub fn write_all<W: std::io::Write>(
    mut rx: mpsc::UnboundedReceiver<Message>,
    out: &mut W,
) -> std::io::Result<()> {
    while let Some(m) = rx.blocking_recv() {
        suite_collab::write_line(out, &suite_collab::encode_message(m.req, &m.event))?;
    }
    Ok(())
}
