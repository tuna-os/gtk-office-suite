// SPDX-License-Identifier: GPL-3.0-or-later
//! The JSON-lines codec: one JSON object per line, `\n` ends it, no object
//! contains a raw newline (serde_json escapes them), blank lines are ignored.
//!
//! Decoding is split from reading so a GTK shell that reads with
//! `DataInputStream::read_line_async` can call [`decode_message`] on the line
//! it got, while a process-backed client and the sidecar use [`LineReader`] over
//! any [`BufRead`]. Nothing here panics on hostile input and nothing here
//! allocates more than the line limit plus one buffer.

use std::io::{self, BufRead, Write};

use serde_json::Value;

use crate::protocol::{Command, Event, Message, Request, COMMAND_NAMES, EVENT_NAMES};

/// The longest line either end reads, in bytes, newline excluded. A snapshot of
/// a 100,000-cell workbook is about 2.9 MB (RFC-0005 3.7), which is about 3.9 MB
/// as base64; a saved file is smaller. 64 MiB leaves room for a large workbook
/// and bounds what a broken or hostile peer can make the other end allocate.
pub const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

/// Why a line is not a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Not JSON, not an object, a field of the wrong type, or base64 that is not base64.
    Malformed(String),
    /// An object with no `cmd` / `event` string.
    Untagged,
    /// A well-formed object naming a command (or event) this version does not have.
    Unknown(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Malformed(m) => write!(f, "malformed line: {m}"),
            DecodeError::Untagged => write!(f, "the object has no \"cmd\" or \"event\" string"),
            DecodeError::Unknown(n) => write!(f, "unknown command or event {n:?}"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn decode_tagged<T: serde::de::DeserializeOwned>(
    line: &str,
    tag: &str,
    known: &[&str],
) -> Result<T, DecodeError> {
    let value: Value =
        serde_json::from_str(line).map_err(|e| DecodeError::Malformed(e.to_string()))?;
    let Value::Object(map) = &value else {
        return Err(DecodeError::Malformed("the line is not a JSON object".into()));
    };
    match map.get(tag) {
        Some(Value::String(name)) if known.contains(&name.as_str()) => {}
        Some(Value::String(name)) => return Err(DecodeError::Unknown(name.clone())),
        _ => return Err(DecodeError::Untagged),
    }
    serde_json::from_value(value).map_err(|e| DecodeError::Malformed(e.to_string()))
}

/// One line the shell wrote.
pub fn decode_request(line: &str) -> Result<Request, DecodeError> {
    decode_tagged(line, "cmd", COMMAND_NAMES)
}

/// One line the sidecar wrote.
pub fn decode_message(line: &str) -> Result<Message, DecodeError> {
    decode_tagged(line, "event", EVENT_NAMES)
}

/// `request` as a line, without its newline.
pub fn encode_request(req: Option<u64>, command: &Command) -> String {
    serde_json::to_string(&Request {
        req,
        command: command.clone(),
    })
    .expect("a command serialises")
}

/// `event` as a line, without its newline.
pub fn encode_message(req: Option<u64>, event: &Event) -> String {
    serde_json::to_string(&Message {
        req,
        event: event.clone(),
    })
    .expect("an event serialises")
}

/// Write `line` and the newline that ends it, and flush: a line the peer has
/// not been handed is a line it cannot answer.
pub fn write_line<W: Write>(out: &mut W, line: &str) -> io::Result<()> {
    debug_assert!(!line.contains('\n'), "a JSON line holds no raw newline");
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

/// What [`LineReader::read_line`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Text(String),
    /// The line was over the limit. All of it, up to its newline, has been
    /// read and thrown away; the next call starts at the next line.
    TooLong { limit: usize },
    /// The line was not UTF-8; it is consumed.
    NotUtf8,
    /// The stream ended. A last line with no newline is returned first.
    Eof,
}

/// Reads newline-ended lines with a bound, and survives a line that breaks it.
pub struct LineReader<R> {
    inner: R,
    limit: usize,
}

impl<R: BufRead> LineReader<R> {
    pub fn new(inner: R) -> Self {
        Self::with_limit(inner, MAX_LINE_BYTES)
    }

    pub fn with_limit(inner: R, limit: usize) -> Self {
        LineReader { inner, limit }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    /// The next non-blank line. Blank lines (and a lone `\r`) are skipped.
    pub fn read_line(&mut self) -> io::Result<Line> {
        loop {
            let mut buf: Vec<u8> = Vec::new();
            let mut too_long = false;
            let mut saw_any = false;
            loop {
                let chunk = match self.inner.fill_buf() {
                    Ok(c) => c,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };
                if chunk.is_empty() {
                    break;
                }
                saw_any = true;
                let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
                    Some(i) => (i, true),
                    None => (chunk.len(), false),
                };
                if !too_long {
                    if buf.len() + take > self.limit {
                        too_long = true;
                        buf = Vec::new();
                    } else {
                        buf.extend_from_slice(&chunk[..take]);
                    }
                }
                self.inner.consume(take + usize::from(done));
                if done {
                    break;
                }
            }
            if !saw_any {
                return Ok(Line::Eof);
            }
            if too_long {
                return Ok(Line::TooLong { limit: self.limit });
            }
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            if buf.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            return Ok(match String::from_utf8(buf) {
                Ok(s) => Line::Text(s),
                Err(_) => Line::NotUtf8,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::protocol::{
        Author, Binding, ConnState, ConnectConfig, DropKind, ErrorCode, HeadInfo, Person,
        PublishKind, Role,
    };

    fn reader(text: &[u8], limit: usize) -> LineReader<Cursor<Vec<u8>>> {
        LineReader::with_limit(Cursor::new(text.to_vec()), limit)
    }

    fn every_command() -> Vec<Command> {
        vec![
            Command::Hello {
                protocol: 1,
                min_protocol: Some(1),
                client: "tables 0.1".into(),
            },
            Command::Connect {
                binding: Binding::Streams,
                config: ConnectConfig {
                    server: "https://s.example/v1/stream/".into(),
                    ca_file: Some("/etc/ca.pem".into()),
                },
            },
            Command::Share {
                title: "Budget".into(),
                mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
                first_version: vec![0, 1, 2, 255],
                genesis: Some(vec![9; 100]),
                doc_model: "tables/1".into(),
                people: vec![Person {
                    principal: "ed25519:abc".into(),
                    role: Role::Viewer,
                }],
            },
            Command::Share {
                title: "t".into(),
                mime: "m".into(),
                first_version: vec![],
                genesis: None,
                doc_model: "tables/1".into(),
                people: vec![],
            },
            Command::Join { invite: "streams:https://s/#d".into() },
            Command::PublishUpdate { doc: "d".into(), bytes: vec![1, 2, 3] },
            Command::PublishVersion {
                doc: "d".into(),
                bytes: vec![4],
                message: Some("saved".into()),
                resolve: vec!["$a".into(), "$b".into()],
            },
            Command::FetchVersion { doc: "d".into(), id: "$a".into() },
            Command::Applied { doc: "d".into(), id: "u1".into() },
            Command::Rejected { doc: "d".into(), id: "u1".into(), why: "no".into() },
            Command::Resync { doc: "d".into() },
            Command::Leave { doc: "d".into() },
            Command::Grant { doc: "d".into(), principal: "p".into(), role: Role::Editor },
            Command::Revoke { doc: "d".into(), principal: "p".into() },
        ]
    }

    fn author() -> Author {
        Author { principal: "ed25519:p".into(), device: "dev".into() }
    }

    fn every_event() -> Vec<Event> {
        vec![
            Event::Hello {
                protocol: 1,
                server: "collabd 0.0.0".into(),
                bindings: vec![Binding::Streams],
                max_line: MAX_LINE_BYTES,
            },
            Event::Connected {
                binding: Binding::Streams,
                principal: "ed25519:p".into(),
                device: "dev".into(),
            },
            Event::Shared {
                doc: "d".into(),
                invite: "i".into(),
                principal: "p".into(),
                base_version: "$v0".into(),
            },
            Event::Joined {
                doc: "d".into(),
                title: "t".into(),
                mime: "m".into(),
                role: Role::Editor,
                base_version: Some("$v0".into()),
            },
            Event::Joined {
                doc: "d".into(),
                title: "t".into(),
                mime: "m".into(),
                role: Role::Viewer,
                base_version: None,
            },
            Event::State { doc: None, state: ConnState::Connecting },
            Event::State { doc: Some("d".into()), state: ConnState::Offline { since_ms: 7 } },
            Event::State { doc: Some("d".into()), state: ConnState::Error { message: "x".into() } },
            Event::State { doc: None, state: ConnState::AuthRequired },
            Event::State { doc: None, state: ConnState::Online },
            Event::Genesis {
                doc: "d".into(),
                id: "u0".into(),
                base_version: "$v0".into(),
                author: author(),
                bytes: vec![0; 5],
            },
            Event::Update {
                doc: "d".into(),
                id: "u1".into(),
                author: author(),
                ts_ms: 1,
                bytes: vec![7, 7],
            },
            Event::Heads {
                doc: "d".into(),
                heads: vec![
                    HeadInfo {
                        id: "$v0".into(),
                        author: author(),
                        ts_ms: 2,
                        message: None,
                        covers: Some("u9".into()),
                        mine: false,
                        covered: true,
                    },
                    HeadInfo {
                        id: "$v1".into(),
                        author: author(),
                        ts_ms: 3,
                        message: Some("saved".into()),
                        covers: None,
                        mine: true,
                        covered: true,
                    },
                    HeadInfo {
                        id: "$v2".into(),
                        author: author(),
                        ts_ms: 4,
                        message: None,
                        covers: None,
                        mine: false,
                        covered: false,
                    },
                ],
                conflict: true,
            },
            Event::VersionFile {
                doc: "d".into(),
                id: "$v0".into(),
                mime: "m".into(),
                bytes: vec![1],
            },
            Event::Published {
                doc: "d".into(),
                kind: PublishKind::Update,
                id: "u1".into(),
                seq: Some(3),
                parents: vec![],
            },
            Event::Published {
                doc: "d".into(),
                kind: PublishKind::Version,
                id: "$v1".into(),
                seq: None,
                parents: vec!["$v0".into()],
            },
            Event::Dropped {
                doc: "d".into(),
                kind: DropKind::Record,
                id: "r".into(),
                reason: "bad signature".into(),
            },
            Event::Gap { doc: "d".into(), author: author(), expected: 4, found: 6 },
            Event::Done { doc: Some("d".into()) },
            Event::Done { doc: None },
            Event::error(ErrorCode::Forbidden, "viewer"),
        ]
    }

    #[test]
    fn every_command_round_trips_through_a_line() {
        for (i, c) in every_command().into_iter().enumerate() {
            for req in [None, Some(i as u64)] {
                let line = encode_request(req, &c);
                assert!(!line.contains('\n'));
                let back = decode_request(&line).unwrap_or_else(|e| panic!("{line}: {e}"));
                assert_eq!(back, Request { req, command: c.clone() }, "{line}");
            }
        }
    }

    #[test]
    fn every_event_round_trips_through_a_line() {
        for (i, e) in every_event().into_iter().enumerate() {
            for req in [None, Some(i as u64 + 10)] {
                let line = encode_message(req, &e);
                assert!(!line.contains('\n'));
                let back = decode_message(&line).unwrap_or_else(|x| panic!("{line}: {x}"));
                assert_eq!(back, Message { req, event: e.clone() }, "{line}");
            }
        }
    }

    #[test]
    fn the_name_table_matches_the_enums() {
        // A new variant that is not in the table would decode as "unknown".
        let names: Vec<&str> = every_command().iter().map(Command::name).collect();
        for n in COMMAND_NAMES {
            assert!(names.contains(n), "no command {n} in the round-trip list");
        }
        for c in every_command() {
            assert!(COMMAND_NAMES.contains(&c.name()));
        }
        for e in every_event() {
            let line = encode_message(None, &e);
            let v: Value = serde_json::from_str(&line).unwrap();
            assert!(EVENT_NAMES.contains(&v["event"].as_str().unwrap()), "{line}");
        }
    }

    #[test]
    fn the_wire_shape_is_flat_and_bytes_are_base64() {
        let line = encode_request(
            Some(7),
            &Command::PublishUpdate { doc: "d".into(), bytes: b"foobar".to_vec() },
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"req": 7, "cmd": "publish_update", "doc": "d", "bytes": "Zm9vYmFy"})
        );
        let line = encode_message(
            None,
            &Event::State { doc: None, state: ConnState::Offline { since_ms: 5 } },
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"event": "state", "state": {"kind": "offline", "since_ms": 5}})
        );
    }

    #[test]
    fn malformed_lines_are_errors_not_panics() {
        for line in [
            "",
            "not json",
            "{",
            "[]",
            "42",
            "null",
            "\"cmd\"",
            r#"{"cmd": 5}"#,
            r#"{"cmd": "publish_update"}"#,
            r#"{"cmd": "publish_update", "doc": 1, "bytes": "AA=="}"#,
            r#"{"cmd": "publish_update", "doc": "d", "bytes": "not base64!"}"#,
            r#"{"cmd": "publish_update", "doc": "d", "bytes": 12}"#,
            r#"{"cmd": "hello", "protocol": -1, "client": "x"}"#,
            r#"{"cmd": "hello", "protocol": "one", "client": "x"}"#,
            r#"{"cmd": "connect", "binding": "carrier_pigeon", "config": {"server": "s"}}"#,
            r#"{"req": "seven", "cmd": "leave", "doc": "d"}"#,
            "\u{0}",
        ] {
            assert!(
                matches!(
                    decode_request(line),
                    Err(DecodeError::Malformed(_) | DecodeError::Untagged)
                ),
                "{line:?} gave {:?}",
                decode_request(line)
            );
        }
    }

    #[test]
    fn an_unknown_command_is_told_from_a_malformed_one() {
        assert_eq!(
            decode_request(r#"{"cmd": "launch_missiles", "target": "moon"}"#),
            Err(DecodeError::Unknown("launch_missiles".into()))
        );
        assert_eq!(decode_request(r#"{"req": 3}"#), Err(DecodeError::Untagged));
        assert_eq!(
            decode_message(r#"{"event": "fireworks"}"#),
            Err(DecodeError::Unknown("fireworks".into()))
        );
        // A command is not an event, and the other way round.
        assert!(decode_message(r#"{"cmd": "leave", "doc": "d"}"#).is_err());
        assert!(decode_request(r#"{"event": "state", "state": {"kind": "online"}}"#).is_err());
    }

    #[test]
    fn unknown_extra_fields_are_ignored_so_a_newer_peer_can_add_them() {
        let r = decode_request(r#"{"cmd": "leave", "doc": "d", "since": "v2"}"#).unwrap();
        assert_eq!(r.command, Command::Leave { doc: "d".into() });
    }

    #[test]
    fn a_head_without_mine_and_covered_decodes_as_neither_and_both_are_always_written() {
        let old = r#"{"event":"heads","doc":"d","heads":[{"id":"$v","author":{"principal":"p","device":"d"},"ts_ms":1}],"conflict":true}"#;
        let Message { event: Event::Heads { heads, .. }, .. } = decode_message(old).unwrap() else {
            panic!("not heads")
        };
        assert!(!heads[0].mine && !heads[0].covered && heads[0].covers.is_none());
        let line = encode_message(None, &Event::Heads { doc: "d".into(), heads, conflict: false });
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["heads"][0]["mine"], serde_json::json!(false));
        assert_eq!(v["heads"][0]["covered"], serde_json::json!(false));
        assert!(v["heads"][0].get("covers").is_none());
    }

    #[test]
    fn a_deeply_nested_line_is_an_error_not_a_stack_overflow() {
        let line = format!("{}1{}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(matches!(decode_request(&line), Err(DecodeError::Malformed(_))));
    }

    #[test]
    fn lines_are_split_on_newlines_and_blank_lines_are_skipped() {
        let mut r = reader(b"one\n\n  \r\ntwo\r\nthree", 100);
        assert_eq!(r.read_line().unwrap(), Line::Text("one".into()));
        assert_eq!(r.read_line().unwrap(), Line::Text("two".into()));
        assert_eq!(r.read_line().unwrap(), Line::Text("three".into()), "no final newline");
        assert_eq!(r.read_line().unwrap(), Line::Eof);
        assert_eq!(r.read_line().unwrap(), Line::Eof);
    }

    #[test]
    fn an_oversized_line_is_skipped_whole_and_the_next_one_is_read() {
        let mut data = vec![b'x'; 50];
        data.extend_from_slice(b"\nok\n");
        let mut r = reader(&data, 10);
        assert_eq!(r.read_line().unwrap(), Line::TooLong { limit: 10 });
        assert_eq!(r.read_line().unwrap(), Line::Text("ok".into()));
        assert_eq!(r.read_line().unwrap(), Line::Eof);
    }

    #[test]
    fn a_line_of_exactly_the_limit_is_accepted_and_one_more_byte_is_not() {
        let mut r = reader(b"0123456789\n01234567890\nx\n", 10);
        assert_eq!(r.read_line().unwrap(), Line::Text("0123456789".into()));
        assert_eq!(r.read_line().unwrap(), Line::TooLong { limit: 10 });
        assert_eq!(r.read_line().unwrap(), Line::Text("x".into()));
    }

    #[test]
    fn an_oversized_last_line_with_no_newline_ends_the_stream_cleanly() {
        let mut r = reader(&[b'y'; 100], 10);
        assert_eq!(r.read_line().unwrap(), Line::TooLong { limit: 10 });
        assert_eq!(r.read_line().unwrap(), Line::Eof);
    }

    #[test]
    fn an_oversized_line_is_not_buffered() {
        // A one-byte-at-a-time reader makes every fill_buf a one-byte chunk;
        // the limit still holds and nothing larger than it is ever kept.
        struct Drip(Cursor<Vec<u8>>);
        impl io::Read for Drip {
            fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
                let n = 1.min(b.len());
                self.0.read(&mut b[..n])
            }
        }
        let mut data = vec![b'z'; 10_000];
        data.extend_from_slice(b"\nfine\n");
        let mut r = LineReader::with_limit(io::BufReader::with_capacity(1, Drip(Cursor::new(data))), 16);
        assert_eq!(r.read_line().unwrap(), Line::TooLong { limit: 16 });
        assert_eq!(r.read_line().unwrap(), Line::Text("fine".into()));
    }

    #[test]
    fn a_line_that_is_not_utf8_is_consumed_and_reported() {
        let mut r = reader(b"\xff\xfe\nnext\n", 100);
        assert_eq!(r.read_line().unwrap(), Line::NotUtf8);
        assert_eq!(r.read_line().unwrap(), Line::Text("next".into()));
    }

    #[test]
    fn a_large_payload_survives_a_line() {
        let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        let cmd = Command::PublishUpdate { doc: "d".into(), bytes: big.clone() };
        let mut wire = Vec::new();
        write_line(&mut wire, &encode_request(Some(1), &cmd)).unwrap();
        let mut r = LineReader::new(Cursor::new(wire));
        let Line::Text(line) = r.read_line().unwrap() else { panic!("no line") };
        let Command::PublishUpdate { bytes, .. } = decode_request(&line).unwrap().command else {
            panic!("wrong command")
        };
        assert_eq!(bytes, big);
    }
}
