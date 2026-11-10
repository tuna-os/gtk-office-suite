// SPDX-License-Identifier: GPL-3.0-or-later

//! Command line and access token parsing for the `join_call` example.
//!
//! This lives outside `join_call.rs` so that `tests/join_call_args.rs` can
//! compile and test it without the `livekit` feature the example requires.
//! Cargo does not treat `examples/support/` as an example target.
//!
//! The access token is never accepted as a command line argument: argv is
//! visible to other local users through the process list and ends up in shell
//! history. It is read from stdin or from a file instead.

use std::{io::Read, path::PathBuf};

use zeroize::Zeroizing;

pub const USAGE: &str = "usage: join_call <homeserver> <user_id> <device_id> <room_id> \
     [--access-token-file PATH] [--focus JWT_SERVICE_URL] \
     [--assert-peer USER_ID:DEVICE_ID] [--assert-timeout SECS]\n\
     The access token is read from stdin, or from PATH if --access-token-file \
     is given (`-` means stdin).";

/// Where to read the access token from.
#[derive(Debug, PartialEq, Eq)]
pub enum AccessTokenSource {
    Stdin,
    File(PathBuf),
}

/// Command line options after the four positional arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    /// Preferred LiveKit JWT service URL to advertise as focus.
    pub focus: Option<String>,
    /// Assertion mode: the `{user_id}:{device_id}` of the expected peer.
    pub assert_peer: Option<String>,
    /// Assertion timeout in seconds.
    pub assert_timeout: u64,
}

/// The parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub homeserver: String,
    pub user_id: String,
    pub device_id: String,
    pub room_id: String,
    pub access_token_source: AccessTokenSource,
    pub options: Options,
}

/// Parse the command line, without the program name.
///
/// Error messages never echo positional arguments: when the legacy form with
/// the access token in argv is used, one of them is the token.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut positional = Vec::new();
    let mut access_token_source = AccessTokenSource::Stdin;
    let mut options = Options {
        focus: None,
        assert_peer: None,
        assert_timeout: 90,
    };

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--access-token-file" => {
                let path = iter
                    .next()
                    .ok_or("--access-token-file needs a PATH (or `-` for stdin)")?;
                access_token_source = if path == "-" {
                    AccessTokenSource::Stdin
                } else {
                    AccessTokenSource::File(path.into())
                };
            }
            "--focus" => options.focus = Some(iter.next().ok_or("--focus needs a URL")?),
            "--assert-peer" => {
                options.assert_peer = Some(
                    iter.next()
                        .ok_or("--assert-peer needs a USER_ID:DEVICE_ID")?,
                );
            }
            "--assert-timeout" => {
                options.assert_timeout = iter
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--assert-timeout needs a number of seconds")?;
            }
            other if other.starts_with("--") => {
                // Do not echo an `--option=value` value: it could be a secret.
                let name = other.split('=').next().unwrap_or(other);
                return Err(format!("unknown option: {name}"));
            }
            _ => positional.push(arg),
        }
    }

    let count = positional.len();
    let [homeserver, user_id, device_id, room_id]: [String; 4] =
        positional.try_into().map_err(|_| {
            if count > 4 {
                format!(
                    "expected 4 positional arguments, got {count}: the access token is no longer \
                     accepted on the command line, where other local users and shell history can \
                     see it. Pass it on stdin or with --access-token-file PATH. If you passed a \
                     real access token, consider it exposed and revoke it."
                )
            } else {
                format!("expected 4 positional arguments, got {count}")
            }
        })?;
    if !room_id.starts_with('!') {
        return Err(
            "the fourth positional argument must be a room ID starting with `!`".to_owned(),
        );
    }

    Ok(Args {
        homeserver,
        user_id,
        device_id,
        room_id,
        access_token_source,
        options,
    })
}

