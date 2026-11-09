//! Dark-mode colour adaptation (#35).
//!
//! Emails are designed for light rendering: dark text, light (or absent)
//! backgrounds. In dark mode an email that sets `color:#333` but no background
//! paints near-black text on the reader's dark ground. The sandboxed frames run
//! no JavaScript, so the fix happens here, on the document text, at render
//! time (never in the on-disk body cache): every color the message declares is
//! checked and, when its lightness is wrong for a dark ground, flipped in HSL —
//! hue and saturation kept, lightness mirrored. Text colors darker than
//! mid-grey become light; backgrounds lighter than mid-grey become dark;
//! everything already suited to a dark ground is left untouched, so mail
//! designed dark passes through unchanged.

/// Parse a CSS color token to linear [r, g, b, a] in 0..=1. Handles hex
/// (#rgb/#rgba/#rrggbb/#rrggbbaa), rgb()/rgba() with numbers or percentages,
/// and the common named colors. `bare_hex` additionally accepts legacy
/// attribute values like `bgcolor=ffffff` with no `#`.
pub(super) fn parse_css_color(token: &str, bare_hex: bool) -> Option<[f32; 4]> {
    let t = token.trim();
    let hex = |s: &str| -> Option<[f32; 4]> {
        let v = |i: usize, n: usize| {
            u8::from_str_radix(&s[i..i + n], 16).ok().map(|b| {
                if n == 1 {
                    (b * 17) as f32 / 255.0
                } else {
                    b as f32 / 255.0
                }
            })
        };
        match s.len() {
            3 => Some([v(0, 1)?, v(1, 1)?, v(2, 1)?, 1.0]),
            4 => Some([v(0, 1)?, v(1, 1)?, v(2, 1)?, v(3, 1)?]),
            6 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, 1.0]),
            8 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, v(6, 2)?]),
            _ => None,
        }
    };
    if let Some(rest) = t.strip_prefix('#') {
        return hex(rest);
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("rgb(") || lower.starts_with("rgba(") {
        let inner = t[t.find('(')? + 1..].strip_suffix(')')?;
        let parts: Vec<&str> = inner
            .split(|c| c == ',' || c == '/' || char::is_whitespace(c))
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let chan = |s: &str| -> Option<f32> {
            if let Some(p) = s.strip_suffix('%') {
                p.trim().parse::<f32>().ok().map(|v| v / 100.0)
            } else {
                s.trim().parse::<f32>().ok().map(|v| v / 255.0)
            }
        };
        let alpha = |s: &str| -> Option<f32> {
            if let Some(p) = s.strip_suffix('%') {
                p.trim().parse::<f32>().ok().map(|v| v / 100.0)
            } else {
                s.trim().parse::<f32>().ok()
            }
        };
        return Some([
            chan(parts[0])?.clamp(0.0, 1.0),
            chan(parts[1])?.clamp(0.0, 1.0),
            chan(parts[2])?.clamp(0.0, 1.0),
            parts
                .get(3)
                .and_then(|s| alpha(s))
                .unwrap_or(1.0)
                .clamp(0.0, 1.0),
        ]);
    }
    let named: Option<u32> = match lower.as_str() {
        "black" => Some(0x000000),
        "white" => Some(0xffffff),
        "gray" | "grey" => Some(0x808080),
        "dimgray" | "dimgrey" => Some(0x696969),
        "darkgray" | "darkgrey" => Some(0xa9a9a9),
        "lightgray" | "lightgrey" => Some(0xd3d3d3),
        "gainsboro" => Some(0xdcdcdc),
        "whitesmoke" => Some(0xf5f5f5),
        "silver" => Some(0xc0c0c0),
        "red" => Some(0xff0000),
        "darkred" | "maroon" => Some(0x800000),
        "green" => Some(0x008000),
        "darkgreen" => Some(0x006400),
        "blue" => Some(0x0000ff),
        "navy" | "darkblue" => Some(0x000080),
        "midnightblue" => Some(0x191970),
        "purple" => Some(0x800080),
        "indigo" => Some(0x4b0082),
        "brown" => Some(0xa52a2a),
        "orange" => Some(0xffa500),
        "yellow" => Some(0xffff00),
        "teal" => Some(0x008080),
        "olive" => Some(0x808000),
        _ => None,
    };
    if let Some(rgb) = named {
        return Some([
            ((rgb >> 16) & 0xff) as f32 / 255.0,
            ((rgb >> 8) & 0xff) as f32 / 255.0,
            (rgb & 0xff) as f32 / 255.0,
            1.0,
        ]);
    }
    if bare_hex && t.len() == 6 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return hex(t);
    }
    None
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < f32::EPSILON {
        ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s <= 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0))
}

