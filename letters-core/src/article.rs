// article.rs — import a saved web article (HTML file) as an editable Document.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// An explicit, local-only import (#1787). The page is never *rendered* and
// nothing is fetched: this module has no network code and never will. The
// HTML is tokenised by a small tolerant tokenizer and walked against an
// allowlist; only text, structure (headings, lists, quotes, code), inline
// emphasis, http/https/mailto links and local images that sit beside the
// saved file reach the Document. Scripts, styles, frames, forms, SVG and
// everything else on the deny list are dropped and counted, so the caller
// can say what was left out.
//
// The sanitizer here is written for this model rather than adapted from any
// third-party code (the Frostread reference was studied, not copied).
//
// Source attribution — title, byline, publication date and the page's URL —
// is read from `<title>`, Open Graph / article meta tags, `<link
// rel=canonical>`, `<time datetime>` and the "saved from url=" comment that
// browsers write, and becomes the first paragraphs of the document.

use std::path::{Path, PathBuf};

use crate::model::{Document, ListKind, ParaStyle, Paragraph, Run, RunStyle};
use suite_common_core::document::VertAlign;

/// Largest HTML file read.
pub const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;
/// Largest single local image kept.
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
/// Most local images kept from one article.
pub const MAX_IMAGES: usize = 100;
/// Most local image bytes kept from one article, in total.
pub const MAX_TOTAL_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
/// Deepest element nesting accepted.
pub const MAX_DEPTH: usize = 256;
/// Most paragraphs produced.
pub const MAX_PARAGRAPHS: usize = 50_000;

/// What an import produced.
#[derive(Debug, Clone)]
pub struct ArticleImport {
    /// The new document: attribution paragraphs, then the article body.
    pub document: Document,
    /// The article's title, when the page named one.
    pub title: Option<String>,
    pub author: Option<String>,
    pub published: Option<String>,
    /// The page's own URL (http/https only), when the file records it.
    pub source_url: Option<String>,
    /// What was left out or could not be kept, one line each.
    pub notes: Vec<String>,
}

/// Import the saved HTML file at `path`. Nothing outside the returned value
/// is touched, so a failure leaves any open document as it was.
pub fn import_file(path: &Path) -> Result<ArticleImport, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "html" | "htm" | "xhtml") {
        return Err(format!("\"{name}\" is not a saved web page. Choose a .html, .htm or .xhtml file."));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("Cannot read {name}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("\"{name}\" is not a regular file."));
    }
    if meta.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "\"{name}\" is {} MiB, larger than the {} MiB limit for article import.",
            meta.len() / (1024 * 1024),
            MAX_INPUT_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {name}: {e}"))?;
    // Re-check: the file may have grown between the stat and the read.
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!("\"{name}\" is larger than the {} MiB limit for article import.", MAX_INPUT_BYTES / (1024 * 1024)));
    }
    let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    import_bytes(&bytes, Some(base), &name)
}

/// Import HTML held in memory. `asset_base` is the directory local images
/// are resolved in (and confined to); with `None` no image is kept.
pub fn import_bytes(bytes: &[u8], asset_base: Option<&Path>, name: &str) -> Result<ArticleImport, String> {
    if bytes.iter().take(4096).any(|&b| b == 0) {
        return Err(format!("\"{name}\" is a binary file, not a saved web page."));
    }
    let lossy = String::from_utf8_lossy(bytes);
    let replaced = lossy.contains('\u{FFFD}') && !bytes.windows(3).any(|w| w == [0xEF, 0xBF, 0xBD]);
    let html: &str = &lossy;

    let meta = scan_metadata(html);
    let mut notes = Vec::new();
    if replaced {
        notes.push("Some bytes were not valid UTF-8 and were replaced; check the text for garbled characters.".to_string());
    }

    let base = asset_base.and_then(|b| b.canonicalize().ok());
    let mut walk = Walker::new(&meta, base);
    walk.run(html)?;
    let Walker { mut paras, counts, .. } = walk;

    let chars: usize = paras.iter().flat_map(|p| &p.runs).map(|r| r.text.chars().count()).sum();
    if paras.is_empty() || chars == 0 {
        return Err(format!(
            "No readable article text found in \"{name}\". Letters imports saved web articles; \
             this file has no text outside scripts, menus and other page furniture."
        ));
    }

    // The leading attribution block.
    let title = meta.title();
    let mut head: Vec<Paragraph> = Vec::new();
    if let Some(t) = &title {
        let dup = paras.first().is_some_and(|p| {
            p.style.heading == Some(1) && normalize_ws(&p.runs.iter().map(|r| r.text.as_str()).collect::<String>()) == normalize_ws(t)
        });
        if dup {
            // The page's own h1 is the title: keep it first, attribution after.
            head.push(paras.remove(0));
        } else {
            head.push(Paragraph {
                style: ParaStyle { heading: Some(1), ..Default::default() },
                runs: vec![Run::plain(t.clone())],
            });
        }
    }
    let mut byline = String::new();
    if let Some(a) = &meta.author {
        byline.push_str(&format!("By {a}"));
    }
    if let Some(d) = &meta.published {
        if !byline.is_empty() {
            byline.push_str(" — ");
        }
        byline.push_str(&format!("Published {d}"));
    }
    if !byline.is_empty() {
        head.push(Paragraph {
            style: ParaStyle::default(),
            runs: vec![Run { text: byline, style: RunStyle { italic: true, ..Default::default() } }],
        });
    }
    let source_url = meta.source_url();
    let mut source = vec![Run::plain("Source: ")];
    match &source_url {
        Some(u) => source.push(Run { text: u.clone(), style: RunStyle { link: Some(u.clone()), ..Default::default() } }),
        None => source.push(Run::plain(format!("saved file {name} (the page's address was not recorded)"))),
    }
    if let Some(s) = &meta.site {
        source.push(Run::plain(format!(" ({s})")));
    }
    head.push(Paragraph { style: ParaStyle::default(), runs: source });
    head.append(&mut paras);

    if head.len() >= MAX_PARAGRAPHS {
        head.truncate(MAX_PARAGRAPHS);
        notes.push(format!("The article is very long; only the first {MAX_PARAGRAPHS} paragraphs were imported."));
    }

    counts.append_notes(&mut notes);

    let mut document = Document::new();
    document.paragraphs = head;
    Ok(ArticleImport { document, title, author: meta.author.clone(), published: meta.published.clone(), source_url, notes })
}