/// Read the access token from `input`.
///
/// The whole input, trimmed, must be a single token without whitespace. The
/// input buffer is zeroized when it is dropped; the returned token is too.
pub fn read_access_token(mut input: impl Read) -> Result<Zeroizing<String>, String> {
    // Reserve up front so that reading does not reallocate and leave copies
    // of the token in freed memory.
    let mut raw = Zeroizing::new(String::with_capacity(8192));
    input
        .read_to_string(&mut raw)
        .map_err(|error| format!("cannot read the access token: {error}"))?;
    let token = raw.trim();
    if token.is_empty() {
        return Err("no access token given (empty input)".to_owned());
    }
    if token.contains(char::is_whitespace) {
        return Err("the access token input must be a single token on one line".to_owned());
    }
    Ok(Zeroizing::new(token.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|&arg| arg.to_owned()))
    }

    #[test]
    fn parses_token_free_command_line() {
        assert_eq!(
            args(&[
                "https://matrix.example.org",
                "@alice:example.org",
                "DEVICEID",
                "!room:example.org",
                "--focus",
                "http://jwt.example.org",
                "--assert-peer",
                "@bob:example.org:BOBDEVICE",
                "--assert-timeout",
                "30",
            ]),
            Ok(Args {
                homeserver: "https://matrix.example.org".to_owned(),
                user_id: "@alice:example.org".to_owned(),
                device_id: "DEVICEID".to_owned(),
                room_id: "!room:example.org".to_owned(),
                access_token_source: AccessTokenSource::Stdin,
                options: Options {
                    focus: Some("http://jwt.example.org".to_owned()),
                    assert_peer: Some("@bob:example.org:BOBDEVICE".to_owned()),
                    assert_timeout: 30,
                },
            })
        );
    }

    #[test]
    fn access_token_file_flag() {
        let with_file = |path: &str| {
            args(&[
                "https://hs",
                "@a:hs",
                "DEV",
                "!r:hs",
                "--access-token-file",
                path,
            ])
            .map(|parsed| parsed.access_token_source)
        };
        assert_eq!(
            with_file("/run/secrets/token"),
            Ok(AccessTokenSource::File("/run/secrets/token".into()))
        );
        assert_eq!(with_file("-"), Ok(AccessTokenSource::Stdin));
        assert!(args(&["https://hs", "@a:hs", "DEV", "!r:hs", "--access-token-file"]).is_err());
    }

    #[test]
    fn rejects_legacy_access_token_positional() {
        let secret = "syt_c2VjcmV0_legacyTokenValue";
        let error = args(&[
            "https://hs",
            "@a:hs",
            "DEV",
            secret,
            "!r:hs",
            "--focus",
            "http://jwt",
        ])
        .unwrap_err();
        assert!(
            error.contains("access token is no longer accepted"),
            "{error}"
        );
        assert!(!error.contains(secret), "error leaks the token: {error}");

        // The legacy form with the room ID forgotten puts the token where the
        // room ID belongs.
        let error = args(&["https://hs", "@a:hs", "DEV", secret]).unwrap_err();
        assert!(error.contains("room ID"), "{error}");
        assert!(!error.contains(secret), "error leaks the token: {error}");
    }

    #[test]
    fn unknown_option_does_not_echo_value() {
        let error = args(&["--access-token=syt_secret"]).unwrap_err();
        assert_eq!(error, "unknown option: --access-token");
    }

    #[test]
    fn usage_does_not_advertise_argv_token() {
        assert!(USAGE.contains("--access-token-file"));
        assert!(!USAGE.contains("<access_token>"));
    }

    #[test]
    fn reads_trimmed_token() {
        let token = read_access_token(&b"  syt_abc_123\n"[..]).unwrap();
        assert_eq!(token.as_str(), "syt_abc_123");
    }

    #[test]
    fn rejects_empty_or_multi_token_input() {
        assert!(read_access_token(&b"\n"[..]).is_err());
        assert!(read_access_token(&b"syt_abc\nsyt_def\n"[..]).is_err());
    }
}
