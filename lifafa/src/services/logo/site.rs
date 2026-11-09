//! The site's own icon: what its home page and web manifest declare, and
//! the well-known paths, largest first.

use super::bundled::looks_like_svg;

/// Where a site publishes its icon, largest first: what its home page and
/// web manifest declare, merged with the well-known paths (the root
/// `apple-touch-icon.png` counts as 180px, `favicon.ico` as 32px) — so a
/// declared 32px icon still loses to a root apple-touch-icon, and a declared
/// 512px one is tried before anything else.
pub(super) fn candidate_urls(domain: &str) -> Vec<String> {
    let mut found = discover(domain);
    found.extend([
        (180, format!("https://{domain}/apple-touch-icon.png")),
        (180, format!("https://www.{domain}/apple-touch-icon.png")),
        (32, format!("https://{domain}/favicon.ico")),
        (32, format!("https://www.{domain}/favicon.ico")),
    ]);
    found.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    let mut out: Vec<String> = Vec::new();
    for (_, url) in found {
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// The icons a site's home page declares — its `<link rel="icon">`s and
/// `apple-touch-icon`s — and those in its web manifest, each with the size
/// the site claims for it. Empty when the page cannot be read.
fn discover(domain: &str) -> Vec<(u32, String)> {
    let Some((base, html)) = get_text(&format!("https://{domain}/"))
        .or_else(|| get_text(&format!("https://www.{domain}/")))
    else {
        return Vec::new();
    };
    let mut found = link_icons(&html, &base);
    if let Some(manifest) = link_manifest(&html, &base) {
        if let Some((mbase, json)) = get_text(&manifest) {
            found.extend(manifest_icons(&json, &mbase));
        }
    }
    found
}

/// The browser-ish identity sites see: plenty answer a bare library
/// identity with a challenge page instead of their icon.
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux) Hylki";

/// A page or manifest as text, with the URL it was finally served from (so
/// relative links resolve against where redirects landed). Capped: the head
/// of a page is what matters, and a manifest is small.
fn get_text(url: &str) -> Option<(String, String)> {
    use std::io::Read;
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .set(
            "Accept",
            "text/html,application/manifest+json,application/json;q=0.9,*/*;q=0.5",
        )
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .ok()?;
    let final_url = resp.get_url().to_string();
    let mut buf = Vec::new();
    resp.into_reader()
        .take(512 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    Some((final_url, String::from_utf8_lossy(&buf).into_owned()))
}

/// The `<link>` tags of a page, each as its attributes, as html5ever reads
/// them (names lowercased, every entity decoded). The page may be cut short
/// at 512 KiB; the parser takes what is there.
fn link_tags(html: &str) -> Vec<Vec<(String, String)>> {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};

    fn walk(node: &Handle, tags: &mut Vec<Vec<(String, String)>>) {
        if let NodeData::Element { name, attrs, .. } = &node.data {
            if name.local.as_ref() == "link" {
                tags.push(
                    attrs
                        .borrow()
                        .iter()
                        .map(|a| (a.name.local.to_string(), a.value.to_string()))
                        .collect(),
                );
            }
        }
        for child in node.children.borrow().iter() {
            walk(child, tags);
        }
    }
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut tags = Vec::new();
    walk(&dom.document, &mut tags);
    tags
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// The largest edge a `sizes` attribute claims ("32x32 192x192" → 192;
/// "any" or nothing → `None`).
fn largest_size(sizes: Option<&str>) -> Option<u32> {
    sizes?
        .split_whitespace()
        .filter_map(|s| s.split(['x', 'X']).next()?.parse::<u32>().ok())
        .max()
}

fn is_svg(href: &str, mime: Option<&str>) -> bool {
    mime.is_some_and(|t| t.to_ascii_lowercase().contains("svg"))
        || href
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .ends_with(".svg")
}

/// The icon links a page declares, with their claimed (or assumed) sizes.
/// A vector icon counts as 256px: it is rasterised at logo size (see
/// `square_svg`), which beats any bitmap short of the manifest's big ones.
/// `mask-icon`s are monochrome silhouettes, not the brand.
fn link_icons(html: &str, base: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for attrs in link_tags(html) {
        let Some(href) = attr(&attrs, "href")
            .map(str::trim)
            .filter(|h| !h.is_empty())
        else {
            continue;
        };
        let rel = attr(&attrs, "rel").unwrap_or("").to_ascii_lowercase();
        let rels: Vec<&str> = rel.split_whitespace().collect();
        if rels.contains(&"mask-icon") {
            continue;
        }
        let claimed = largest_size(attr(&attrs, "sizes"));
        let size = if is_svg(href, attr(&attrs, "type"))
            && rels
                .iter()
                .any(|r| *r == "icon" || r.starts_with("apple-touch-icon"))
        {
            256
        } else if rels.iter().any(|r| r.starts_with("apple-touch-icon")) {
            claimed.unwrap_or(180)
        } else if rels.contains(&"fluid-icon") {
            claimed.unwrap_or(128)
        } else if rels.contains(&"icon") {
            claimed.unwrap_or(48)
        } else {
            continue;
        };
        if let Some(url) = resolve_url(base, href) {
            out.push((size, url));
        }
    }
    out
}

/// The page's web manifest, if it links one.
fn link_manifest(html: &str, base: &str) -> Option<String> {
    link_tags(html).into_iter().find_map(|attrs| {
        let rel = attr(&attrs, "rel")?.to_ascii_lowercase();
        if !rel.split_whitespace().any(|r| r == "manifest") {
            return None;
        }
        resolve_url(base, attr(&attrs, "href")?.trim())
    })
}

/// The icons a web manifest lists, with their claimed sizes.
fn manifest_icons(json: &str, base: &str) -> Vec<(u32, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    v["icons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|icon| {
            let src = icon["src"].as_str()?.trim();
            if src.is_empty() {
                return None;
            }
            let size = if is_svg(src, icon["type"].as_str()) {
                256
            } else {
                largest_size(icon["sizes"].as_str()).unwrap_or(48)
            };
            Some((size, resolve_url(base, src)?))
        })
        .collect()
}

/// `href` as seen from `base` (an absolute URL), by RFC 3986's rules; only
/// an `http(s)` result counts. Parsed relaxed, since sites write links by
/// hand.
fn resolve_url(base: &str, href: &str) -> Option<String> {
    use gtk::glib::{Uri, UriFlags};
    let url = Uri::resolve_relative(Some(base), href.trim(), UriFlags::PARSE_RELAXED).ok()?;
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")).then(|| url.to_string())
}

pub(super) fn get(url: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .ok()?;
    // Some sites answer every icon path with their home page and a 200 — pm.me
    // sends 347KB of HTML for `/favicon.ico`. Ask what it is before reading it.
    if !is_image_type(resp.content_type()) {
        return None;
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .take(1_000_000)
        .read_to_end(&mut buf)
        .ok()?;
    // And a backstop for the ones that mislabel it.
    (!buf.is_empty() && !looks_like_markup(&buf)).then_some(buf)
}

/// Whether a response claims to be an image. An empty or unknown type is
/// allowed through — plenty of servers send `application/octet-stream` for an
/// `.ico`, and the bytes are checked either way.
fn is_image_type(content_type: &str) -> bool {
    let ct = content_type.trim().to_ascii_lowercase();
    let ct = ct.split(';').next().unwrap_or("").trim();
    ct.is_empty() || ct.starts_with("image/") || ct == "application/octet-stream"
}

/// Whether these bytes are a web page rather than an image — some sites answer a
/// missing icon with their home page and a 200.
fn looks_like_markup(bytes: &[u8]) -> bool {
    let head: String = bytes
        .iter()
        .take(64)
        .map(|b| *b as char)
        .collect::<String>()
        .trim_start()
        .to_ascii_lowercase();
    if looks_like_svg(bytes) {
        return false;
    }
    head.starts_with("<!doctype") || head.starts_with("<html") || head.starts_with("<?xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_icons_are_found_and_ranked_by_size() {
        let html = r##"<html><head>
            <link rel='stylesheet' href='/style.css'>
            <link rel="icon" href="/img/favicon-32x32.png" sizes="32x32" />
            <LINK REL="icon" HREF="//cdn.example.com/icon-192.png?v=2&amp;x=1" SIZES="192x192">
            <link rel="apple-touch-icon" href="touch.png">
            <link rel="icon" type="image/svg+xml" href="/icon.svg">
            <link rel="mask-icon" href="/pin.png" color="#000">
            <link rel="manifest" href="../site.webmanifest">
            </head></html>"##;
        let base = "https://www.example.com/a/b/index.html";
        let icons = link_icons(html, base);
        assert_eq!(
            icons,
            vec![
                (
                    32,
                    "https://www.example.com/img/favicon-32x32.png".to_string()
                ),
                (
                    192,
                    "https://cdn.example.com/icon-192.png?v=2&x=1".to_string()
                ),
                (180, "https://www.example.com/a/b/touch.png".to_string()),
                // A vector icon is rasterised at logo size, so it ranks high.
                (256, "https://www.example.com/icon.svg".to_string()),
            ]
        );
        assert_eq!(
            link_manifest(html, base).as_deref(),
            Some("https://www.example.com/a/site.webmanifest")
        );
        let manifest = r#"{"icons":[{"src":"/i/512.png","sizes":"512x512","type":"image/png"},
                                     {"src":"v.svg","sizes":"any","type":"image/svg+xml"},
                                     {"src":"i/any.png","sizes":"any"}]}"#;
        assert_eq!(
            manifest_icons(manifest, "https://www.example.com/a/site.webmanifest"),
            vec![
                (512, "https://www.example.com/i/512.png".to_string()),
                (256, "https://www.example.com/a/v.svg".to_string()),
                (48, "https://www.example.com/a/i/any.png".to_string()),
            ]
        );
        assert_eq!(largest_size(Some("16x16 48x48 32x32")), Some(48));
        assert_eq!(
            resolve_url("https://example.com", "favicon.ico").as_deref(),
            Some("https://example.com/favicon.ico")
        );
        assert_eq!(
            resolve_url("https://example.com/", "data:image/png;base64,AAAA"),
            None
        );
        let base = "https://www.example.com/a/b/page.html";
        assert_eq!(
            resolve_url(base, "../i.png").as_deref(),
            Some("https://www.example.com/a/i.png")
        );
        assert_eq!(
            resolve_url(base, "./i.png").as_deref(),
            Some("https://www.example.com/a/b/i.png")
        );
        assert_eq!(
            resolve_url(base, "/i.png").as_deref(),
            Some("https://www.example.com/i.png")
        );
        assert_eq!(
            resolve_url(base, "//cdn.example.net/i.png").as_deref(),
            Some("https://cdn.example.net/i.png")
        );
        assert_eq!(resolve_url(base, "mailto:x@example.com"), None);
    }

    #[test]
    fn only_images_are_read() {
        assert!(is_image_type("image/png"));
        assert!(is_image_type("image/vnd.microsoft.icon; charset=utf-8"));
        assert!(is_image_type("image/x-icon"));
        // Servers that don't know what an .ico is get the benefit of the doubt;
        // the bytes are sniffed anyway.
        assert!(is_image_type(""));
        assert!(is_image_type("application/octet-stream"));
        // A home page is not an icon, and is not worth downloading to find out.
        assert!(!is_image_type("text/html; charset=utf-8"));
        assert!(!is_image_type("application/json"));
    }

    #[test]
    fn a_home_page_served_in_place_of_an_icon_is_rejected() {
        assert!(looks_like_markup(b"<!DOCTYPE html><html>"));
        assert!(looks_like_markup(b"  <html lang=\"en\">"));
        // An XML prologue is a page unless an SVG follows it.
        assert!(looks_like_markup(b"<?xml version=\"1.0\"?><html>"));
        assert!(!looks_like_markup(
            b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        ));
        assert!(!looks_like_markup(b"\x89PNG\r\n\x1a\n"));
        assert!(!looks_like_markup(b"\x00\x00\x01\x00"));
    }
}
