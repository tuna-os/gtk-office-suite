//! Removing an attachment from a message on the server (#289).
//!
//! IMAP and JMAP servers cannot edit a message: the only way to take a file
//! out is to store a copy without it and delete the original. The copy is
//! the original byte for byte except for the one MIME part, which becomes a
//! short note in Thunderbird's format (`text/x-moz-deleted`), so either
//! client recognises what the other removed.

use mail_parser::{MessageParser, MessagePart, MimeHeaders};

use crate::cache::Cache;
use crate::encoding::{base64_encode, base64_lines};
use crate::worker::render::attachments::{attachment_parts, extract_attachments, guess_mime};
use crate::worker::WorkerEvent;

/// Whether a part is the note left where an attachment was removed.
pub(super) fn is_placeholder(part: &MessagePart<'_>) -> bool {
    part.content_type().is_some_and(|c| {
        c.ctype().eq_ignore_ascii_case("text")
            && c.subtype()
                .is_some_and(|s| s.eq_ignore_ascii_case("x-moz-deleted"))
    })
}

/// Why an attachment could not be removed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum StripError {
    /// The message could not be parsed, or no attachment by that name is in it.
    NotFound,
    /// The part sits under a signature or inside encryption: changing it
    /// would break the one, and cannot be done to the other.
    Protected,
    /// The attachment is the whole message (a single-part message), so
    /// there is nothing to keep but the headers.
    WholeMessage,
}

impl StripError {
    pub(super) fn text(&self) -> String {
        use crate::i18n::i18n;
        match self {
            StripError::NotFound => {
                i18n("The attachment is no longer in the message on the server")
            }
            StripError::Protected => {
                i18n("The message is signed or encrypted, and removing a file would break it")
            }
            StripError::WholeMessage => {
                i18n("The attachment is the whole message; delete the message instead")
            }
        }
    }
}

/// `raw` without the attachment called `name`. Where several share the
/// name, the one whose decoded size is nearest `size` goes. `date` is
/// stamped into the note, in the form Thunderbird writes.
pub(super) fn without_attachment(
    raw: &[u8],
    name: &str,
    size: u64,
    date: &str,
) -> Result<Vec<u8>, StripError> {
    if crate::mail::pgp::detect(raw).is_some() {
        return Err(StripError::Protected);
    }
    let parsed = MessageParser::default()
        .parse(raw)
        .ok_or(StripError::NotFound)?;
    let (id, _) = attachment_parts(&parsed, raw)
        .into_iter()
        .filter(|(_, a)| a.name == name)
        .min_by_key(|(_, a)| (a.data.len() as u64).abs_diff(size))
        .ok_or(StripError::NotFound)?;
    if id == 0 {
        return Err(StripError::WholeMessage);
    }
    let part = &parsed.parts[id];
    let (start, end) = (part.offset_header, part.offset_end);
    let protected = parsed.parts.iter().any(|p| {
        p.offset_header <= start
            && end <= p.offset_end
            && p.content_type().is_some_and(|c| {
                let sub = c.subtype().unwrap_or_default();
                (c.ctype().eq_ignore_ascii_case("multipart")
                    && (sub.eq_ignore_ascii_case("signed")
                        || sub.eq_ignore_ascii_case("encrypted")))
                    || (c.ctype().eq_ignore_ascii_case("application")
                        && sub.eq_ignore_ascii_case("pkcs7-mime"))
            })
    });
    if protected {
        return Err(StripError::Protected);
    }
    let headers = raw
        .get(start..part.offset_body)
        .ok_or(StripError::NotFound)?;
    let eol = if raw.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = Vec::with_capacity(raw.len() - (end - start) + 512);
    out.extend_from_slice(&raw[..start]);
    out.extend_from_slice(&placeholder(name, headers, date, eol));
    out.extend_from_slice(&raw[end..]);
    Ok(out)
}

