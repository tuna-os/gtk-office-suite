// numfmt.rs — xlsx number formats: which one each cell uses, and what it means.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// An xlsx cell names a cell style by index (`<c s="3">`), the style names a
// number format by id (`<xf numFmtId="164">`), and the id is either one of
// Excel's built-ins (0–49) or declared in styles.xml (`<numFmt numFmtId="164"
// formatCode="0.0%"/>`). The reader used to ignore all three, so every value
// showed raw: 0.153 for 15.3%, 45000 for a date (render lab
// `tables/number-formats`).
//
// Format codes are a small language; this maps each onto the nearest
// `NumberFormatKind`, falling back to General rather than guessing when a
// code means something the model can't express.

use suite_common_core::format::NumberFormatKind;

/// Excel's built-in format ids, as their format codes.
pub(super) fn builtin_code(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// The first section of a code (`positive;negative;zero;text`) with quoted
/// literals, escapes and `[...]` modifiers removed, plus any currency symbol
/// those carried.
fn strip(code: &str) -> (String, Option<String>) {
    let section = code.split(';').next().unwrap_or("");
    let mut out = String::new();
    let mut currency = None;
    let mut chars = section.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                let lit: String = chars.by_ref().take_while(|&c| c != '"').collect();
                if currency.is_none() && lit.chars().any(is_currency) {
                    currency = Some(lit.trim().to_string());
                }
            }
            '\\' => {
                if let Some(c) = chars.next() {
                    if is_currency(c) {
                        currency.get_or_insert_with(|| c.to_string());
                    }
                }
            }
            '[' => {
                let inner: String = chars.by_ref().take_while(|&c| c != ']').collect();
                // [$€-407] — a currency symbol with a locale; [Red], [h] etc. are modifiers.
                if let Some(sym) = inner.strip_prefix('$') {
                    let sym = sym.split('-').next().unwrap_or("");
                    if !sym.is_empty() {
                        currency.get_or_insert_with(|| sym.to_string());
                    }
                } else if inner.eq_ignore_ascii_case("h") || inner.eq_ignore_ascii_case("hh") {
                    out.push('h');
                }
            }
            '_' | '*' => {
                chars.next(); // padding: `_)` and `* ` take the next char
            }
            c if is_currency(c) => {
                currency.get_or_insert_with(|| c.to_string());
            }
            c => out.push(c),
        }
    }
    (out, currency)
}

fn is_currency(c: char) -> bool {
    matches!(c, '$' | '€' | '£' | '¥' | '₹' | '₩' | '₽' | '¢')
}

/// Digits after the decimal point in a numeric code.
fn decimals(code: &str) -> u8 {
    code.split_once('.')
        .map(|(_, frac)| frac.chars().take_while(|c| matches!(c, '0' | '#' | '?')).count() as u8)
        .unwrap_or(0)
}

/// A date/time code as a chrono format string. `m` means minutes after an
/// hour or before seconds, months otherwise, as in Excel.
fn chrono_format(code: &str) -> String {
    let lower = code.to_ascii_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut last_was_hour = false;
    while i < chars.len() {
        let c = chars[i];
        let run = chars[i..].iter().take_while(|&&x| x == c).count();
        let token = match c {
            'y' => if run >= 3 { "%Y" } else { "%y" },
            'd' => match run { 1 => "%-d", 2 => "%d", 3 => "%a", _ => "%A" },
            'h' => if run >= 2 { "%H" } else { "%-H" },
            's' => "%S",
            'm' => {
                let rest: String = chars[i + run..].iter().collect();
                let minutes = last_was_hour || rest.trim_start_matches(|c: char| !c.is_ascii_alphabetic()).starts_with('s');
                if minutes {
                    "%M"
                } else {
                    match run { 1 => "%-m", 2 => "%m", 3 => "%b", _ => "%B" }
                }
            }
            'a' if lower[i..].starts_with("am/pm") => {
                i += 5;
                out.push_str("%p");
                continue;
            }
            _ => {
                out.extend(std::iter::repeat_n(c, run));
                i += run;
                continue;
            }
        };
        last_was_hour = c == 'h';
        out.push_str(token);
        i += run;
    }
    out
}

/// Values a kind and a code must draw alike to be the same format.
const SAMPLES: [&str; 7] = ["1234.567", "-1234.567", "0", "0.5", "12", "45306.75", "text"];