// ── Metadata ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct Meta {
    html_title: Option<String>,
    og_title: Option<String>,
    author: Option<String>,
    published: Option<String>,
    og_url: Option<String>,
    canonical: Option<String>,
    saved_from: Option<String>,
    site: Option<String>,
    has_article: bool,
    has_main: bool,
    has_body: bool,
}

impl Meta {
    fn title(&self) -> Option<String> {
        self.og_title.clone().or_else(|| self.html_title.clone())
    }
    fn source_url(&self) -> Option<String> {
        [&self.canonical, &self.og_url, &self.saved_from].into_iter().flatten().find_map(|u| web_url(u))
    }
}

fn clean(s: &str, max: usize) -> Option<String> {
    let s: String = s.chars().filter(|c| !c.is_control() || c.is_whitespace()).collect();
    let s = normalize_ws(&s);
    if s.is_empty() {
        None
    } else {
        Some(s.chars().take(max).collect())
    }
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `Some(url)` only for a plain http/https address.
fn web_url(u: &str) -> Option<String> {
    let u: String = u.chars().filter(|c| !c.is_control()).collect();
    let u = u.trim();
    let lower = u.to_ascii_lowercase();
    if (lower.starts_with("http://") || lower.starts_with("https://")) && u.len() <= 2048 && !u.contains(char::is_whitespace) {
        Some(u.to_string())
    } else {
        None
    }
}

fn scan_metadata(html: &str) -> Meta {
    let mut m = Meta::default();
    let mut in_title = false;
    let mut title_buf = String::new();
    let mut time_seen: Option<String> = None;
    for tok in Tokenizer::new(html) {
        match tok {
            Tok::Comment(c) => {
                // Chrome / SingleFile: "saved from url=(0021)https://…"
                if m.saved_from.is_none() {
                    if let Some(rest) = c.trim().strip_prefix("saved from url=") {
                        let rest = rest.trim_start_matches(|c: char| c != ')').trim_start_matches(')');
                        m.saved_from = Some(rest.trim().to_string());
                    }
                }
            }
            Tok::Start { name, attrs, .. } => match name.as_str() {
                "title" => in_title = m.html_title.is_none(),
                "article" => m.has_article = true,
                "main" => m.has_main = true,
                "body" => m.has_body = true,
                "meta" => {
                    let key = attr(&attrs, "property").or_else(|| attr(&attrs, "name")).map(|s| s.to_ascii_lowercase());
                    let content = attr(&attrs, "content");
                    if let (Some(key), Some(content)) = (key, content) {
                        match key.as_str() {
                            "og:title" | "twitter:title" => {
                                m.og_title = m.og_title.take().or_else(|| clean(content, 300));
                            }
                            "author" | "article:author" | "dc.creator" | "parsely-author" => {
                                // article:author is sometimes a profile URL.
                                if web_url(content).is_none() {
                                    m.author = m.author.take().or_else(|| clean(content, 200));
                                }
                            }
                            "article:published_time" | "date" | "pubdate" | "dc.date" | "dc.date.issued" | "og:article:published_time" => {
                                m.published = m.published.take().or_else(|| clean(content, 64));
                            }
                            "og:url" => m.og_url = m.og_url.take().or_else(|| Some(content.to_string())),
                            "og:site_name" => m.site = m.site.take().or_else(|| clean(content, 100)),
                            _ => {}
                        }
                    }
                }
                "link" => {
                    if attr(&attrs, "rel").is_some_and(|r| r.split_whitespace().any(|t| t.eq_ignore_ascii_case("canonical"))) {
                        if let Some(h) = attr(&attrs, "href") {
                            m.canonical = m.canonical.take().or_else(|| Some(h.to_string()));
                        }
                    }
                }
                "time" if time_seen.is_none() => {
                    time_seen = attr(&attrs, "datetime").and_then(|d| clean(d, 64));
                }
                _ => {}
            },
            Tok::End(name) if name == "title" => {
                if in_title {
                    m.html_title = clean(&title_buf, 300);
                }
                in_title = false;
            }
            Tok::Text(t) if in_title => title_buf.push_str(&decode_entities(t)),
            _ => {}
        }
    }
    if m.published.is_none() {
        m.published = time_seen;
    }
    m
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

// ── Tokenizer ────────────────────────────────────────────────────────────

enum Tok<'a> {
    Text(&'a str),
    Comment(&'a str),
    Start { name: String, attrs: Vec<(String, String)>, self_closing: bool },
    End(String),
}

struct Tokenizer<'a> {
    s: &'a str,
    pos: usize,
    pending_end: Option<String>,
    pending_raw: Option<&'a str>,
}

impl<'a> Tokenizer<'a> {
    fn new(s: &'a str) -> Self {
        Self { s, pos: 0, pending_end: None, pending_raw: None }
    }
}

fn find_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Tok<'a>;

    fn next(&mut self) -> Option<Tok<'a>> {
        if let Some(raw) = self.pending_raw.take() {
            if !raw.is_empty() {
                return Some(Tok::Text(raw));
            }
        }
        if let Some(n) = self.pending_end.take() {
            return Some(Tok::End(n));
        }
        let s = self.s;
        if self.pos >= s.len() {
            return None;
        }
        let rest = &s[self.pos..];
        if !rest.starts_with('<') {
            let end = rest.find('<').unwrap_or(rest.len());
            self.pos += end;
            return Some(Tok::Text(&rest[..end]));
        }
        if let Some(r) = rest.strip_prefix("<!--") {
            return Some(match r.find("-->") {
                Some(i) => {
                    self.pos += 4 + i + 3;
                    Tok::Comment(&r[..i])
                }
                None => {
                    self.pos = s.len();
                    Tok::Comment("")
                }
            });
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").map(|i| i + 3).unwrap_or(rest.len());
            self.pos += end;
            return self.next();
        }
        let b = rest.as_bytes();
        if b.len() > 1 && (b[1] == b'!' || b[1] == b'?') {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            self.pos += end;
            return self.next();
        }
        if b.len() > 2 && b[1] == b'/' && b[2].is_ascii_alphabetic() {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            let inner = &rest[2..end.saturating_sub(1).max(2)];
            let name: String = inner.chars().take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>').collect();
            self.pos += end;
            return Some(Tok::End(name.to_ascii_lowercase()));
        }
        if b.len() > 1 && b[1].is_ascii_alphabetic() {
            return Some(self.start_tag());
        }
        // A bare '<' is text.
        self.pos += 1;
        Some(Tok::Text("<"))
    }
}