/// The part that takes an attachment's place: Thunderbird's shape, with the
/// original part headers kept in the text as it keeps them.
fn placeholder(name: &str, original_headers: &[u8], date: &str, eol: &str) -> Vec<u8> {
    let label = header_param(&format!("Deleted: {name}"));
    let note = crate::i18n::i18n(
        "You deleted an attachment from this message. The original MIME headers for the attachment were:",
    );
    let mut body = note.into_bytes();
    body.extend_from_slice(eol.as_bytes());
    // The original headers, less the blank line that ended them, in this
    // message's line endings.
    let text = String::from_utf8_lossy(original_headers);
    for line in text.trim_end_matches(['\r', '\n']).lines() {
        body.extend_from_slice(line.trim_end_matches('\r').as_bytes());
        body.extend_from_slice(eol.as_bytes());
    }
    let ascii = body.is_ascii();
    let mut out = Vec::new();
    let mut line = |s: &str| {
        out.extend_from_slice(s.as_bytes());
        out.extend_from_slice(eol.as_bytes());
    };
    line(&format!(
        "Content-Type: text/x-moz-deleted; charset=UTF-8; name={label}"
    ));
    line(if ascii {
        "Content-Transfer-Encoding: 7bit"
    } else {
        "Content-Transfer-Encoding: base64"
    });
    line(&format!("Content-Disposition: inline; filename={label}"));
    line(&format!(
        "X-Mozilla-Altered: AttachmentDeleted; date=\"{date}\""
    ));
    line("");
    if ascii {
        out.extend_from_slice(&body);
    } else {
        out.extend_from_slice(base64_lines(&body, eol).as_bytes());
    }
    // The boundary that follows belongs on a line of its own; the part
    // replaced ended just before the line break that precedes it.
    if out.ends_with(eol.as_bytes()) {
        out.truncate(out.len() - eol.len());
    }
    out
}

/// A quoted header parameter, as an RFC 2047 encoded-word when the name is
/// not plain ASCII (how most mail clients write attachment names).
fn header_param(value: &str) -> String {
    if value.is_ascii() && !value.contains(['"', '\\', '\r', '\n']) {
        return format!("\"{value}\"");
    }
    let clean: String = value.chars().filter(|c| !c.is_control()).collect();
    format!("\"=?UTF-8?B?{}?=\"", base64_encode(clean.as_bytes()))
}

/// Now, as Thunderbird dates the note: `Thu Sep 25 10:00:00 2026`.
pub(super) fn note_date() -> String {
    chrono::Local::now()
        .format("%a %b %e %H:%M:%S %Y")
        .to_string()
}

/// Put what a message holds after an attachment was removed (#289) into
/// the cache: the files, the paperclip, and the gallery's index of them.
pub(in crate::worker) fn cache_rewritten(
    c: &Cache,
    account_id: u32,
    path: &str,
    uid: u32,
    raw: &[u8],
) {
    let items = extract_attachments(raw);
    c.save_attachments(account_id, path, uid, &items);
    c.mark_attachments_checked(account_id, path, uid);
    c.set_has_attachment(account_id, path, uid, !items.is_empty());
    let metas: Vec<crate::models::AttachmentMeta> = items
        .iter()
        .enumerate()
        .map(|(i, a)| crate::models::AttachmentMeta {
            idx: i as u32,
            name: a.name.clone(),
            mime: guess_mime(&a.name),
            size: a.data.len() as u64,
            section: String::new(),
        })
        .collect();
    c.save_attachment_meta(account_id, path, uid, &metas);
}

/// An attachment removal that did not happen (#289): the error, and word
/// to the views showing the file as being deleted that it is not.
pub(in crate::worker) fn strip_refused(
    emit: &impl Fn(WorkerEvent),
    text: String,
    message_id: u32,
    name: String,
    size: u64,
) {
    emit(WorkerEvent::error(text));
    emit(WorkerEvent::AttachmentNotDeleted {
        message_id,
        name,
        size,
    });
}

/// A message with a body and two files, for the tests here and the live
/// ones against a real server.
#[cfg(test)]
pub(super) const MIXED: &str = "From: a@example.org\r\n\
To: b@example.org\r\n\
Subject: Report\r\n\
Message-ID: <r1@example.org>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"XX\"\r\n\
\r\n\
--XX\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Here is the report.\r\n\
--XX\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQKJcTl8uXrp/Og0MTGCg==\r\n\
--XX\r\n\
Content-Type: text/csv; name=\"data.csv\"\r\n\
Content-Disposition: attachment; filename=\"data.csv\"\r\n\
\r\n\
a,b\r\n\
1,2\r\n\
--XX--\r\n";