/// The format a code means: the kind the model names it by when that kind
/// draws every sample exactly as the code does, else the code itself
/// ([`NumberFormatKind::Custom`]), so nothing a file says is lost or
/// drawn differently.
pub fn kind_for_code(code: &str) -> NumberFormatKind {
    let kind = nearest_kind(code);
    if kind == NumberFormatKind::General {
        return kind;
    }
    // Sections, colours and conditions are the code's own, even where a
    // kind draws the same text: kept, they save back as they came.
    let brackets = code.split('[').skip(1).any(|b| !b.starts_with('$'));
    if code.contains(';') || brackets {
        return NumberFormatKind::Custom(code.trim().to_string());
    }
    let nf = suite_common_core::format::NumberFormat::new(kind.clone());
    // Dates are compared where a date exists: from 1900-03-01 on.
    let samples: &[&str] = match kind {
        NumberFormatKind::Date(_) | NumberFormatKind::DateTime(_) => &["45306.75", "61", "text"],
        _ => &SAMPLES,
    };
    let same = samples
        .iter()
        .all(|s| nf.format(s) == suite_common_core::format_code::format_with_code(code, s));
    if same {
        kind
    } else {
        NumberFormatKind::Custom(code.trim().to_string())
    }
}

/// The code to write for a kind; `None` for General.
pub fn code_for_kind(kind: &NumberFormatKind) -> Option<String> {
    use NumberFormatKind::*;
    let places = |d: u8| if d == 0 { String::new() } else { format!(".{}", "0".repeat(d as usize)) };
    match kind {
        General => None,
        Number(d) => Some(format!("#,##0{}", places(*d))),
        Currency(sym, d) => Some(format!("\"{sym}\"#,##0{}", places(*d))),
        Percent(d) => Some(format!("0{}%", places(*d))),
        Scientific(d) => Some(format!("0{}E+00", places(*d))),
        Text => Some("@".into()),
        Date(fmt) | DateTime(fmt) => Some(code_for_chrono(fmt)),
        Fraction(d) => Some(format!("# {}/{}", "?".repeat(*d as usize), "?".repeat(*d as usize))),
        Custom(code) => Some(code.clone()),
    }
}

/// A chrono date format as a spreadsheet code (`%Y-%m-%d` → `yyyy-mm-dd`).
fn code_for_chrono(fmt: &str) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let padless = chars.peek() == Some(&'-');
        if padless {
            chars.next();
        }
        let spec = chars.next().unwrap_or('%');
        out.push_str(match (spec, padless) {
            ('Y', _) => "yyyy",
            ('y', _) => "yy",
            ('m', false) => "mm",
            ('m', true) => "m",
            ('d', false) | ('e', _) => "dd",
            ('d', true) => "d",
            ('b', _) => "mmm",
            ('B', _) => "mmmm",
            ('a', _) => "ddd",
            ('A', _) => "dddd",
            ('H', false) | ('I', false) => "hh",
            ('H', true) | ('I', true) => "h",
            ('M', _) => "mm",
            ('S', _) => "ss",
            ('p', _) => "AM/PM",
            _ => "",
        });
    }
    out
}

/// What a format code means, as the model's named kinds can express it.
pub fn nearest_kind(code: &str) -> NumberFormatKind {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        return NumberFormatKind::General;
    }
    let (plain, currency) = strip(code);
    let plain = plain.trim();
    if plain == "@" {
        return NumberFormatKind::Text;
    }
    let lower = plain.to_ascii_lowercase();
    if lower.contains("e+") || lower.contains("e-") {
        return NumberFormatKind::Scientific(decimals(plain));
    }
    if let Some((_, den)) = plain.split_once('/') {
        if plain.contains('?') || den.chars().all(|c| matches!(c, '?' | '#' | '0' | ' ')) {
            let digits = den.chars().filter(|c| matches!(c, '?' | '#' | '0')).count().max(1);
            return NumberFormatKind::Fraction(digits.min(4) as u8);
        }
    }
    let has_date = lower.chars().any(|c| matches!(c, 'y' | 'd'))
        || (lower.contains('m') && !lower.contains('0') && !lower.contains('#'));
    let has_time = lower.contains('h') || lower.contains('s') && !lower.contains('0');
    if has_date || has_time {
        let fmt = chrono_format(plain);
        return if has_date && has_time {
            NumberFormatKind::DateTime(fmt)
        } else if has_date {
            NumberFormatKind::Date(fmt)
        } else {
            // A time of day on its own; DateTime prints it from the serial.
            NumberFormatKind::DateTime(fmt)
        };
    }
    if plain.contains('%') {
        return NumberFormatKind::Percent(decimals(plain));
    }
    if let Some(sym) = currency {
        return NumberFormatKind::Currency(sym, decimals(plain));
    }
    if plain.contains('0') || plain.contains('#') {
        return NumberFormatKind::Number(decimals(plain));
    }
    NumberFormatKind::General
}

fn xml_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!(" {attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    tag[start..].split('"').next()
}

