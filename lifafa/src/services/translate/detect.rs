//! Telling a message's language offline, from its visible words, so the
//! banner is offered only when it differs from the reader's.

use super::base;

/// Which language a message's text is in, when that can be told with
/// confidence: one of this module's codes, or `None`. Runs offline.
pub(crate) fn detect(html: &str) -> Option<String> {
    let text = visible_text(html);
    // A line or two is too little to go on, and a long newsletter is
    // decided well before its end.
    let text: String = text.chars().take(4000).collect();
    if text.chars().filter(|c| c.is_alphabetic()).count() < 40 {
        return None;
    }
    let info = whatlang::detect(&text)?;
    if !info.is_reliable() {
        return None;
    }
    let code = match info.lang().code() {
        "ara" => "ar",
        "bul" => "bg",
        "cat" => "ca",
        "ces" => "cs",
        "cmn" => "zh-Hans",
        "dan" => "da",
        "deu" => "de",
        "ell" => "el",
        "eng" => "en",
        "est" => "et",
        "fin" => "fi",
        "fra" => "fr",
        "heb" => "he",
        "hin" => "hi",
        "hrv" => "hr",
        "hun" => "hu",
        "ind" => "id",
        "ita" => "it",
        "jpn" => "ja",
        "kor" => "ko",
        "lav" => "lv",
        "lit" => "lt",
        "nld" => "nl",
        "nob" => "nb",
        "pol" => "pl",
        "por" => "pt",
        "ron" => "ro",
        "rus" => "ru",
        "slk" => "sk",
        "slv" => "sl",
        "spa" => "es",
        "srp" => "sr",
        "swe" => "sv",
        "tha" => "th",
        "tur" => "tr",
        "ukr" => "uk",
        "vie" => "vi",
        other => other,
    };
    Some(code.to_string())
}

/// Whether a message in `detected` is worth offering to translate into
/// `target`: a different language, not a variety of the same one.
pub(crate) fn differs(detected: &str, target: &str) -> bool {
    base(detected) != base(target)
}

/// The words of a fragment of HTML, roughly: tags dropped, the common
/// entities decoded, quoted text left out. Only good enough for telling
/// the language, which is all it is used for.
pub(super) fn visible_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut tag = String::new();
    let mut skip_depth = 0usize;
    for c in html.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let t = tag.trim().to_ascii_lowercase();
                let name: String = t
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if matches!(name.as_str(), "blockquote" | "style" | "script") {
                    if t.starts_with('/') {
                        skip_depth = skip_depth.saturating_sub(1);
                    } else {
                        skip_depth += 1;
                    }
                }
                out.push(' ');
                tag.clear();
            } else {
                tag.push(c);
            }
        } else if c == '<' {
            in_tag = true;
        } else if skip_depth == 0 {
            out.push(c);
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    // Plain-text replies quote with "> " lines.
    out.lines()
        .filter(|l| !l.trim_start().starts_with('>'))
        .collect::<Vec<_>>()
        .join("\n")
}