#[cfg(test)]
mod tests {
    use super::*;

    use crate::worker::render::attachments::extract_attachments;

    fn names(raw: &[u8]) -> Vec<String> {
        extract_attachments(raw)
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    #[test]
    fn removes_only_the_named_part() {
        let out = without_attachment(
            MIXED.as_bytes(),
            "report.pdf",
            18,
            "Thu Sep 25 10:00:00 2026",
        )
        .unwrap();
        let text = String::from_utf8(out.clone()).unwrap();
        assert_eq!(names(&out), vec!["data.csv"]);
        assert!(!text.contains("JVBERi0x"));
        assert!(text.contains(
            "Content-Type: text/x-moz-deleted; charset=UTF-8; name=\"Deleted: report.pdf\"\r\n"
        ));
        assert!(text
            .contains("X-Mozilla-Altered: AttachmentDeleted; date=\"Thu Sep 25 10:00:00 2026\""));
        // Everything around the part is untouched.
        let head = MIXED.find("--XX\r\nContent-Type: application/pdf").unwrap();
        assert!(text.starts_with(&MIXED[..head]));
        let tail = MIXED.find("--XX\r\nContent-Type: text/csv").unwrap();
        assert!(text.ends_with(&format!("\r\n{}", &MIXED[tail..])));
        // The body still reads the same.
        let parsed = MessageParser::default().parse(&out).unwrap();
        assert_eq!(
            parsed.body_text(0).as_deref().map(str::trim),
            Some("Here is the report.")
        );
        assert_eq!(parsed.body_text(1), None);
        // The note is not drawn into the reader either.
        assert!(!super::super::extract_body(&out).contains("You deleted"));
    }

    #[test]
    fn removing_every_attachment_leaves_none_listed() {
        let once = without_attachment(MIXED.as_bytes(), "report.pdf", 18, "d").unwrap();
        let twice = without_attachment(&once, "data.csv", 8, "d").unwrap();
        assert!(names(&twice).is_empty());
        let parsed = MessageParser::default().parse(&twice).unwrap();
        assert_eq!(
            parsed.body_text(0).as_deref().map(str::trim),
            Some("Here is the report.")
        );
    }

    #[test]
    fn keeps_bare_newlines() {
        let lf = MIXED.replace("\r\n", "\n");
        let out = without_attachment(lf.as_bytes(), "data.csv", 8, "d").unwrap();
        assert!(!out.windows(2).any(|w| w == b"\r\n"));
        assert_eq!(names(&out), vec!["report.pdf"]);
    }

    #[test]
    fn unknown_name_is_not_found() {
        assert_eq!(
            without_attachment(MIXED.as_bytes(), "nope.pdf", 1, "d"),
            Err(StripError::NotFound)
        );
    }

    #[test]
    fn refuses_signed_mail() {
        let signed = MIXED.replace(
            "multipart/mixed; boundary=\"XX\"",
            "multipart/signed; protocol=\"application/pgp-signature\"; boundary=\"XX\"",
        );
        assert_eq!(
            without_attachment(signed.as_bytes(), "data.csv", 8, "d"),
            Err(StripError::Protected)
        );
    }

    #[test]
    fn refuses_single_part_attachment() {
        let single = "From: a@example.org\r\n\
Content-Type: application/pdf; name=\"x.pdf\"\r\n\
Content-Disposition: attachment; filename=\"x.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQK\r\n";
        assert_eq!(
            without_attachment(single.as_bytes(), "x.pdf", 9, "d"),
            Err(StripError::WholeMessage)
        );
    }

    #[test]
    fn non_ascii_name_is_encoded() {
        let raw = MIXED.replace("data.csv", "données.csv");
        let out = without_attachment(raw.as_bytes(), "données.csv", 8, "d").unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("name=\"=?UTF-8?B?"));
        assert_eq!(names(&out), vec!["report.pdf"]);
    }
}