/// `(row, col, style index)` for every cell in a worksheet part that names a
/// non-default style, 0-based.
pub fn cell_style_indices(sheet_xml: &str) -> Vec<(usize, usize, usize)> {
    let Some(data) = sheet_xml.split("<sheetData").nth(1) else { return Vec::new() };
    let data = data.split("</sheetData>").next().unwrap_or("");
    data.split("<c ")
        .skip(1)
        .filter_map(|tag| {
            let tag = format!(" {}", tag.split('>').next().unwrap_or(""));
            let s = xml_attr(&tag, "s")?.parse::<usize>().ok().filter(|&s| s > 0)?;
            let (r, c) = crate::sheet::parse_cell_ref(xml_attr(&tag, "r")?)?;
            Some((r, c, s))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use NumberFormatKind::*;

    #[test]
    fn a_code_a_kind_draws_differently_stays_a_code() {
        // Drawn the same: the kind.
        assert_eq!(kind_for_code("#,##0.00"), Number(2));
        assert_eq!(kind_for_code("yyyy-mm-dd"), Date("%Y-%m-%d".into()));
        assert_eq!(kind_for_code("@"), Text);
        // No thousands separator, two sections, a condition, literals,
        // a colour: the code, as written.
        for code in ["0.00", "#,##0;[Red]-#,##0", "[>=1000]#,##0,\"K\";0", "0.0 \"kg\"", "#,##0.00_);(#,##0.00)"] {
            assert_eq!(kind_for_code(code), Custom(code.into()), "{code}");
        }
    }

    #[test]
    fn every_kind_writes_a_code_that_reads_back_as_itself() {
        for kind in [Number(0), Number(2), Currency("$".into(), 2), Scientific(3), Text, Date("%Y-%m-%d".into()), Custom("0.0;-0.0;\"–\"".into())] {
            let code = code_for_kind(&kind).unwrap();
            assert_eq!(kind_for_code(&code), kind, "{code}");
        }
    }

    #[test]
    fn codes_map_to_the_nearest_kind() {
        let kind_for_code = nearest_kind;
        assert_eq!(kind_for_code("General"), General);
        assert_eq!(kind_for_code("0.0%"), Percent(1));
        assert_eq!(kind_for_code("0%"), Percent(0));
        assert_eq!(kind_for_code("\"$\"#,##0.00"), Currency("$".into(), 2));
        assert_eq!(kind_for_code("$#,##0.00"), Currency("$".into(), 2));
        assert_eq!(kind_for_code("[$€-407]#,##0.00"), Currency("€".into(), 2));
        assert_eq!(kind_for_code("#,##0.00"), Number(2));
        assert_eq!(kind_for_code("0"), Number(0));
        assert_eq!(kind_for_code("# ?/?"), Fraction(1));
        assert_eq!(kind_for_code("# ??/??"), Fraction(2));
        assert_eq!(kind_for_code("0.00E+00"), Scientific(2));
        assert_eq!(kind_for_code("@"), Text);
    }

    #[test]
    fn dates_and_times_translate_to_chrono() {
        let kind_for_code = nearest_kind;
        assert_eq!(kind_for_code("yyyy-mm-dd"), Date("%Y-%m-%d".into()));
        assert_eq!(kind_for_code("mm-dd-yy"), Date("%m-%d-%y".into()));
        assert_eq!(kind_for_code("d-mmm-yy"), Date("%-d-%b-%y".into()));
        // m after h is minutes, not months.
        assert_eq!(kind_for_code("m/d/yy h:mm"), DateTime("%-m/%-d/%y %-H:%M".into()));
        assert_eq!(kind_for_code("h:mm:ss"), DateTime("%-H:%M:%S".into()));
    }

    #[test]
    fn styles_resolve_builtin_and_custom_ids_in_xf_order() {
        let styles = r#"<styleSheet><numFmts count="2">
            <numFmt numFmtId="164" formatCode="0.0%"/>
            <numFmt numFmtId="165" formatCode="&quot;$&quot;#,##0.00"/></numFmts>
            <cellStyleXfs count="1"><xf numFmtId="3"/></cellStyleXfs>
            <cellXfs count="4"><xf numFmtId="0" fontId="0"/><xf numFmtId="164" applyNumberFormat="1"/>
            <xf numFmtId="165"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
        let kinds: Vec<_> =
            super::super::xlsx_styles::parse_cell_styles(styles).into_iter().map(|x| x.format.kind).collect();
        assert_eq!(kinds, vec![General, Percent(1), Currency("$".into(), 2), Date("%m-%d-%y".into())]);
    }

    #[test]
    fn cells_name_their_style_by_index() {
        let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1" s="1"><v>0.153</v></c>
            <c r="B1"><v>2</v></c></row><row r="3"><c r="C3" s="2" t="n"><v>1</v></c></row>
            </sheetData></worksheet>"#;
        assert_eq!(cell_style_indices(sheet), vec![(0, 0, 1), (2, 2, 2)]);
    }
}
