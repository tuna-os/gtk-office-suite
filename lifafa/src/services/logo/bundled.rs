//! Bundled marks: `data/logos/`, embedded by build.rs (see
//! tools/fetch-logos.py), and the SVG reframing that squares them.

use std::collections::HashMap;
use std::sync::OnceLock;

#[cfg(test)]
use super::mailbox::is_mailbox_host;
use super::{domain_of, host_of, LOGO_PX};

#[derive(serde::Deserialize)]
struct LogoFile {
    #[serde(default)]
    logo: Vec<LogoEntry>,
}

/// One line of `data/logos/logos.toml`: a sender domain and the mark it
/// gets — `source` is `gilbarbara`, `simple` or `brand` (one of
/// `data/brands/`), `file` the SVG or mark id, `color` Simple Icons' brand
/// hex.
#[derive(serde::Deserialize, Clone)]
pub(super) struct LogoEntry {
    domain: String,
    pub(super) source: String,
    pub(super) file: String,
    #[serde(default)]
    color: Option<String>,
}

fn bundled_map() -> &'static HashMap<String, LogoEntry> {
    static MAP: OnceLock<HashMap<String, LogoEntry>> = OnceLock::new();
    MAP.get_or_init(|| {
        let file: LogoFile = match toml::from_str(include_str!("../../../data/logos/logos.toml")) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("bundled logo map unreadable: {e}");
                LogoFile { logo: Vec::new() }
            }
        };
        file.logo
            .into_iter()
            .map(|e| (e.domain.clone(), e))
            .collect()
    })
}

/// The bundled mark for a sender: the sending host's entry first
/// (`aws.amazon.com`), then the registrable domain's.
pub(super) fn bundled_entry(email: &str) -> Option<&'static LogoEntry> {
    let map = bundled_map();
    host_of(email)
        .and_then(|h| map.get(&h))
        .or_else(|| domain_of(email).and_then(|d| map.get(&d)))
}

/// Whether a sender has a bundled mark (shown with no request made).
#[cfg(test)]
pub(super) fn has_bundled(email: &str) -> bool {
    !is_mailbox_host(email) && bundled_entry(email).is_some()
}

/// A bundled mark's bytes: the app's service marks as their PNG, the SVG
/// sets framed to a `LOGO_PX` square — a Simple Icons glyph in the color
/// that reads on its brand color, over that color; a gilbarbara mark
/// over white, inset a little, since many are dark on transparent.
pub(super) fn bundled_bytes(entry: &LogoEntry) -> Option<Vec<u8>> {
    match entry.source.as_str() {
        "brand" => crate::desktop::brand::png(&entry.file).map(<[u8]>::to_vec),
        "simple" | "gilbarbara" => {
            let path = format!("/co/hyprlab/Hylki/logos/{}/{}", entry.source, entry.file);
            let data =
                gtk::gio::resources_lookup_data(&path, gtk::gio::ResourceLookupFlags::NONE).ok()?;
            let svg = std::str::from_utf8(&data).ok()?;
            let framed = if entry.source == "simple" {
                let bg = entry.color.as_deref().unwrap_or("#000000");
                let fg = crate::color::readable_text(bg).to_string();
                square_svg(svg, LOGO_PX, Some(bg), 0.22, Some(&fg))?
            } else {
                square_svg(svg, LOGO_PX, Some("#ffffff"), 0.12, None)?
            };
            Some(framed.into_bytes())
        }
        _ => None,
    }
}

/// `svg` re-framed as a `px`-square document: its drawing centred and
/// scaled to fit, inset by `pad` (a fraction of the edge) on each side,
/// over `bg` when given, with `fill` as the color of content that sets
/// none (Simple Icons' glyphs). The original root's attributes ride on
/// the nested element, so namespaces, styles and gradients keep working;
/// a root with no `viewBox` gets one from its width and height. `None`
/// for anything that is not an SVG document.
pub(super) fn square_svg(
    svg: &str,
    px: u32,
    bg: Option<&str>,
    pad: f32,
    fill: Option<&str>,
) -> Option<String> {
    let lower = svg.to_ascii_lowercase();
    let start = svg_root_start(&lower)?;
    let open_end = start + svg[start..].find('>')?;
    let close = lower.rfind("</svg>")?;
    if close <= open_end {
        return None;
    }
    let attrs = parse_attrs(svg[start + 4..open_end].trim_end_matches('/'));
    let content = &svg[open_end + 1..close];
    let view_box = attrs
        .iter()
        .find(|(k, _)| k == "viewbox")
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| v.split([' ', ',']).filter(|s| !s.is_empty()).count() == 4)
        .or_else(|| {
            let dim = |name: &str| {
                attrs
                    .iter()
                    .find(|(k, _)| k == name)
                    .and_then(|(_, v)| {
                        v.trim()
                            .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%')
                            .parse::<f32>()
                            .ok()
                    })
                    .filter(|n| *n > 0.0)
            };
            Some(format!("0 0 {} {}", dim("width")?, dim("height")?))
        })?;
    let inset = (px as f32 * pad).round();
    let inner = px as f32 - 2.0 * inset;
    let mut kept = String::new();
    for (k, v) in &attrs {
        if matches!(
            k.as_str(),
            "width" | "height" | "x" | "y" | "viewbox" | "preserveaspectratio"
        ) || (fill.is_some() && k == "fill")
        {
            continue;
        }
        kept.push(' ');
        kept.push_str(k);
        kept.push_str("=\"");
        kept.push_str(&v.replace('"', "&quot;"));
        kept.push('"');
    }
    let bg_rect = bg
        .map(|c| format!("<rect width=\"{px}\" height=\"{px}\" fill=\"{c}\"/>"))
        .unwrap_or_default();
    let fill_attr = fill.map(|f| format!(" fill=\"{f}\"")).unwrap_or_default();
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{px}\" height=\"{px}\" viewBox=\"0 0 {px} {px}\">\
         {bg_rect}<svg x=\"{inset}\" y=\"{inset}\" width=\"{inner}\" height=\"{inner}\" viewBox=\"{view_box}\" \
         preserveAspectRatio=\"xMidYMid meet\"{kept}{fill_attr}>{content}</svg></svg>"
    ))
}

