//! Recipient fields as the composer types them: parsing, cleaning pasted
//! addresses, and the lettre mailboxes they become.

use lettre::message::Mailbox;
use lettre::Address;

use crate::worker::smtp::SmtpError;

/// Parse a recipient field ("Name <a@b>, c@d") into (name, email) pairs.
///
/// A comma inside a quoted name (`"Martin, Jason" <j@x>`) or inside the
/// brackets is not a separator. Nor is one in an unquoted name, which is what
/// a pasted or hand-typed `Martin, Jason <j@x>` has: a piece with no `@` that
/// runs into a `Name <addr>` is the front of that name.
pub(crate) fn parse_recipients(s: &str) -> Vec<(String, String)> {
    let mut pieces: Vec<String> = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut escaped, mut angle) = (false, false, false);
    for ch in s.chars() {
        match ch {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' if !quoted && !angle => {
                pieces.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    pieces.push(cur);

    let mut out = Vec::new();
    let mut carry = String::new();
    for (i, piece) in pieces.iter().enumerate() {
        let part = if carry.is_empty() {
            piece.trim().to_string()
        } else {
            format!("{carry},{piece}").trim().to_string()
        };
        carry.clear();
        if part.is_empty() {
            continue;
        }
        let next_is_named = pieces.get(i + 1).is_some_and(|n| n.contains('<'));
        if !part.contains('@') && next_is_named {
            carry = part;
            continue;
        }
        match (part.rfind('<'), part.rfind('>')) {
            (Some(lt), Some(gt)) if lt < gt => {
                out.push((unquote_name(&part[..lt]), clean_address(&part[lt + 1..gt])));
            }
            _ => out.push((String::new(), clean_address(&part))),
        }
    }
    out
}

/// Characters that take no space on screen: zero-width spaces and joiners,
/// direction marks and embeddings, the BOM and the soft hyphen.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

/// An address as typed or pasted, without the invisible characters a copy
/// from a web page or a chat brings along (#368). The server refuses an
/// address with one inside, and nobody can see why. A display name keeps
/// its characters: a zero-width non-joiner is part of how a Persian name is
/// spelled.
fn clean_address(raw: &str) -> String {
    raw.chars()
        .filter(|&c| !is_invisible(c))
        .map(|c| {
            if matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}') {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// The field with the invisible characters taken out of its addresses, when
/// it had any: the composer shows the cleaned text before it sends.
pub(crate) fn clean_recipient_field(field: &str) -> Option<String> {
    if !field
        .chars()
        .any(|c| is_invisible(c) || matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'))
    {
        return None;
    }
    let clean = parse_recipients(field)
        .iter()
        .map(|(name, addr)| format_recipient(name, addr))
        .collect::<Vec<_>>()
        .join(", ");
    (clean != field.trim()).then_some(clean)
}

/// The first address in a recipient field that cannot be sent to, so the
/// composer can say so while the message is still open.
pub(crate) fn invalid_recipient(field: &str) -> Option<String> {
    parse_recipients(field)
        .into_iter()
        .map(|(_, addr)| addr)
        .find(|addr| addr.parse::<Address>().is_err())
}

/// A display name as written in a header, without its quotes and escapes.
fn unquote_name(raw: &str) -> String {
    let raw = raw.trim();
    let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
        return raw.trim_matches('"').trim().to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars.next().unwrap_or(c)
        } else {
            c
        });
    }
    out.trim().to_string()
}

/// "Name <addr>" for a recipient field (the address alone when there is no
/// name), quoting a name that has a comma or another character RFC 5322 keeps
/// out of a bare name, so [`parse_recipients`] reads it back whole.
pub(crate) fn format_recipient(name: &str, email: &str) -> String {
    let name = name.trim();
    if name.is_empty() || name == email {
        return email.to_string();
    }
    if name.contains(|c: char| "()<>[]:;@\\,.\"".contains(c)) {
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\" <{email}>")
    } else {
        format!("{name} <{email}>")
    }
}

/// Build a `Name <addr>` mailbox from its parts. Never format the two into one
/// string and parse that back: an RFC 5322 display name has to be quoted unless
/// it is a bare atom, so "Alfonso Lizárraga", "Martin, Jason" or a name that is
/// itself an address ("a@b.com <a@b.com>", which is what an import with no
/// separate display name produces) all fail to parse and the send is rejected.
/// `Mailbox` quotes and encodes the name itself when the header is written.
pub(in crate::worker) fn mailbox(name: &str, addr: &str) -> Result<Mailbox, SmtpError> {
    let address: Address = addr
        .parse()
        .map_err(|e| format!("invalid email address {addr:?}: {e}"))?;
    let name = name.trim();
    // A "display name" that just repeats the address is noise in the header.
    if name.is_empty() || name.eq_ignore_ascii_case(addr.trim()) {
        Ok(Mailbox::new(None, address))
    } else {
        Ok(Mailbox::new(Some(name.to_string()), address))
    }
}

