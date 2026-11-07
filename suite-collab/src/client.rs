// SPDX-License-Identifier: GPL-3.0-or-later
//! The shell's side of the conversation: [`SessionClient`], and
//! [`ProcessClient`], which speaks it over any byte stream pair (a child
//! process's stdin and stdout, a pipe, a test buffer).
//!
//! The GTK shells do not use [`ProcessClient::recv`]: they read lines with the
//! main loop's `DataInputStream::read_line_async` and hand each line to
//! [`crate::decode_message`], so no thread blocks. [`ProcessClient`] is for
//! tests, tools and anything with a thread to spare; both go through the same
//! codec.

use std::io::{self, BufReader, Read, Write};

use crate::codec::{
    decode_message, encode_request, write_line, DecodeError, Line, LineReader, MAX_LINE_BYTES,
};
use crate::protocol::{
    Binding, Command, ErrorCode, Event, Message, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION,
};

#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    /// The sidecar wrote a line this client cannot decode (the line, cut to
    /// 200 characters, is kept for the log). `recv` can be called again: the
    /// next call reads the next line.
    Decode(DecodeError, String),
    /// The sidecar wrote a line over this client's limit; it was skipped.
    LineTooLong { limit: usize },
    NotUtf8,
    /// The command would not fit in a line the sidecar will read; nothing was written.
    TooLargeToSend { bytes: usize, limit: usize },
    /// The sidecar ended the conversation before the handshake finished.
    Closed,
    /// `hello` failed: no version in common, or the sidecar said something else.
    Handshake(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "i/o: {e}"),
            ClientError::Decode(e, line) => write!(f, "{e} (line: {line})"),
            ClientError::LineTooLong { limit } => write!(f, "a line over {limit} bytes was skipped"),
            ClientError::NotUtf8 => write!(f, "a line was not UTF-8"),
            ClientError::TooLargeToSend { bytes, limit } => {
                write!(f, "a command of {bytes} bytes is over the sidecar's limit of {limit}")
            }
            ClientError::Closed => write!(f, "the sidecar closed the connection"),
            ClientError::Handshake(m) => write!(f, "handshake failed: {m}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        ClientError::Io(e)
    }
}

/// What the sidecar said about itself in answer to `hello`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiated {
    pub protocol: u32,
    pub server: String,
    pub bindings: Vec<Binding>,
    pub max_line: usize,
}

/// One conversation with a sidecar. Synchronous; `send` never waits for the answer.
pub trait SessionClient {
    /// Write `command` as one line and return the `req` number it carries; the
    /// event that answers it carries the same number.
    fn send(&mut self, command: &Command) -> Result<u64, ClientError>;

    /// The next line the sidecar wrote, decoded. `Ok(None)` when it closed its
    /// output. Blocks until a line or the end.
    fn recv(&mut self) -> Result<Option<Message>, ClientError>;
}

/// A [`SessionClient`] over a reader and a writer.
pub struct ProcessClient<R, W> {
    lines: LineReader<BufReader<R>>,
    out: W,
    next_req: u64,
    max_send: usize,
}

impl<R: Read, W: Write> ProcessClient<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self::with_limit(reader, writer, MAX_LINE_BYTES)
    }

    /// `limit`: the longest line this client will read.
    pub fn with_limit(reader: R, writer: W, limit: usize) -> Self {
        ProcessClient {
            lines: LineReader::with_limit(BufReader::new(reader), limit),
            out: writer,
            next_req: 1,
            max_send: MAX_LINE_BYTES,
        }
    }

    /// Open the conversation: send `hello` naming the versions this crate
    /// speaks and read the answer. Must be the first call.
    pub fn handshake(&mut self, client: &str) -> Result<Negotiated, ClientError> {
        self.send(&Command::Hello {
            protocol: PROTOCOL_VERSION,
            min_protocol: Some(MIN_PROTOCOL_VERSION),
            client: client.to_owned(),
        })?;
        let Some(m) = self.recv()? else {
            return Err(ClientError::Closed);
        };
        match m.event {
            Event::Hello { protocol, server, bindings, max_line } => {
                if !(MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&protocol) {
                    return Err(ClientError::Handshake(format!(
                        "the sidecar chose protocol {protocol}, this client speaks {MIN_PROTOCOL_VERSION} to {PROTOCOL_VERSION}"
                    )));
                }
                self.max_send = max_line.min(MAX_LINE_BYTES);
                Ok(Negotiated { protocol, server, bindings, max_line })
            }
            Event::Error { code: ErrorCode::UnsupportedProtocol, message } => {
                Err(ClientError::Handshake(message))
            }
            other => Err(ClientError::Handshake(format!(
                "expected hello, the sidecar sent {other:?}"
            ))),
        }
    }

    /// The writer, for a caller that owns the pipe and wants to close it.
    pub fn into_writer(self) -> W {
        self.out
    }
}