/// Where the root `<svg` tag starts in a lowercased document, skipping the
/// XML declaration, comments and a doctype.
fn svg_root_start(lower: &str) -> Option<usize> {
    let mut at = 0;
    while let Some(i) = lower[at..].find("<svg") {
        let pos = at + i;
        let next = lower[pos + 4..].chars().next();
        if next.is_none_or(|c| c.is_whitespace() || c == '>' || c == '/') {
            return Some(pos);
        }
        at = pos + 4;
    }
    None
}

/// `name="value"` pairs of a tag (names lowercased and kept whole, so
/// `xmlns:xlink` survives).
fn parse_attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag.trim();
    while !rest.is_empty() {
        let name_len = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let name = rest[..name_len].trim().to_ascii_lowercase();
        rest = rest[name_len..].trim_start();
        let mut value = String::new();
        if let Some(r) = rest.strip_prefix('=') {
            let r = r.trim_start();
            if let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let inner = &r[1..];
                let end = inner.find(q).unwrap_or(inner.len());
                value = inner[..end].to_string();
                rest = inner[end..].strip_prefix(q).unwrap_or("").trim_start();
            } else {
                let end = r.find(|c: char| c.is_whitespace()).unwrap_or(r.len());
                value = r[..end].to_string();
                rest = r[end..].trim_start();
            }
        }
        if !name.is_empty() {
            out.push((name, value));
        }
    }
    out
}

/// Whether these bytes are an SVG document (an `<svg` tag near the top).
pub(super) fn looks_like_svg(bytes: &[u8]) -> bool {
    let head: String = bytes
        .iter()
        .take(1024)
        .map(|b| *b as char)
        .collect::<String>()
        .to_ascii_lowercase();
    svg_root_start(&head).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::logo::known_missing;

    #[test]
    fn an_svg_is_reframed_to_a_square_and_keeps_its_root_attributes() {
        let glyph = r##"<svg role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><title>PayPal</title><path d="M1 1h22v22H1z"/></svg>"##;
        let out = square_svg(glyph, 160, Some("#002991"), 0.22, Some("#ffffff")).unwrap();
        assert!(out.starts_with(r##"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="160" viewBox="0 0 160 160">"##), "{out}");
        assert!(
            out.contains(r##"<rect width="160" height="160" fill="#002991"/>"##),
            "{out}"
        );
        assert!(out.contains(r##"<svg x="35" y="35" width="90" height="90" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet" role="img" xmlns="http://www.w3.org/2000/svg" fill="#ffffff">"##), "{out}");
        assert!(
            out.ends_with(r##"<path d="M1 1h22v22H1z"/></svg></svg>"##),
            "{out}"
        );
        // A prologue, a doctype and px-suffixed dimensions in place of a viewBox.
        let mark = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE svg><svg width=\"256px\" height=\"128px\" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><rect width=\"256\" height=\"128\"/></svg>";
        let out = square_svg(mark, 160, Some("#ffffff"), 0.0, None).unwrap();
        assert!(out.contains(r##"viewBox="0 0 256 128""##), "{out}");
        assert!(
            out.contains(r##"xmlns:xlink="http://www.w3.org/1999/xlink""##),
            "{out}"
        );
        // Not an SVG at all.
        assert_eq!(
            square_svg("<html><body>no</body></html>", 160, None, 0.0, None),
            None
        );
        assert!(looks_like_svg(
            b"<?xml version=\"1.0\"?>\n<!-- x -->\n<svg viewBox=\"0 0 1 1\"/>"
        ));
        assert!(!looks_like_svg(b"<svgfoo>"));
    }

    #[test]
    fn the_bundled_map_knows_common_senders_by_host_and_domain() {
        let paypal = bundled_entry("service@paypal.com").expect("paypal is bundled");
        assert_eq!(paypal.source, "simple");
        assert_eq!(paypal.file, "paypal.svg");
        assert!(paypal.color.is_some());
        // The sending host wins over the registrable domain when both are listed.
        assert_eq!(
            bundled_entry("no-reply@aws.amazon.com").map(|e| e.file.as_str()),
            Some("aws.svg")
        );
        // A subdomain falls back to the registrable domain's mark.
        assert_eq!(
            bundled_entry("noreply@mail.spotify.com").map(|e| e.source.as_str()),
            Some("gilbarbara")
        );
        // The app's own service marks cover the mail providers.
        assert!(bundled_entry("someone@example.org").is_none());
        assert!(has_bundled("x@paypal.com") && !known_missing("x@paypal.com"));
    }
}