#[cfg(test)]
mod recipient_tests {
    use super::{format_recipient, parse_recipients};
    use crate::worker::compose::recipients::mailbox;
    use crate::worker::{clean_recipient_field, invalid_recipient};

    fn pair(name: &str, email: &str) -> (String, String) {
        (name.to_string(), email.to_string())
    }

    #[test]
    fn a_comma_in_a_name_does_not_split_it() {
        assert_eq!(
            parse_recipients("\"Martin, Jason\" <j@x.example>, b@x.example"),
            vec![
                pair("Martin, Jason", "j@x.example"),
                pair("", "b@x.example")
            ]
        );
        assert_eq!(
            parse_recipients("Martin, Jason <j@x.example>, Ann <a@x.example>,"),
            vec![
                pair("Martin, Jason", "j@x.example"),
                pair("Ann", "a@x.example")
            ]
        );
        assert_eq!(
            parse_recipients("\"a \\\"b\\\" c\" <q@x.example>"),
            vec![pair("a \"b\" c", "q@x.example")]
        );
        // A stray word stays its own (invalid) entry rather than joining a
        // bare address, so the send still names it as the bad one.
        assert_eq!(
            parse_recipients("bob, a@x.example"),
            vec![pair("", "bob"), pair("", "a@x.example")]
        );
    }

    #[test]
    fn formatted_recipients_read_back() {
        for name in ["Jason Martin", "Martin, Jason", "J. \"Jay\" Martin", "a\\b"] {
            let field = format!("{}, c@x.example", format_recipient(name, "j@x.example"));
            assert_eq!(
                parse_recipients(&field),
                vec![pair(name, "j@x.example"), pair("", "c@x.example")],
                "{field}"
            );
        }
        assert_eq!(format_recipient("", "j@x.example"), "j@x.example");
        assert_eq!(
            format_recipient("Jason Martin", "j@x.example"),
            "Jason Martin <j@x.example>"
        );
    }

    /// #368: invisible characters pasted along with an address are dropped
    /// from the address, and kept in a name.
    #[test]
    fn pasted_addresses_lose_invisible_characters() {
        assert_eq!(
            parse_recipients(
                "\u{200b}ann\u{200b}@example.com\u{200e}, Bob <\u{feff}bob@example.com\u{a0}>"
            ),
            vec![
                (String::new(), "ann@example.com".to_string()),
                ("Bob".to_string(), "bob@example.com".to_string())
            ],
        );
        let persian = "\u{0645}\u{06cc}\u{200c}\u{0631}\u{0648}\u{062f}";
        assert_eq!(
            parse_recipients(&format!("{persian} <a@b.com>"))[0].0,
            persian
        );
        assert_eq!(
            clean_recipient_field("ann\u{200b}@example.com, Bob <bob@example.com>").as_deref(),
            Some("ann@example.com, Bob <bob@example.com>")
        );
        assert_eq!(clean_recipient_field("ann@example.com"), None);
        assert_eq!(
            invalid_recipient("ann@example.com, Bob <bob@example.com>"),
            None
        );
        assert_eq!(
            invalid_recipient("ann@example.com, bob@"),
            Some("bob@".to_string())
        );
        assert_eq!(
            invalid_recipient("ann example.com"),
            Some("ann example.com".to_string())
        );
    }

    #[test]
    fn mailbox_quotes_display_names_that_are_not_atoms() {
        // Each of these fails if the mailbox is built by formatting
        // "Name <addr>" and parsing the result back.
        for name in ["Alfonso Lizárraga", "Martin, Jason", "Dr. X", "a@b.com"] {
            let mb = mailbox(name, "a@b.com").expect("mailbox builds");
            assert_eq!(mb.email.to_string(), "a@b.com");
        }
    }

    #[test]
    fn mailbox_drops_a_name_that_merely_repeats_the_address() {
        // An import with no separate display name sets both to the address;
        // "a@b.com <a@b.com>" is noise, so the header carries the address alone.
        let mb = mailbox("A@B.com", "a@b.com").expect("mailbox builds");
        assert!(mb.name.is_none());
        assert_eq!(mb.to_string(), "a@b.com");
    }
}