impl<'a> Tokenizer<'a> {
    fn start_tag(&mut self) -> Tok<'a> {
        let s = self.s;
        let b = s.as_bytes();
        let mut i = self.pos + 1;
        let name_start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'/' && b[i] != b'>' {
            i += 1;
        }
        let name = s[name_start..i].to_ascii_lowercase();
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut self_closing = false;
        loop {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() {
                break;
            }
            if b[i] == b'>' {
                i += 1;
                break;
            }
            if b[i] == b'/' {
                if i + 1 < b.len() && b[i + 1] == b'>' {
                    self_closing = true;
                    i += 2;
                    break;
                }
                i += 1;
                continue;
            }
            let ks = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' && !(b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'>') {
                i += 1;
            }
            let key = s[ks..i].to_ascii_lowercase();
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let mut val = String::new();
            if i < b.len() && b[i] == b'=' {
                i += 1;
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                    let q = b[i];
                    i += 1;
                    let vs = i;
                    while i < b.len() && b[i] != q {
                        i += 1;
                    }
                    val = decode_entities(&s[vs..i]);
                    i = (i + 1).min(b.len());
                } else {
                    let vs = i;
                    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                        i += 1;
                    }
                    val = decode_entities(&s[vs..i]);
                }
            }
            if !key.is_empty() && !attrs.iter().any(|(k, _)| *k == key) && attrs.len() < 64 {
                attrs.push((key, val));
            }
        }
        // `i` may sit mid-char only if the tag was cut off by EOF; clamp to a boundary.
        while i < s.len() && !s.is_char_boundary(i) {
            i += 1;
        }
        self.pos = i.min(s.len());
        // Raw-text elements: their content is never markup.
        if matches!(name.as_str(), "script" | "style" | "textarea" | "xmp" | "noembed" | "noframes") && !self_closing {
            let close = format!("</{name}");
            let end = find_ci(s, &close, self.pos).unwrap_or(s.len());
            let raw = &s[self.pos..end];
            self.pos = end;
            // Leave the close tag to be tokenised normally.
            self.pending_raw = Some(raw);
        }
        Tok::Start { name, attrs, self_closing }
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let semi = rest[1..].find(';').filter(|&j| j <= 32);
        if let Some(j) = semi {
            let ent = &rest[1..1 + j];
            if let Some(c) = entity_char(ent) {
                out.push(c);
                rest = &rest[j + 2..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

fn entity_char(ent: &str) -> Option<char> {
    if let Some(num) = ent.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        let c = char::from_u32(code)?;
        return if c == '\0' || (c.is_control() && !c.is_whitespace()) { None } else { Some(c) };
    }
    Some(match ent {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{A0}',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "bull" => '•',
        "middot" => '·',
        "times" => '×',
        "deg" => '°',
        "euro" => '€',
        "pound" => '£',
        _ => return None,
    })
}

// ── Walker ───────────────────────────────────────────────────────────────

const SKIP: &[&str] = &[
    "head", "title", "script", "style", "noscript", "iframe", "frame", "frameset", "object", "embed", "applet", "form", "button",
    "select", "textarea", "input", "nav", "aside", "footer", "svg", "math", "canvas", "template", "audio", "video", "dialog", "map",
    "xmp", "noembed", "noframes", "datalist", "option",
];
const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
const BLOCK: &[&str] = &[
    "p", "div", "section", "article", "main", "header", "figure", "figcaption", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li",
    "blockquote", "pre", "table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "dl", "dt", "dd", "address", "details",
    "summary", "fieldset", "hgroup", "body", "html", "center",
];

#[derive(Default)]
struct Counts {
    scripts: usize,
    frames: usize,
    forms: usize,
    styles: usize,
    media: usize,
    graphics: usize,
    remote_images: usize,
    unusable_images: usize,
    links_dropped: usize,
    tables: usize,
    image_cap: bool,
}

impl Counts {
    fn append_notes(&self, notes: &mut Vec<String>) {
        let mut add = |n: usize, text: &str| {
            if n > 0 {
                notes.push(format!("{n} {text}"));
            }
        };
        add(self.scripts, "script(s) were not imported or run.");
        add(self.frames, "embedded frame/object(s) were left out.");
        add(self.forms, "form control(s) were left out.");
        add(self.styles, "style sheet(s) were ignored; the article uses Letters' own styles.");
        add(self.media, "audio/video element(s) were left out.");
        add(self.graphics, "SVG/canvas graphic(s) were left out.");
        add(self.remote_images, "image(s) point to the web and were not downloaded; their descriptions were kept.");
        add(self.unusable_images, "image(s) were not kept (missing, outside the saved page's folder, too large or not PNG/JPEG/GIF/WebP).");
        add(self.links_dropped, "link(s) were kept as plain text (relative, or not http/https/mailto).");
        add(self.tables, "table(s) were flattened to paragraphs.");
        if self.image_cap {
            notes.push(format!("Only the first {MAX_IMAGES} local images were kept."));
        }
    }
}

struct Frame {
    name: String,
    skip: bool,
    href: Option<String>,
}

struct Walker<'m> {
    meta: &'m Meta,
    base: Option<PathBuf>,
    stack: Vec<Frame>,
    skip_depth: usize,
    region: Option<&'static str>,
    region_state: u8, // 0 not entered, 1 inside, 2 done
    region_depth: usize,
    runs: Vec<Run>,
    paras: Vec<Paragraph>,
    counts: Counts,
    images: usize,
    image_bytes: u64,
    table_seen: bool,
    pre_depth: usize,
}

impl<'m> Walker<'m> {
    fn new(meta: &'m Meta, base: Option<PathBuf>) -> Self {
        let region = if meta.has_article {
            Some("article")
        } else if meta.has_main {
            Some("main")
        } else if meta.has_body {
            Some("body")
        } else {
            None
        };
        Self {
            meta,
            base,
            stack: Vec::new(),
            skip_depth: 0,
            region,
            region_state: if region.is_none() { 1 } else { 0 },
            region_depth: 0,
            runs: Vec::new(),
            paras: Vec::new(),
            counts: Counts::default(),
            images: 0,
            image_bytes: 0,
            table_seen: false,
            pre_depth: 0,
        }
    }

    fn live(&self) -> bool {
        self.region_state == 1 && self.skip_depth == 0
    }

    fn run(&mut self, html: &str) -> Result<(), String> {
        let _ = self.meta;
        for tok in Tokenizer::new(html) {
            if self.paras.len() >= MAX_PARAGRAPHS {
                break;
            }
            match tok {
                Tok::Text(t) => {
                    if self.live() {
                        let text = decode_entities(t);
                        self.text(&text);
                    }
                }
                Tok::Comment(_) => {}
                Tok::Start { name, attrs, self_closing } => self.start(name, attrs, self_closing)?,
                Tok::End(name) => self.end(&name),
            }
        }
        self.flush();
        Ok(())
    }

    fn start(&mut self, name: String, attrs: Vec<(String, String)>, self_closing: bool) -> Result<(), String> {
        // Implied end tags for the common unclosed elements.
        match name.as_str() {
            "p" | "li" | "dt" | "dd" | "tr" | "td" | "th" => self.imply_close(&name),
            _ => {}
        }
        let void = VOID.contains(&name.as_str());
        let hidden = attr(&attrs, "hidden").is_some()
            || attr(&attrs, "aria-hidden").is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || attr(&attrs, "style").is_some_and(|v| {
                let v: String = v.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
                v.contains("display:none") || v.contains("visibility:hidden")
            });
        let skip_name = SKIP.contains(&name.as_str());

        // Region entry (first <article>/<main>/<body>).
        if self.region_state == 0 && self.region == Some(name.as_str()) && self.skip_depth == 0 {
            self.region_state = 1;
            self.region_depth = self.stack.len();
        }

        if self.region_state == 1 && self.skip_depth == 0 {
            match name.as_str() {
                "script" => self.counts.scripts += 1,
                "iframe" | "frame" | "frameset" | "object" | "embed" | "applet" => self.counts.frames += 1,
                "form" | "button" | "select" | "textarea" | "input" => self.counts.forms += 1,
                "style" => self.counts.styles += 1,
                "audio" | "video" => self.counts.media += 1,
                "svg" | "canvas" | "math" => self.counts.graphics += 1,
                "link" if attr(&attrs, "rel").is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet")) => self.counts.styles += 1,
                "table" => {
                    if !self.table_seen {
                        self.table_seen = true;
                    }
                    self.counts.tables += 1;
                }
                _ => {}
            }
        }

        if self.live() {
            if BLOCK.contains(&name.as_str()) {
                self.flush();
            }
            match name.as_str() {
                "br" => self.flush(),
                "hr" => self.flush(),
                "img" => self.image(&attrs),
                _ => {}
            }
        }

        if void || self_closing && !skip_name {
            return Ok(());
        }
        if self_closing {
            return Ok(());
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(format!("The page nests elements more than {MAX_DEPTH} levels deep; it is not a normal article."));
        }
        let skip = skip_name || hidden;
        if skip {
            self.skip_depth += 1;
        }
        let href = if name == "a" { attr(&attrs, "href").map(|h| h.to_string()) } else { None };
        if name == "pre" {
            self.pre_depth += 1;
        }
        self.stack.push(Frame { name, skip, href });
        Ok(())
    }

    fn imply_close(&mut self, name: &str) {
        let group: &[&str] = match name {
            "p" => &["p"],
            "li" => &["li"],
            "dt" | "dd" => &["dt", "dd"],
            "td" | "th" => &["td", "th"],
            _ => &["tr", "td", "th"],
        };
        // Close only if the nearest list/table container has such an open item.
        for i in (0..self.stack.len()).rev() {
            let n = self.stack[i].name.as_str();
            if group.contains(&n) {
                if self.live() {
                    self.flush();
                }
                self.truncate_stack(i);
                return;
            }
            if matches!(n, "ul" | "ol" | "dl" | "table" | "div" | "section" | "article" | "blockquote" | "body") {
                return;
            }
        }
    }

    fn truncate_stack(&mut self, len: usize) {
        while self.stack.len() > len {
            let f = self.stack.pop().unwrap();
            if f.skip {
                self.skip_depth -= 1;
            }
            if f.name == "pre" {
                self.pre_depth -= 1;
            }
        }
        if self.region_state == 1 && self.stack.len() <= self.region_depth && self.region.is_some() && len <= self.region_depth {
            self.region_state = 2;
        }
    }

    fn end(&mut self, name: &str) {
        let Some(i) = self.stack.iter().rposition(|f| f.name == name) else { return };
        if self.live() && (BLOCK.contains(&name) || name == "br") {
            self.flush();
        }
        // A skipped ancestor's end must not be swallowed by an inner mismatch.
        self.truncate_stack(i);
    }

    fn style_now(&self) -> RunStyle {
        let mut st = RunStyle::default();
        for f in &self.stack {
            match f.name.as_str() {
                "b" | "strong" => st.bold = true,
                "i" | "em" | "cite" | "dfn" | "var" => st.italic = true,
                "u" | "ins" => st.underline = true,
                "s" | "strike" | "del" => st.strikethrough = true,
                "code" | "kbd" | "samp" | "tt" => st.code = true,
                "mark" => st.highlight = true,
                "sup" => st.vert_align = Some(VertAlign::Superscript),
                "sub" => st.vert_align = Some(VertAlign::Subscript),
                "a" => {
                    if let Some(h) = &f.href {
                        st.link = safe_href(h);
                    }
                }
                _ => {}
            }
        }
        st
    }

    fn text(&mut self, text: &str) {
        if self.pre_depth > 0 {
            let style = self.style_now();
            let mut first = true;
            for line in text.replace("\r\n", "\n").split('\n') {
                if !first {
                    self.flush();
                }
                first = false;
                if !line.is_empty() {
                    self.runs.push(Run { text: line.to_string(), style: style.clone() });
                }
            }
            return;
        }
        let style = self.style_now();
        // Count dropped links once per text node that would have been linked.
        if self.stack.iter().any(|f| f.name == "a" && f.href.as_deref().is_some_and(|h| safe_href(h).is_none())) && !text.trim().is_empty() {
            self.counts.links_dropped += 1;
        }
        let mut buf = String::with_capacity(text.len());
        let mut prev_space = self.runs.last().map(|r| r.text.ends_with(' ')).unwrap_or(true);
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{A0}' {
                if !prev_space {
                    buf.push(' ');
                    prev_space = true;
                }
            } else if c.is_control() {
                continue;
            } else {
                buf.push(c);
                prev_space = false;
            }
        }
        if buf.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some(last) if last.style == style && last.style.image.is_none() => last.text.push_str(&buf),
            _ => self.runs.push(Run { text: buf, style }),
        }
    }

    fn image(&mut self, attrs: &[(String, String)]) {
        let alt = attr(attrs, "alt").and_then(|a| clean(a, 300)).unwrap_or_default();
        let src = attr(attrs, "src").unwrap_or("").trim();
        let mut kept = None;
        if !src.is_empty() {
            if has_scheme(src) || src.starts_with("//") {
                self.counts.remote_images += 1;
            } else if self.images >= MAX_IMAGES {
                self.counts.image_cap = true;
            } else if let Some(p) = self.local_image(src) {
                kept = Some(p);
            } else {
                self.counts.unusable_images += 1;
            }
        }
        match kept {
            Some(path) => {
                self.images += 1;
                let mut style = self.style_now();
                style.image = Some(path);
                self.runs.push(Run { text: alt, style });
            }
            None if !alt.is_empty() => {
                let st = self.style_now();
                self.runs.push(Run { text: alt, style: st });
            }
            None => {}
        }
    }

    /// A local image: inside the saved page's folder, a real PNG/JPEG/GIF/WebP
    /// within the size limits. Returns its canonical path.
    fn local_image(&mut self, src: &str) -> Option<String> {
        let base = self.base.as_ref()?;
        let rel = src.split(['?', '#']).next().unwrap_or("");
        let rel = percent_decode(rel)?;
        if rel.is_empty() || rel.contains('\0') || rel.starts_with('/') || rel.contains('\\') {
            return None;
        }
        let full = base.join(&rel).canonicalize().ok()?;
        if !full.starts_with(base) {
            return None;
        }
        let meta = std::fs::metadata(&full).ok()?;
        if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES || self.image_bytes + meta.len() > MAX_TOTAL_IMAGE_BYTES {
            return None;
        }
        let mut head = [0u8; 12];
        use std::io::Read;
        let n = std::fs::File::open(&full).ok()?.read(&mut head).ok()?;
        let h = &head[..n];
        let ok = h.starts_with(&[0x89, b'P', b'N', b'G'])
            || h.starts_with(&[0xFF, 0xD8, 0xFF])
            || h.starts_with(b"GIF8")
            || (h.len() >= 12 && &h[..4] == b"RIFF" && &h[8..12] == b"WEBP");
        if !ok {
            return None;
        }
        self.image_bytes += meta.len();
        Some(full.to_string_lossy().to_string())
    }

    /// End the current paragraph, giving it the style of the element it sits in.
    fn flush(&mut self) {
        let mut runs = std::mem::take(&mut self.runs);
        if self.pre_depth == 0 {
            while runs.last().is_some_and(|r| r.style.image.is_none() && r.text.ends_with(' ')) {
                let r = runs.last_mut().unwrap();
                r.text.pop();
                if r.text.is_empty() {
                    runs.pop();
                }
            }
            if let Some(first) = runs.first_mut() {
                if first.style.image.is_none() {
                    first.text = first.text.trim_start().to_string();
                }
            }
        }
        runs.retain(|r| !r.text.is_empty() || r.style.image.is_some());
        if runs.is_empty() {
            return;
        }
        let mut style = ParaStyle::default();
        let mut list_depth = 0u8;
        let mut in_list_item = false;
        for f in &self.stack {
            match f.name.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => style.heading = Some(f.name.as_bytes()[1] - b'0'),
                "ul" | "ol" => {
                    list_depth = list_depth.saturating_add(1);
                    style.list = if f.name == "ol" { ListKind::Numbered } else { ListKind::Bullet };
                }
                "li" => in_list_item = true,
                "pre" => style.code_block = Some(String::new()),
                "blockquote" => style.block_quote = true,
                _ => {}
            }
        }
        if !in_list_item || style.heading.is_some() {
            style.list = ListKind::None;
        } else {
            style.list_level = list_depth.saturating_sub(1).min(8);
        }
        if style.list != ListKind::None {
            // Quotes inside list items stay plain, as the Markdown reader does.
            style.block_quote = false;
        }
        self.paras.push(Paragraph { style, runs });
    }
}

