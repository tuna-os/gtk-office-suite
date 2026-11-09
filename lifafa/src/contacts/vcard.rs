//! vCard text: unfolding, splitting a content line, escaping, and reading
//! the display name and the photo.

use std::path::PathBuf;

use super::read::{cache_dir, data_dir};

// vCard's TEXT escapes are iCalendar's (crate::encoding).
pub(super) use crate::encoding::escape_text_value as escape_vcard_value;
pub(crate) use crate::encoding::unescape_text_value as unescape_vcard_text;

/// Extract the display name (`FN`) from a vCard, preserving its capitalisation.
/// Handles line folding (a CRLF/LF followed by a space or tab continues the
/// previous line) and the standard text escapes.
pub(crate) fn unfold_vcard(vcard: &str) -> String {
    vcard
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "")
        .replace("\n\t", "")
}

/// The name in a content line's property part, without its parameters
/// or group prefix: `item1.EMAIL;TYPE=WORK` gives `EMAIL`.
pub(super) fn property_name(property: &str) -> &str {
    property
        .split(';')
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
}

/// Split a (unfolded) vCard content line into property (name + params) and
/// value at the first colon *outside* double quotes. A naive colon split
/// breaks on quoted parameter values — iCloud's photo lines carry
/// `X-EVOLUTION-WEBDAV-IMG-URL="https://…"` before the real value, and the
/// `https:` inside the quotes swallowed everything after it.
pub(crate) fn split_vcard_line(line: &str) -> Option<(&str, &str)> {
    let mut in_quotes = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => return Some((&line[..i], &line[i + 1..])),
            _ => {}
        }
    }
    None
}

pub(super) fn vcard_display_name(vcard: &str) -> Option<String> {
    for line in unfold_vcard(vcard).lines() {
        let Some((prop, value)) = line.split_once(':') else {
            continue;
        };
        // The property name is everything before the first ';' (which begins any
        // parameters), e.g. `FN` or `FN;PID=1.1`.
        let name = prop.split(';').next().unwrap_or("").trim();
        if name.eq_ignore_ascii_case("FN") {
            let value = unescape_vcard_text(value);
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Decode an embedded vCard PHOTO. Handles both Nextcloud's escaped data URI
/// form and the conventional `PHOTO;ENCODING=b` form. Remote PHOTO URLs are
/// deliberately not fetched, both for privacy and to avoid tracking.
pub(super) fn vcard_photo(vcard: &str) -> Option<Vec<u8>> {
    vcard_photo_with(vcard, true)
}

/// [`vcard_photo`], optionally refusing `file://` photos. The local Hylki book
/// holds vCards from arbitrary `.vcf` files, which must not be able to point
/// at files EDS keeps.
pub(crate) fn vcard_photo_with(vcard: &str, allow_files: bool) -> Option<Vec<u8>> {
    use base64::Engine as _;

    const MAX_PHOTO_BYTES: usize = 2_000_000;
    for line in unfold_vcard(vcard).lines() {
        let Some((property, raw_value)) = split_vcard_line(line) else {
            continue;
        };
        let name = property_name(property).trim();
        if !name.eq_ignore_ascii_case("PHOTO") {
            continue;
        }

        let value = raw_value.replace("\\;", ";").replace("\\,", ",");
        let property_upper = property.to_ascii_uppercase();
        let payload = if value.to_ascii_lowercase().starts_with("data:image/") {
            let Some((metadata, payload)) = value.split_once(',') else {
                continue;
            };
            if !metadata.to_ascii_lowercase().ends_with(";base64") {
                continue;
            }
            payload
        } else if property_upper.contains("ENCODING=B")
            || property_upper.contains("ENCODING=BASE64")
        {
            value.as_str()
        } else if allow_files && value.to_ascii_lowercase().starts_with("file://") {
            if let Some(bytes) = read_eds_photo_file(&value, MAX_PHOTO_BYTES) {
                return Some(bytes);
            }
            continue;
        } else {
            // Never fetch http(s) or other remote PHOTO URIs implicitly.
            continue;
        };

        let encoded: String = payload
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        // Base64 expands data by roughly 4/3. Reject oversized input before
        // allocating the decoded image buffer.
        if encoded.len() > (MAX_PHOTO_BYTES * 4 / 3 + 4) {
            continue;
        }
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
            if !bytes.is_empty() && bytes.len() <= MAX_PHOTO_BYTES {
                return Some(bytes);
            }
        }
    }
    None
}

/// Read an EDS-materialized `file://` photo, but only from an address-book data
/// or cache directory. A CardDAV vCard must not be able to make Hylki read an
/// arbitrary local file.
pub(super) fn read_eds_photo_file(uri: &str, max_bytes: usize) -> Option<Vec<u8>> {
    use std::io::Read;

    let (path, host) = gtk::glib::filename_from_uri(uri).ok()?;
    if host.is_some() {
        return None;
    }
    let roots: Vec<PathBuf> = [data_dir(), cache_dir()].into_iter().flatten().collect();
    let path = confine_to_roots(&path, &roots)?;

    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (!bytes.is_empty() && bytes.len() <= max_bytes).then_some(bytes)
}

/// Canonicalize `path` and admit it only when it lands inside one of `roots`
/// (also canonicalized) — `..` segments and symlinks are resolved before the
/// containment check, so neither can escape a root. No resolvable roots means
/// nothing is admitted.
pub(super) fn confine_to_roots(path: &std::path::Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| path.starts_with(&root))
        .then_some(path)
}

/// `s` split at each `sep` not escaped with a backslash; the parts keep
/// their escapes.
pub(super) fn split_unescaped(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut escaped) = (0, false);
    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == sep {
            parts.push(&s[start..i]);
            start = i + ch.len_utf8();
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Whether a stored vCard is a contact group, for the reads that only want
/// people and never parse the whole card.
pub(super) fn is_group_vcard(vcard: &str) -> bool {
    unfold_vcard(vcard).lines().any(|line| {
        let Some((prop, value)) = line.split_once(':') else {
            return false;
        };
        let name = property_name(prop).trim();
        let value = value.trim();
        (name.eq_ignore_ascii_case("X-EVOLUTION-LIST") && value.eq_ignore_ascii_case("true"))
            || ((name.eq_ignore_ascii_case("KIND")
                || name.eq_ignore_ascii_case("X-ADDRESSBOOKSERVER-KIND"))
                && value.eq_ignore_ascii_case("group"))
    })
}

/// `line` put after a card's VERSION line (first, when it has none).
pub(super) fn insert_after_version(card: &str, line: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut put = false;
    for l in card.split("\r\n").flat_map(|l| l.split('\n')) {
        out.push(l.to_string());
        if !put && l.to_ascii_uppercase().starts_with("VERSION:") {
            out.push(line.to_string());
            put = true;
        }
    }
    if !put {
        out.insert(1.min(out.len()), line.to_string());
    }
    out.join("\r\n")
}
