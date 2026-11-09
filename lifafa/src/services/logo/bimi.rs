//! BIMI: the logo a sender publishes for mail clients, named by a DNS TXT
//! record at `default._bimi.<domain>`.

use super::bundled::{looks_like_svg, square_svg};
use super::site::USER_AGENT;
use super::{domain_of, host_of, BIMI_MAX_BYTES, LOGO_PX};

/// What asking for a sender's BIMI logo came to.
pub(super) enum Bimi {
    /// The logo, as a `LOGO_PX`-square SVG document.
    Logo(Vec<u8>),
    /// The sender publishes none (or nothing usable): not asked again for a week.
    None,
    /// The question could not be asked (no network, resolver down): asked again next time.
    Unreachable,
}

/// The sender's BIMI logo, from the sending host's record or the
/// registrable domain's. Blocking (DNS, then one HTTPS fetch); call off
/// the main thread.
pub(super) fn bimi_logo(email: &str) -> Bimi {
    let mut names: Vec<String> = Vec::new();
    if let Some(host) = host_of(email) {
        names.push(host);
    }
    if let Some(domain) = domain_of(email) {
        if !names.contains(&domain) {
            names.push(domain);
        }
    }
    let mut unreachable = false;
    for name in names {
        let url = match bimi_record_url(&name) {
            Ok(Some(url)) => url,
            Ok(None) => continue,
            Err(()) => {
                unreachable = true;
                continue;
            }
        };
        if let Some(svg) = bimi_fetch(&url) {
            if let Some(framed) = square_svg(&svg, LOGO_PX, None, 0.0, None) {
                return Bimi::Logo(framed.into_bytes());
            }
        }
    }
    if unreachable {
        Bimi::Unreachable
    } else {
        Bimi::None
    }
}

/// The logo URL in `default._bimi.<name>`'s TXT record: `Ok(None)` when
/// the name has no such record, `Err` when the resolver could not say.
fn bimi_record_url(name: &str) -> Result<Option<String>, ()> {
    use gtk::gio::prelude::*;
    let resolver = gtk::gio::Resolver::default();
    let records = match resolver.lookup_records(
        &format!("default._bimi.{name}"),
        gtk::gio::ResolverRecordType::Txt,
        gtk::gio::Cancellable::NONE,
    ) {
        Ok(r) => r,
        Err(e)
            if e.kind::<gtk::gio::ResolverError>() == Some(gtk::gio::ResolverError::NotFound) =>
        {
            tracing::debug!("bimi: no record for {name}");
            return Ok(None);
        }
        Err(e) => {
            tracing::debug!("bimi: could not look up {name}: {e}");
            return Err(());
        }
    };
    for record in records {
        // A TXT record is `(as)`: the strings of one record, to be joined.
        let text: String = record
            .child_value(0)
            .iter()
            .filter_map(|v| v.str().map(str::to_string))
            .collect();
        tracing::debug!("bimi: {name} record {text:?}");
        if let Some(url) = parse_bimi_record(&text) {
            return Ok(Some(url));
        }
    }
    Ok(None)
}

/// The `l=` location of a `v=BIMI1` record, when it is an https URL.
/// An empty `l=` (a declared opt-out) or any other version is nothing.
fn parse_bimi_record(txt: &str) -> Option<String> {
    let mut is_bimi = false;
    let mut location: Option<String> = None;
    for tag in txt.split(';') {
        let Some((k, v)) = tag.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "v" => is_bimi = v.eq_ignore_ascii_case("BIMI1"),
            "l" => location = Some(v.to_string()),
            _ => {}
        }
    }
    let url = location.filter(|_| is_bimi)?;
    url.to_ascii_lowercase()
        .starts_with("https://")
        .then_some(url)
}

/// The BIMI SVG at `url`: small, served as an image or XML, and an SVG.
fn bimi_fetch(url: &str) -> Option<String> {
    use std::io::Read;
    let resp = match ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "image/svg+xml,*/*;q=0.5")
        .timeout(std::time::Duration::from_secs(5))
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("bimi: {url}: {e}");
            return None;
        }
    };
    let ct = resp.content_type().to_ascii_lowercase();
    if !(ct.is_empty() || ct.contains("svg") || ct.contains("xml") || ct.contains("octet-stream")) {
        tracing::debug!("bimi: {url}: not an SVG ({ct})");
        return None;
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .take(BIMI_MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() || buf.len() as u64 > BIMI_MAX_BYTES || !looks_like_svg(&buf) {
        tracing::debug!("bimi: {url}: {} bytes, not usable", buf.len());
        return None;
    }
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bimi_record_names_its_logo() {
        assert_eq!(
            parse_bimi_record("v=BIMI1; l=https://www.apple.com/bimi/v2/apple.svg;a=https://www.apple.com/bimi/v2/apple.pem;").as_deref(),
            Some("https://www.apple.com/bimi/v2/apple.svg")
        );
        // Tags in any order, spacing free.
        assert_eq!(
            parse_bimi_record("l=https://x.example/l.svg;v=bimi1").as_deref(),
            Some("https://x.example/l.svg")
        );
        // A declared opt-out, a plain-http location, an SPF record.
        assert_eq!(parse_bimi_record("v=BIMI1; l=;"), None);
        assert_eq!(parse_bimi_record("v=BIMI1; l=http://x.example/l.svg"), None);
        assert_eq!(parse_bimi_record("v=spf1 include:spf.example ~all"), None);
    }
}