fn has_scheme(s: &str) -> bool {
    let t: String = s.chars().filter(|c| !c.is_control() && !c.is_whitespace()).collect();
    match t.find(':') {
        Some(i) => t[..i].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) && i > 0,
        None => false,
    }
}

/// `http`, `https` and `mailto` links only; `javascript:`, `data:`, `file:`
/// and relative references are dropped (the text stays).
fn safe_href(h: &str) -> Option<String> {
    let t: String = h.chars().filter(|c| !c.is_control()).collect();
    let t = t.trim();
    let lower = t.to_ascii_lowercase();
    if (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")) && t.len() <= 2048 {
        Some(t.to_string())
    } else {
        None
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    fn text_of(p: &Paragraph) -> String {
        p.runs.iter().map(|r| r.text.as_str()).collect()
    }

    const ARTICLE: &str = r#"<!-- saved from url=(0030)https://example.org/news/story -->
<!DOCTYPE html><html><head><title>Story | Example</title>
<meta property="og:title" content="The Story">
<meta name="author" content="Ada Lovelace">
<meta property="article:published_time" content="2026-11-01T09:00:00Z">
<link rel="stylesheet" href="https://cdn.example/x.css">
<script>alert('head')</script></head>
<body><nav>Home | About</nav>
<article><h1>The Story</h1>
<p>First <b>bold</b> and <em>italic</em> with a <a href="https://example.org/x">link</a> &amp; <a href="javascript:alert(1)">bad</a>.</p>
<h2>Parts</h2>
<ul><li>one<li>two<ul><li>nested</li></ul></ul>
<ol><li>first</li><li>second</li></ol>
<blockquote><p>Quoted</p></blockquote>
<pre>let x = 1;
let y = 2;</pre>
<script>document.write('evil')</script>
<iframe src="https://evil.example/"></iframe>
<img src="https://tracker.example/p.png" alt="remote pic">
<img src="art_files/pic.png" alt="local pic">
<p style="display:none">hidden text</p>
</article><footer>Copyright junk</footer></body></html>"#;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("art_files")).unwrap();
        std::fs::write(d.path().join("art_files/pic.png"), PNG).unwrap();
        let p = d.path().join("story.html");
        std::fs::write(&p, ARTICLE).unwrap();
        (d, p)
    }

    #[test]
    fn imports_structure_and_attribution() {
        let (_d, p) = fixture();
        let imp = import_file(&p).unwrap();
        let paras = &imp.document.paragraphs;
        assert_eq!(imp.title.as_deref(), Some("The Story"));
        assert_eq!(imp.source_url.as_deref(), Some("https://example.org/news/story"));
        assert_eq!(paras[0].style.heading, Some(1));
        assert_eq!(text_of(&paras[0]), "The Story");
        assert_eq!(text_of(&paras[1]), "By Ada Lovelace — Published 2026-11-01T09:00:00Z");
        assert_eq!(text_of(&paras[2]), "Source: https://example.org/news/story");
        assert_eq!(paras[2].runs[1].style.link.as_deref(), Some("https://example.org/news/story"));
        // The page's own h1 is not repeated after the attribution title.
        assert_eq!(paras.iter().filter(|p| p.style.heading == Some(1)).count(), 1);
        let body = &paras[3..];
        assert!(text_of(&body[0]).starts_with("First"));
        let first = body.iter().find(|p| text_of(p).starts_with("First")).unwrap();
        assert_eq!(text_of(first), "First bold and italic with a link & bad.");
        assert!(first.runs.iter().any(|r| r.text == "bold" && r.style.bold));
        assert!(first.runs.iter().any(|r| r.text == "link" && r.style.link.as_deref() == Some("https://example.org/x")));
        assert!(first.runs.iter().all(|r| r.text != "bad" || r.style.link.is_none()));
        assert!(body.iter().any(|p| p.style.heading == Some(2) && text_of(p) == "Parts"));
        let nested = body.iter().find(|p| text_of(p) == "nested").unwrap();
        assert_eq!((nested.style.list, nested.style.list_level), (ListKind::Bullet, 1));
        assert!(body.iter().any(|p| text_of(p) == "two" && p.style.list == ListKind::Bullet && p.style.list_level == 0));
        assert!(body.iter().any(|p| text_of(p) == "second" && p.style.list == ListKind::Numbered));
        assert!(body.iter().any(|p| text_of(p) == "Quoted" && p.style.block_quote));
        let code: Vec<_> = body.iter().filter(|p| p.style.code_block.is_some()).map(text_of).collect();
        assert_eq!(code, ["let x = 1;", "let y = 2;"]);
    }

    #[test]
    fn nothing_active_survives_and_assets_stay_local() {
        let (_d, p) = fixture();
        let imp = import_file(&p).unwrap();
        let all: String = imp.document.paragraphs.iter().map(text_of).collect::<Vec<_>>().join("\n");
        for banned in ["alert", "document.write", "evil", "Home | About", "Copyright junk", "hidden text", "cdn.example"] {
            assert!(!all.contains(banned), "{banned} leaked into the document");
        }
        let images: Vec<_> = imp.document.paragraphs.iter().flat_map(|p| &p.runs).filter_map(|r| r.style.image.clone()).collect();
        assert_eq!(images.len(), 1);
        assert!(images[0].ends_with("art_files/pic.png"));
        // No run carries a non-web, non-mailto link, nor a remote image.
        for r in imp.document.paragraphs.iter().flat_map(|p| &p.runs) {
            if let Some(l) = &r.style.link {
                assert!(l.starts_with("https://"));
            }
            assert!(r.style.image.as_deref().is_none_or(|i| !i.contains("://")));
        }
        assert!(all.contains("remote pic"));
        let notes = imp.notes.join("\n");
        assert!(notes.contains("script"), "{notes}");
        assert!(notes.contains("frame"), "{notes}");
        assert!(notes.contains("not downloaded"), "{notes}");
    }

    #[test]
    fn images_outside_the_page_folder_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let site = d.path().join("site");
        std::fs::create_dir(&site).unwrap();
        std::fs::write(d.path().join("secret.png"), PNG).unwrap();
        std::fs::write(site.join("notpng.png"), b"hello world, not an image").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.path().join("secret.png"), site.join("link.png")).unwrap();
        let html = r#"<body><p>Text here</p><img src="../secret.png"><img src="/etc/passwd"><img src="link.png"><img src="notpng.png"><img src="file:///etc/passwd"><img src="data:image/png;base64,AAAA"></body>"#;
        let imp = import_bytes(html.as_bytes(), Some(&site), "t.html").unwrap();
        assert!(imp.document.paragraphs.iter().flat_map(|p| &p.runs).all(|r| r.style.image.is_none()));
    }

    #[test]
    fn errors_leave_nothing_behind() {
        let d = tempfile::tempdir().unwrap();
        let w = |n: &str, b: &[u8]| {
            let p = d.path().join(n);
            std::fs::write(&p, b).unwrap();
            p
        };
        assert!(import_file(&w("a.txt", b"hello")).unwrap_err().contains("not a saved web page"));
        assert!(import_file(&w("b.html", &[0, 1, 2, 3])).unwrap_err().contains("binary"));
        assert!(import_file(&w("c.html", b"<html><script>x()</script><nav>menu</nav></html>")).unwrap_err().contains("No readable article text"));
        assert!(import_file(&d.path().join("missing.html")).unwrap_err().contains("Cannot read"));
        let big = w("big.html", &vec![b'a'; (MAX_INPUT_BYTES + 1) as usize]);
        assert!(import_file(&big).unwrap_err().contains("limit"));
        let deep = "<div>".repeat(MAX_DEPTH + 10) + "text";
        assert!(import_bytes(deep.as_bytes(), None, "d.html").unwrap_err().contains("deep"));
    }

    #[test]
    fn malformed_markup_does_not_panic() {
        for s in ["<", "<p", "<p attr=\"x", "<!--", "<a href=", "</", "<<<>>>", "&#xFFFFFFF; &#0; &bogus", "<script>", "<p>é</p", "<p>é<"] {
            let _ = import_bytes(s.as_bytes(), None, "m.html");
        }
        let imp = import_bytes("<p>caf\u{e9} &amp; &#169; &#x41;</p>".as_bytes(), None, "m.html").unwrap();
        assert_eq!(text_of(imp.document.paragraphs.last().unwrap()), "café & © A");
    }

    #[test]
    fn saves_and_reopens_through_supported_formats() {
        let (d, p) = fixture();
        let imp = import_file(&p).unwrap();
        for ext in ["docx", "odt", "md"] {
            let out = d.path().join(format!("out.{ext}"));
            crate::save::write(&imp.document, &out).unwrap();
            let back = crate::save::read(&out).unwrap();
            let all: String = back.paragraphs.iter().map(text_of).collect::<Vec<_>>().join("\n");
            assert!(all.contains("The Story"), "{ext}");
            assert!(all.contains("Ada Lovelace"), "{ext}");
            assert!(all.contains("https://example.org/news/story"), "{ext}");
            assert!(all.contains("First bold and italic"), "{ext}");
            assert!(!all.contains("alert"), "{ext}");
        }
    }

    #[test]
    fn imported_document_is_editable() {
        let (_d, p) = fixture();
        let imp = import_file(&p).unwrap();
        let mut ed = crate::structured::StructuredEditor::new(imp.document);
        ed.set_cursor(0);
        ed.insert_text("Edited: ");
        assert!(text_of(&ed.document().paragraphs[0]).starts_with("Edited: The Story"));
    }
}