/// Flip a color for the dark ground when its lightness calls for it: text
/// darker than mid-grey mirrors up (floored so it stays clearly readable),
/// backgrounds lighter than mid-grey mirror down (floored above pure black so
/// they read as surfaces, like the reader's own grounds). `None` = keep as is.
fn adapt_color(token: &str, background: bool, bare_hex: bool) -> Option<String> {
    let [r, g, b, a] = parse_css_color(token, bare_hex)?;
    if a <= 0.01 {
        return None; // effectively transparent either way
    }
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let flipped = if background {
        if l <= 0.6 {
            return None;
        }
        (1.0 - l).max(0.08)
    } else {
        if l >= 0.5 {
            return None;
        }
        (1.0 - l).max(0.72)
    };
    let (nr, ng, nb) = hsl_to_rgb(h, s, flipped);
    let to8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(if a < 1.0 {
        format!("rgba({},{},{},{:.2})", to8(nr), to8(ng), to8(nb), a)
    } else {
        format!("#{:02x}{:02x}{:02x}", to8(nr), to8(ng), to8(nb))
    })
}

/// The character that starts at byte `i` of `s`. The scanners below call it
/// only while `i` is below the length, and step by whole characters, so
/// there always is one.
fn char_at(s: &str, i: usize) -> char {
    s[i..]
        .chars()
        .next()
        .expect("the scanners stay inside the text, on character boundaries")
}