impl<R: Read, W: Write> SessionClient for ProcessClient<R, W> {
    fn send(&mut self, command: &Command) -> Result<u64, ClientError> {
        let req = self.next_req;
        let line = encode_request(Some(req), command);
        if line.len() > self.max_send {
            return Err(ClientError::TooLargeToSend { bytes: line.len(), limit: self.max_send });
        }
        write_line(&mut self.out, &line)?;
        self.next_req += 1;
        Ok(req)
    }

    fn recv(&mut self) -> Result<Option<Message>, ClientError> {
        match self.lines.read_line()? {
            Line::Eof => Ok(None),
            Line::TooLong { limit } => Err(ClientError::LineTooLong { limit }),
            Line::NotUtf8 => Err(ClientError::NotUtf8),
            Line::Text(t) => decode_message(&t)
                .map(Some)
                .map_err(|e| ClientError::Decode(e, t.chars().take(200).collect())),
        }
    }
}

impl<R, W> std::fmt::Debug for ProcessClient<R, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessClient").field("next_req", &self.next_req).finish()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::codec::{decode_request, encode_message};
    use crate::protocol::{ConnState, Request};

    fn sidecar_says(events: &[(Option<u64>, Event)]) -> Cursor<Vec<u8>> {
        let mut text = String::new();
        for (req, e) in events {
            text.push_str(&encode_message(*req, e));
            text.push('\n');
        }
        Cursor::new(text.into_bytes())
    }

    fn hello(protocol: u32) -> Event {
        Event::Hello {
            protocol,
            server: "collabd test".into(),
            bindings: vec![Binding::Streams],
            max_line: 1000,
        }
    }

    fn written(client: ProcessClient<Cursor<Vec<u8>>, Vec<u8>>) -> Vec<Request> {
        String::from_utf8(client.into_writer())
            .unwrap()
            .lines()
            .map(|l| decode_request(l).unwrap())
            .collect()
    }

    #[test]
    fn the_handshake_sends_hello_with_the_versions_it_speaks() {
        let mut c = ProcessClient::new(sidecar_says(&[(Some(1), hello(1))]), Vec::new());
        let n = c.handshake("tables 0.1").unwrap();
        assert_eq!(n.protocol, 1);
        assert_eq!(n.bindings, vec![Binding::Streams]);
        assert_eq!(n.max_line, 1000);
        assert_eq!(
            written(c),
            vec![Request {
                req: Some(1),
                command: Command::Hello {
                    protocol: PROTOCOL_VERSION,
                    min_protocol: Some(MIN_PROTOCOL_VERSION),
                    client: "tables 0.1".into()
                }
            }]
        );
    }

    #[test]
    fn a_sidecar_that_speaks_no_common_version_fails_the_handshake() {
        let refusal = Event::error(ErrorCode::UnsupportedProtocol, "this sidecar speaks 7 to 9");
        let mut c = ProcessClient::new(sidecar_says(&[(Some(1), refusal)]), Vec::new());
        match c.handshake("x") {
            Err(ClientError::Handshake(m)) => assert!(m.contains("7 to 9"), "{m}"),
            other => panic!("{other:?}"),
        }
        // A sidecar that picks a version outside the range this client named is broken.
        let mut c = ProcessClient::new(sidecar_says(&[(Some(1), hello(9))]), Vec::new());
        assert!(matches!(c.handshake("x"), Err(ClientError::Handshake(_))));
        let mut c = ProcessClient::new(sidecar_says(&[(Some(1), hello(0))]), Vec::new());
        assert!(matches!(c.handshake("x"), Err(ClientError::Handshake(_))));
    }

    #[test]
    fn a_sidecar_that_answers_something_else_or_nothing_fails_the_handshake() {
        let other = Event::State { doc: None, state: ConnState::Online };
        let mut c = ProcessClient::new(sidecar_says(&[(None, other)]), Vec::new());
        assert!(matches!(c.handshake("x"), Err(ClientError::Handshake(_))));
        let mut c = ProcessClient::new(Cursor::new(Vec::new()), Vec::new());
        assert!(matches!(c.handshake("x"), Err(ClientError::Closed)));
    }

    #[test]
    fn requests_are_numbered_from_one_and_the_numbers_are_on_the_wire() {
        let mut c = ProcessClient::new(Cursor::new(Vec::new()), Vec::new());
        let a = c.send(&Command::Leave { doc: "a".into() }).unwrap();
        let b = c.send(&Command::Leave { doc: "b".into() }).unwrap();
        assert_eq!((a, b), (1, 2));
        let reqs: Vec<_> = written(c).iter().map(|r| r.req).collect();
        assert_eq!(reqs, [Some(1), Some(2)]);
    }

    #[test]
    fn recv_returns_events_in_order_then_none() {
        let events = [
            (None, Event::State { doc: None, state: ConnState::Connecting }),
            (Some(4), Event::error(ErrorCode::Forbidden, "viewer")),
        ];
        let mut c = ProcessClient::new(sidecar_says(&events), Vec::new());
        let m = c.recv().unwrap().unwrap();
        assert_eq!(m.event, events[0].1);
        let m = c.recv().unwrap().unwrap();
        assert_eq!((m.req, m.event), (Some(4), events[1].1.clone()));
        assert!(c.recv().unwrap().is_none());
    }

    #[test]
    fn a_bad_line_is_an_error_and_the_next_line_is_still_read() {
        let mut wire = Vec::new();
        wire.extend_from_slice(b"this is not json\n");
        wire.extend_from_slice(b"{\"event\":\"from_the_future\"}\n");
        wire.extend_from_slice(b"\xff\xfe\n");
        wire.extend_from_slice(encode_message(None, &hello(1)).as_bytes());
        wire.push(b'\n');
        let mut c = ProcessClient::new(Cursor::new(wire), Vec::new());
        assert!(matches!(c.recv(), Err(ClientError::Decode(DecodeError::Malformed(_), _))));
        assert!(matches!(c.recv(), Err(ClientError::Decode(DecodeError::Unknown(n), _)) if n == "from_the_future"));
        assert!(matches!(c.recv(), Err(ClientError::NotUtf8)));
        assert!(matches!(c.recv(), Ok(Some(_))));
    }

    #[test]
    fn an_oversized_line_from_the_sidecar_is_skipped() {
        let mut wire = vec![b'x'; 5000];
        wire.push(b'\n');
        wire.extend_from_slice(encode_message(None, &hello(1)).as_bytes());
        wire.push(b'\n');
        let mut c = ProcessClient::with_limit(Cursor::new(wire), Vec::new(), 1000);
        assert!(matches!(c.recv(), Err(ClientError::LineTooLong { limit: 1000 })));
        assert!(matches!(c.recv(), Ok(Some(_))));
    }

    #[test]
    fn a_command_over_the_sidecars_limit_is_refused_before_anything_is_written() {
        let mut c = ProcessClient::new(sidecar_says(&[(Some(1), hello(1))]), Vec::new());
        c.handshake("x").unwrap(); // the sidecar's max_line is 1000
        let big = Command::PublishUpdate { doc: "d".into(), bytes: vec![0; 2000] };
        assert!(matches!(c.send(&big), Err(ClientError::TooLargeToSend { limit: 1000, .. })));
        let next = c.send(&Command::Leave { doc: "d".into() }).unwrap();
        assert_eq!(next, 2, "the refused command did not use a request number");
        let sent = written(c);
        assert_eq!(sent.len(), 2, "hello and leave only: {sent:?}");
    }
}
