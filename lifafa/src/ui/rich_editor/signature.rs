//! Signatures brought in from a file or pasted as source (#120).

/// Convert a stored signature (which may be legacy plain text) to HTML suitable
/// for the editor / for inserting into a message.
pub(crate) fn signature_to_html(sig: &str) -> String {
    if sig.contains('<') {
        sig.to_string()
    } else {
        gtk::glib::markup_escape_text(sig).replace('\n', "<br>")
    }
}

/// A signature brought in from outside (#120): an HTML file, or source
/// pasted into the signature's HTML dialog. Plain text (nothing that looks
/// like a tag) is escaped line by line like a typed signature.
///
/// HTML is the user's own material, but it still goes through the
/// sanitizer: a `<script>` or an event handler in a signature would run in
/// every composer, and a `<style>` block would restyle the whole message.
/// Structure and inline styles survive, since a designed signature is
/// nothing but tables and `style` attributes. An image referenced by a local
/// path is read and embedded as a `data:` URI (a relative path resolves
/// against `base`, the file's own directory), so the send path lifts it into
/// a `cid:` part like any inline picture; a remote image stays a remote URL.
pub(crate) fn signature_from_source(source: &str, base: Option<&std::path::Path>) -> String {
    if !source.contains('<') {
        return signature_to_html(source.trim_end());
    }
    let with_images = inline_local_images(source, base);
    let mut b = ammonia::Builder::default();
    b.add_tags([
        "table", "thead", "tbody", "tfoot", "tr", "td", "th", "font", "center", "u", "span", "img",
    ])
    .add_generic_attributes([
        "style",
        "width",
        "height",
        "align",
        "valign",
        "bgcolor",
        "border",
        "cellpadding",
        "cellspacing",
        "color",
        "face",
        "size",
    ])
    .add_tag_attributes("img", ["src", "alt", "width", "height"])
    .url_schemes(std::collections::HashSet::from([
        "http", "https", "mailto", "tel", "data",
    ]))
    .link_rel(None);
    b.clean(&with_images).to_string().trim().to_string()
}

/// Every `src="…"` that names a local image file, replaced by a `data:` URI
/// of the file's bytes. URLs (`http:`, `data:`, `cid:`, …) and anything that
/// is not a readable image of a sensible size are left exactly as written.
fn inline_local_images(html: &str, base: Option<&std::path::Path>) -> String {
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = find_src_attr(rest) {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        // `tail` starts at `src=`; the value is the quoted run after it.
        let Some((prefix, value)) = quoted_value(tail) else {
            out.push_str(&tail[..4]);
            rest = &tail[4..];
            continue;
        };
        out.push_str(prefix);
        match local_image_data_uri(value, base, MAX_BYTES) {
            Some(uri) => out.push_str(&uri),
            None => out.push_str(value),
        }
        // The closing quote is the first char of `rest`, kept as it was.
        rest = &tail[prefix.len() + value.len()..];
    }
    out.push_str(rest);
    out
}

/// Byte offset of the next ` src=` attribute (any case), or `None`.
pub(super) fn find_src_attr(s: &str) -> Option<usize> {
    let lower = s.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("src=") {
        let at = from + i;
        let preceded_by_space = at > 0 && lower.as_bytes()[at - 1].is_ascii_whitespace();
        if preceded_by_space {
            return Some(at);
        }
        from = at + 4;
    }
    None
}

/// Split `src="value"…` into (`src="`, `value`). `None` when the value is
/// unquoted or unterminated.
pub(super) fn quoted_value(s: &str) -> Option<(&str, &str)> {
    let after = &s[4..];
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &after[quote.len_utf8()..];
    let end = value.find(quote)?;
    Some((&s[..4 + quote.len_utf8()], &value[..end]))
}

