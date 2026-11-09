//! Base64: standard encoding, and a lenient decoder that a mail
//! body's wrapped lines and truncated fetches need.

/// Standard base64, for inline `data:` URIs.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Standard base64 cut into lines of 76 characters (MIME's limit, RFC
/// 2045), each line ended by `eol`, the last one too.
pub(crate) fn base64_lines(data: &[u8], eol: &str) -> String {
    const LINE: usize = 76;
    let encoded = base64_encode(data);
    let mut out = String::with_capacity(encoded.len() + (encoded.len() / LINE + 1) * eol.len());
    let mut rest = encoded.as_str();
    while !rest.is_empty() {
        let (line, tail) = rest.split_at(rest.len().min(LINE));
        out.push_str(line);
        out.push_str(eol);
        rest = tail;
    }
    out
}

/// Decode standard base64, skipping whitespace and `=` wherever they fall (a
/// MIME body wraps its lines) and a lone last character that cannot make a
/// byte. `None` if the input isn't valid (which for a truncated fetch is a
/// real possibility, not a bug).
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    use base64::Engine;
    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    );
    let mut clean: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    if clean.len() % 4 == 1 {
        clean.pop();
    }
    LENIENT.decode(clean).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two copies `base64_lines` replaced, as they were: the worker's
    /// placeholder for a deleted attachment (strip.rs, either line ending)
    /// and outlook_msg's, which ended every line with CRLF.
    #[test]
    fn base64_lines_match_the_copies_they_replace() {
        fn strip_copy(body: &[u8], eol: &[u8]) -> Vec<u8> {
            use base64::Engine;
            let mut out = Vec::new();
            let encoded = base64::engine::general_purpose::STANDARD.encode(body);
            for chunk in encoded.as_bytes().chunks(76) {
                out.extend_from_slice(chunk);
                out.extend_from_slice(eol);
            }
            out
        }
        fn outlook_copy(bytes: &[u8]) -> String {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            let mut out = String::with_capacity(encoded.len() + encoded.len() / 76 * 2);
            for chunk in encoded.as_bytes().chunks(76) {
                out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
                out.push_str("\r\n");
            }
            out
        }
        let all: Vec<u8> = (0..=255).cycle().take(1000).collect();
        for len in [0, 1, 2, 3, 56, 57, 58, 113, 114, 115, 171, 1000] {
            let data = &all[..len];
            assert_eq!(base64_lines(data, "\r\n"), outlook_copy(data), "{len}");
            for eol in ["\r\n", "\n"] {
                assert_eq!(
                    base64_lines(data, eol).into_bytes(),
                    strip_copy(data, eol.as_bytes()),
                    "{len} {eol:?}"
                );
            }
        }
        assert_eq!(base64_lines(b"", "\r\n"), "");
        assert_eq!(base64_lines(b"hi!?", "\n"), "aGkhPw==\n");
        let line = base64_lines(&[0; 57], "\r\n");
        assert_eq!(line, format!("{}\r\n", "A".repeat(76)));
        assert_eq!(
            base64_lines(&[0; 58], "\n"),
            format!("{}\nAA==\n", "A".repeat(76))
        );
    }

    #[test]
    fn base64_is_as_forgiving_as_a_mail_body_needs() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
        assert_eq!(base64_encode(b"hi!?"), "aGkhPw==");
        assert_eq!(base64_decode("aGkh\r\nPw==").unwrap(), b"hi!?");
        assert_eq!(
            base64_decode("aGkhPw").unwrap(),
            b"hi!?",
            "padding is optional"
        );
        assert_eq!(
            base64_decode("aGkhP").unwrap(),
            b"hi!",
            "a lone last character is dropped"
        );
        assert_eq!(base64_decode("aG*h"), None);
    }
}