/// Rewrite one CSS declaration list (an inline `style` attribute's content or
/// a rule body). Declarations are split at `;` outside parentheses and quotes
/// — data: URLs contain semicolons — and only `color`, `background-color`,
/// and `background`'s color tokens are touched.
fn rewrite_declarations(decls: &str) -> String {
    let mut out = String::with_capacity(decls.len());
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut start = 0;
    let mut i = 0;
    while i < decls.len() {
        let c = char_at(decls, i);
        // Comments may contain `;` — `/* background: #fff; */` must not split
        // the declaration after it in half (seen in the wild, issue #35).
        if quote.is_none() && decls[i..].starts_with("/*") {
            i += decls[i..]
                .find("*/")
                .map(|r| r + 2)
                .unwrap_or(decls.len() - i);
            continue;
        }
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ';') if depth == 0 => {
                out.push_str(&rewrite_one_declaration(&decls[start..i]));
                out.push(';');
                start = i + 1;
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    out.push_str(&rewrite_one_declaration(&decls[start..]));
    out
}

/// One `prop: value` declaration, color-adapted when the property carries a
/// color whose direction we know. Anything unrecognised passes through
/// byte-for-byte.
fn rewrite_one_declaration(decl: &str) -> String {
    // Step over any leading comments so `/* old */ background: #fff` still
    // parses to a property we recognise; the comment is kept verbatim.
    let mut p = 0;
    loop {
        let rest = &decl[p..];
        let trimmed = rest.trim_start();
        p += rest.len() - trimmed.len();
        if trimmed.starts_with("/*") {
            match trimmed.find("*/") {
                Some(e) => p += e + 2,
                None => return decl.to_string(),
            }
        } else {
            break;
        }
    }
    let (head, decl_body) = decl.split_at(p);
    let Some(colon) = decl_body.find(':') else {
        return decl.to_string();
    };
    let prop = decl_body[..colon].trim().to_ascii_lowercase();
    let value = &decl_body[colon + 1..];
    let background = match prop.as_str() {
        "color" => false,
        "background-color" | "background" => true,
        _ => return decl.to_string(),
    };
    // Keep any !important, transform the value's color tokens.
    let (value_body, important) = match value.to_ascii_lowercase().find("!important") {
        Some(at) => (&value[..at], &value[at..]),
        None => (value, ""),
    };
    let mut rewritten = String::with_capacity(value_body.len());
    for piece in split_value_tokens(value_body) {
        match &piece {
            ValuePiece::Token(t) => match adapt_color(t, background, false) {
                Some(new) => rewritten.push_str(&new),
                None => rewritten.push_str(t),
            },
            ValuePiece::Raw(r) => rewritten.push_str(r),
        }
    }
    format!("{head}{}:{rewritten}{important}", &decl_body[..colon])
}

enum ValuePiece<'a> {
    /// A candidate color token (word or function call).
    Token(&'a str),
    /// Whitespace, url(...), strings — copied verbatim.
    Raw(&'a str),
}

/// Split a CSS value into color-candidate tokens and verbatim runs, keeping
/// `url(...)` and quoted strings intact (their contents are not colors, and
/// data: URLs may contain anything).
fn split_value_tokens(value: &str) -> Vec<ValuePiece<'_>> {
    let mut pieces = Vec::new();
    let lower = value.to_ascii_lowercase();
    let mut i = 0;
    while i < value.len() {
        let c = char_at(value, i);
        if c.is_whitespace() || c == ',' {
            let start = i;
            while i < value.len() {
                let c = char_at(value, i);
                if c.is_whitespace() || c == ',' {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
            pieces.push(ValuePiece::Raw(&value[start..i]));
        } else if lower[i..].starts_with("url(") {
            let start = i;
            i += 4;
            let mut depth = 1;
            while i < value.len() && depth > 0 {
                let c = char_at(value, i);
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                }
                i += c.len_utf8();
            }
            pieces.push(ValuePiece::Raw(&value[start..i]));
        } else if c == '"' || c == '\'' {
            let start = i;
            i += 1;
            while i < value.len() {
                let ch = char_at(value, i);
                i += ch.len_utf8();
                if ch == c {
                    break;
                }
            }
            pieces.push(ValuePiece::Raw(&value[start..i]));
        } else {
            // A word, possibly a function like rgb(...): take through balanced
            // parens if one opens immediately after the name.
            let start = i;
            while i < value.len() {
                let ch = char_at(value, i);
                if ch.is_whitespace() || ch == ',' {
                    break;
                }
                i += ch.len_utf8();
                if ch == '(' {
                    let mut depth = 1;
                    while i < value.len() && depth > 0 {
                        let c2 = char_at(value, i);
                        if c2 == '(' {
                            depth += 1;
                        } else if c2 == ')' {
                            depth -= 1;
                        }
                        i += c2.len_utf8();
                    }
                    break;
                }
            }
            pieces.push(ValuePiece::Token(&value[start..i]));
        }
    }
    pieces
}

/// Rewrite the declaration bodies inside a `<style>` block, leaving selectors,
/// at-rules, comments, and strings untouched. Brace-nesting (`@media { sel {
/// … } }`) is handled by only treating brace-content that closes without
/// opening another brace as declarations.
fn rewrite_css(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    let mut seg_start = 0;
    let mut in_comment = false;
    let mut quote: Option<char> = None;
    let mut decl_start: Option<usize> = None;
    while i < css.len() {
        let c = char_at(css, i);
        if in_comment {
            if css[i..].starts_with("*/") {
                in_comment = false;
                i += 2;
                continue;
            }
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if css[i..].starts_with("/*") {
            in_comment = true;
            i += 2;
            continue;
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '{' {
            out.push_str(&css[seg_start..i + 1]);
            seg_start = i + 1;
            decl_start = Some(i + 1);
        } else if c == '}' {
            match decl_start.take() {
                // The segment closed without opening a nested brace: it is a
                // declaration list.
                Some(ds) => {
                    out.push_str(&rewrite_declarations(&css[ds..i]));
                    out.push('}');
                    seg_start = i + 1;
                }
                None => {
                    out.push_str(&css[seg_start..i + 1]);
                    seg_start = i + 1;
                }
            }
        }
        i += c.len_utf8();
    }
    out.push_str(&css[seg_start..]);
    out
}

/// Rewrite one tag's color-bearing attributes: `style` (declarations),
/// `color`/`text` (text direction), `bgcolor` (background direction). `text`
/// only means a color on `<body>`.
fn rewrite_tag_attrs(tag: &str) -> String {
    let lower = tag.to_ascii_lowercase();
    let is_body = lower.starts_with("<body");
    let mut out = String::with_capacity(tag.len());
    let mut i = 0;
    while i < tag.len() {
        let rest_lower = &lower[i..];
        let attr = ["style", "color", "bgcolor", "text"].iter().find(|a| {
            rest_lower.starts_with(**a)
                && i > 0
                && lower.as_bytes()[i - 1].is_ascii_whitespace()
                && rest_lower[a.len()..].trim_start().starts_with('=')
        });
        let Some(attr) = attr else {
            let c = char_at(tag, i);
            out.push(c);
            i += c.len_utf8();
            continue;
        };
        if *attr == "text" && !is_body {
            let c = char_at(tag, i);
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        // name, '=', then a quoted or bare value.
        let eq = i + rest_lower
            .find('=')
            .expect("an attribute is only matched with an `=` after it");
        let mut v = eq + 1;
        while v < tag.len() && tag.as_bytes()[v].is_ascii_whitespace() {
            v += 1;
        }
        let (val_start, val_end, quoted) =
            if v < tag.len() && (tag.as_bytes()[v] == b'"' || tag.as_bytes()[v] == b'\'') {
                let q = tag.as_bytes()[v] as char;
                let end = tag[v + 1..].find(q).map(|r| v + 1 + r).unwrap_or(tag.len());
                (v + 1, end, true)
            } else {
                let end = tag[v..]
                    .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                    .map(|r| v + r)
                    .unwrap_or(tag.len());
                (v, end, false)
            };
        let value = &tag[val_start..val_end];
        let new = match *attr {
            "style" => rewrite_declarations(value),
            "bgcolor" => adapt_color(value, true, true).unwrap_or_else(|| value.to_string()),
            _ => adapt_color(value, false, true).unwrap_or_else(|| value.to_string()),
        };
        out.push_str(&tag[i..val_start]);
        out.push_str(&new);
        i = val_end + usize::from(quoted && val_end < tag.len());
        if quoted && val_end < tag.len() {
            out.push(tag.as_bytes()[val_end] as char);
        }
    }
    out
}

/// Pin every `prefers-color-scheme` media query in the document to the ground
/// the reader actually chose, rather than the OS setting WebKit reads.
///
/// The reader sets each message's ground itself (a light card, or a dark one
/// via [`adapt_colors_for_dark`]) and injects a `color-scheme` to match. But
/// `@media (prefers-color-scheme: dark)` inside the sandboxed frame is evaluated
/// against the *desktop's* preference, which our injected value does not change.
/// So an email that ships its own dark rules — a Google Calendar invite forcing
/// `color:#9aa0a6 !important` under `prefers-color-scheme: dark`, say — paints
/// that light-grey text onto our white card whenever the desktop is in dark mode
/// but the message is shown light (a light message theme, or a mismatch between
/// the app's and the desktop's schemes). The result is near-invisible grey on
/// white, and the mirror case (light rules on our dark ground) is just as bad.
///
/// Rewriting the media *feature* to a width test that is always or never true —
/// keeping any surrounding `only screen and …` intact — makes the email's own
/// light/dark rules follow our ground deterministically. `999999px` stands in
/// for "never" (no message is a million pixels wide) and `0px` for "always".
pub(super) fn pin_color_scheme(doc: &str, dark: bool) -> String {
    // (feature we're neutralising, replacement that matches iff we chose it)
    let (dark_repl, light_repl) = if dark {
        ("min-width:0px", "min-width:999999px")
    } else {
        ("min-width:999999px", "min-width:0px")
    };
    let mut out = String::with_capacity(doc.len());
    let lower = doc.to_ascii_lowercase();
    let needle = "prefers-color-scheme";
    let mut i = 0;
    while let Some(rel) = lower[i..].find(needle) {
        let start = i + rel;
        // Parse `prefers-color-scheme` [ws] `:` [ws] (`dark`|`light`).
        let after = start + needle.len();
        let rest = lower[after..].trim_start();
        let ws = lower[after..].len() - rest.len();
        if let Some(colon) = rest.strip_prefix(':') {
            let val = colon.trim_start();
            let vws = colon.len() - val.len();
            let repl = if val.starts_with("dark") {
                Some((4usize, dark_repl))
            } else if val.starts_with("light") {
                Some((5usize, light_repl))
            } else {
                None
            };
            if let Some((vlen, replacement)) = repl {
                out.push_str(&doc[i..start]);
                out.push_str(replacement);
                // Skip the original `prefers-color-scheme : <value>` span.
                i = after + ws + 1 + vws + vlen;
                continue;
            }
        }
        // Not a real prefers-color-scheme:<value> — keep it verbatim and move on.
        out.push_str(&doc[i..after]);
        i = after;
    }
    out.push_str(&doc[i..]);
    out
}

/// The whole-document pass: `<style>` blocks through the CSS rewriter, every
/// other tag through the attribute rewriter, text content untouched. Tag ends
/// are found quote-aware, since `>` inside attribute values is legal HTML.
pub(super) fn adapt_colors_for_dark(doc: &str) -> String {
    let lower = doc.to_ascii_lowercase();
    let mut out = String::with_capacity(doc.len() + 64);
    let mut i = 0;
    while i < doc.len() {
        let Some(rel) = doc[i..].find('<') else {
            out.push_str(&doc[i..]);
            break;
        };
        let tag_start = i + rel;
        out.push_str(&doc[i..tag_start]);
        if lower[tag_start..].starts_with("<!--") {
            let end = doc[tag_start..]
                .find("-->")
                .map(|r| tag_start + r + 3)
                .unwrap_or(doc.len());
            out.push_str(&doc[tag_start..end]);
            i = end;
            continue;
        }
        if lower[tag_start..].starts_with("<style") {
            let Some(open) = doc[tag_start..].find('>') else {
                out.push_str(&doc[tag_start..]);
                break;
            };
            let css_start = tag_start + open + 1;
            out.push_str(&doc[tag_start..css_start]);
            let css_end = lower[css_start..]
                .find("</style")
                .map(|r| css_start + r)
                .unwrap_or(doc.len());
            out.push_str(&rewrite_css(&doc[css_start..css_end]));
            i = css_end;
            continue;
        }
        // Quote-aware scan for the tag's closing '>'.
        let mut j = tag_start;
        let mut quote: Option<u8> = None;
        let mut end = doc.len();
        while j < doc.len() {
            let b = doc.as_bytes()[j];
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'"' || b == b'\'' => quote = Some(b),
                None if b == b'>' => {
                    end = j + 1;
                    break;
                }
                None => {}
            }
            j += 1;
        }
        out.push_str(&rewrite_tag_attrs(&doc[tag_start..end]));
        i = end;
    }
    out
}