/// The `data:` URI for a `src` value that names a local image, else `None`.
fn local_image_data_uri(value: &str, base: Option<&std::path::Path>, max: u64) -> Option<String> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    let path = if let Some(rest) = lower.strip_prefix("file://") {
        std::path::PathBuf::from(crate::encoding::percent::decode(
            &v[v.len() - rest.len()..],
            false,
        ))
    } else if lower.contains(':') && !lower.starts_with('/') && !lower.starts_with('.') {
        // Some other scheme (http, https, data, cid, mailto…): not ours.
        return None;
    } else {
        let p = std::path::PathBuf::from(crate::encoding::percent::decode(v, false));
        if p.is_absolute() {
            p
        } else {
            base?.join(p)
        }
    };
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => return None,
    };
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > max {
        return None;
    }
    let data = std::fs::read(&path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        crate::encoding::base64_encode(&data)
    ))
}

#[cfg(test)]
mod signature_tests {
    use super::super::documents::{image_policy_meta, remote_image_urls};
    use super::*;

    /// A reply to a message whose remote content is blocked still shows the
    /// user's own signature logo, and nothing else from the web (#295).
    #[test]
    fn blocked_images_spare_the_signature() {
        let sig = r#"<p>Jo</p><img src="https://cdn.example/logo.png?v=2&amp;x=1" alt="">
            <img src="data:image/png;base64,AAAA">"#;
        let urls = remote_image_urls(sig);
        assert_eq!(
            urls,
            vec!["https://cdn.example/logo.png?v=2&x=1".to_string()]
        );
        assert_eq!(
            image_policy_meta(Some(&urls)),
            "<meta http-equiv=\"Content-Security-Policy\" \
             content=\"img-src data: blob: https://cdn.example/logo.png\">"
        );
        assert_eq!(
            image_policy_meta(Some(&["https://x.example/a;b".to_string()]))
                .matches("x.example")
                .count(),
            0
        );
        assert_eq!(image_policy_meta(None), "");
    }

    /// Plain text stays a typed signature: escaped, line breaks kept.
    #[test]
    fn plain_text_is_escaped_like_a_typed_signature() {
        assert_eq!(
            signature_from_source("Ada & Co\nCEO\n", None),
            "Ada &amp; Co<br>CEO"
        );
    }

    /// The parts a designed signature is made of survive; what could run or
    /// restyle the message does not.
    #[test]
    fn html_keeps_structure_and_styles_but_not_scripts() {
        let src = "<html><head><style>body{color:red}</style></head><body>\
                   <table style=\"border:0\"><tr><td style=\"color:#333\" onclick=\"x()\">\
                   <b>Ada</b> <a href=\"https://example.com\">site</a></td></tr></table>\
                   <script>alert(1)</script></body></html>";
        let out = signature_from_source(src, None);
        assert!(out.contains("<table style=\"border:0\">"), "{out}");
        assert!(out.contains("<td style=\"color:#333\">"), "{out}");
        assert!(
            out.contains("<a href=\"https://example.com\">site</a>"),
            "{out}"
        );
        assert!(!out.contains("onclick"), "{out}");
        assert!(!out.contains("script"), "{out}");
        assert!(!out.contains("color:red"), "{out}");
    }

    /// A local image next to the file is embedded; a remote one is left alone.
    #[test]
    fn local_images_are_embedded_and_remote_ones_kept() {
        let dir = std::env::temp_dir().join(format!("hylki-sig-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("logo.png"), [0x89, b'P', b'N', b'G']).unwrap();
        let src = "<p><img src=\"logo.png\" width=\"80\"> <img src='https://x.example/a.png'>\
                   <img src=\"missing.png\"></p>";
        let out = signature_from_source(src, Some(&dir));
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            out.contains("src=\"data:image/png;base64,iVBORw==\""),
            "{out}"
        );
        assert!(out.contains("width=\"80\""), "{out}");
        assert!(out.contains("src=\"https://x.example/a.png\""), "{out}");
        // A path that resolves to nothing is left as written.
        assert!(out.contains("src=\"missing.png\""), "{out}");
    }
}
