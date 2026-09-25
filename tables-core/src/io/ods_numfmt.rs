// ods_numfmt.rs — ODF data styles (number formats) and ODF date values.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// An ods cell style names a data style (`style:data-style-name="N11"`), and
// N11 is a `<number:percentage-style>`, `<number:currency-style>`,
// `<number:date-style>`… whose children spell the format out: decimal
// places, the currency symbol, which date parts in which order. This maps
// each onto the nearest NumberFormatKind, as numfmt.rs does for xlsx codes.
//
// Also here: calamine reports an ods date cell as an ISO string
// (`office:date-value="2023-03-15"`), where xlsx gives a serial number. The
// loader turns it into the same serial so both formats hold the same value.

use suite_common_core::format::{NumberFormat, NumberFormatKind};
use std::collections::HashMap;

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = tag[from..].find(&needle) {
        let at = from + i;
        if at > 0 && tag.as_bytes()[at - 1].is_ascii_whitespace() {
            return tag[at + needle.len()..].split('"').next();
        }
        from = at + needle.len();
    }
    None
}

/// `(element name, attributes, text)` for each child element of `body`, in
/// order, one level deep is enough for data styles (they don't nest).
fn children(body: &str) -> Vec<(&str, String, String)> {
    let mut out = Vec::new();
    for chunk in body.split('<').skip(1) {
        if chunk.starts_with('/') {
            continue;
        }
        let tag_end = chunk.find('>').unwrap_or(chunk.len());
        let tag = &chunk[..tag_end];
        let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        let text = chunk.get(tag_end + 1..).unwrap_or("");
        out.push((name, format!(" {}", tag.trim_end_matches('/')), unescape(text)));
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn decimals(attrs: &str) -> u8 {
    attr(attrs, "number:decimal-places").and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// A chrono format for a `<number:date-style>` body, and whether it shows a
/// time of day.
fn date_format(body: &str) -> (String, bool) {
    let mut fmt = String::new();
    let mut time = false;
    for (name, attrs, text) in children(body) {
        let long = attr(&attrs, "number:style") == Some("long");
        match name {
            "number:year" => fmt.push_str(if long { "%Y" } else { "%y" }),
            "number:month" if attr(&attrs, "number:textual") == Some("true") => fmt.push_str(if long { "%B" } else { "%b" }),
            "number:month" => fmt.push_str(if long { "%m" } else { "%-m" }),
            "number:day" => fmt.push_str(if long { "%d" } else { "%-d" }),
            "number:day-of-week" => fmt.push_str(if long { "%A" } else { "%a" }),
            "number:hours" => {
                time = true;
                fmt.push_str(if long { "%H" } else { "%-H" })
            }
            "number:minutes" => {
                time = true;
                fmt.push_str("%M")
            }
            "number:seconds" => {
                time = true;
                fmt.push_str("%S")
            }
            "number:am-pm" => fmt.push_str("%p"),
            "number:text" => fmt.push_str(&text.replace('%', "%%")),
            _ => {}
        }
    }
    (fmt, time)
}

/// Every data style in `xml` (content.xml's or styles.xml's), by name.
pub fn parse_data_styles(xml: &str) -> HashMap<String, NumberFormat> {
    const KINDS: [&str; 7] = [
        "number:number-style",
        "number:percentage-style",
        "number:currency-style",
        "number:date-style",
        "number:time-style",
        "number:text-style",
        "number:boolean-style",
    ];
    let mut out = HashMap::new();
    for kind in KINDS {
        let open = format!("<{kind}");
        let close = format!("</{kind}>");
        for block in xml.split(open.as_str()).skip(1) {
            let head = format!(" {}", block.split('>').next().unwrap_or(""));
            let Some(name) = attr(&head, "style:name") else { continue };
            let body = block.split(close.as_str()).next().unwrap_or("");
            let parts = children(body);
            let part = |n: &str| parts.iter().find(|p| p.0 == n).map(|p| p.1.as_str());
            let format = match kind {
                "number:percentage-style" => Some(NumberFormatKind::Percent(part("number:number").map_or(0, decimals))),
                "number:currency-style" => {
                    let symbol = parts.iter().find(|p| p.0 == "number:currency-symbol").map(|p| p.2.clone()).unwrap_or_default();
                    Some(NumberFormatKind::Currency(symbol, part("number:number").map_or(0, decimals)))
                }
                "number:date-style" => {
                    let (fmt, time) = date_format(body);
                    Some(if time { NumberFormatKind::DateTime(fmt) } else { NumberFormatKind::Date(fmt) })
                }
                "number:text-style" => Some(NumberFormatKind::Text),
                "number:number-style" => {
                    if let Some(a) = part("number:fraction") {
                        let digits = attr(a, "number:min-denominator-digits").and_then(|v| v.parse().ok()).unwrap_or(1);
                        Some(NumberFormatKind::Fraction(digits))
                    } else if let Some(a) = part("number:scientific-number") {
                        Some(NumberFormatKind::Scientific(decimals(a)))
                    } else {
                        // Excel's `"$"#,##0.00` quotes the symbol, so Calc
                        // writes it as literal text beside the number, not
                        // as a currency-symbol: still a currency to show.
                        let symbol = parts
                            .iter()
                            .filter(|p| p.0 == "number:text")
                            .map(|p| p.2.trim())
                            .find(|t| !t.is_empty() && t.chars().all(|c| "$€£¥₹".contains(c)));
                        part("number:number").map(|a| match symbol {
                            Some(sym) => NumberFormatKind::Currency(sym.to_string(), decimals(a)),
                            None => NumberFormatKind::Number(decimals(a)),
                        })
                    }
                }
                // A time of day alone, and booleans: shown as the value.
                _ => None,
            };
            if let Some(kind) = format {
                out.insert(name.to_string(), NumberFormat::new(kind));
            }
        }
    }
    out
}

/// An ODF date or date-time value (`2023-03-15`, `2023-03-15T12:30:00`) as
/// an Excel serial number, the form xlsx stores and the formats expect.
pub fn iso_to_serial(iso: &str) -> Option<f64> {
    use chrono::{NaiveDate, NaiveDateTime};
    let dt = NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S"))
        .ok()
        .or_else(|| NaiveDate::parse_from_str(iso, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)))?;
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?.and_hms_opt(0, 0, 0)?;
    let serial = (dt - epoch).num_milliseconds() as f64 / 86_400_000.0;
    // Excel's fictional 1900-02-29 sits before 1900-03-01, so dates before
    // it are one serial lower than a plain day count from 1899-12-30.
    Some(if serial < 61.0 { serial - 1.0 } else { serial })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from Calc's content.xml for the render lab's number formats,
    // converted to ods.
    const STYLES: &str = r#"<office:automatic-styles>
      <number:percentage-style style:name="N11"><number:number number:decimal-places="1" number:min-decimal-places="1" number:min-integer-digits="1"/><number:text>%</number:text></number:percentage-style>
      <number:currency-style style:name="N104P0" style:volatile="true"><number:currency-symbol number:language="en" number:country="US">$</number:currency-symbol><number:number number:decimal-places="2" number:min-integer-digits="1" number:grouping="true"/></number:currency-style>
      <number:date-style style:name="N121"><number:year number:style="long"/><number:text>-</number:text><number:month number:style="long"/><number:text>-</number:text><number:day number:style="long"/></number:date-style>
      <number:number-style style:name="N122"><number:fraction number:min-integer-digits="0" number:min-numerator-digits="1" number:min-denominator-digits="1"/></number:number-style>
      <number:number-style style:name="N153"><number:text>$</number:text><number:number number:decimal-places="2" number:min-decimal-places="2" number:min-integer-digits="1" number:grouping="true"/></number:number-style>
      <number:number-style style:name="N4"><number:number number:decimal-places="2" number:min-integer-digits="1" number:grouping="true"/></number:number-style>
      <number:date-style style:name="N50"><number:month/><number:text>/</number:text><number:day/><number:text>/</number:text><number:year/><number:text> </number:text><number:hours number:style="long"/><number:text>:</number:text><number:minutes number:style="long"/></number:date-style>
    </office:automatic-styles>"#;

    #[test]
    fn data_styles_map_to_the_nearest_kind() {
        let s = parse_data_styles(STYLES);
        assert_eq!(s["N11"].kind, NumberFormatKind::Percent(1));
        assert_eq!(s["N104P0"].kind, NumberFormatKind::Currency("$".into(), 2));
        assert_eq!(s["N121"].kind, NumberFormatKind::Date("%Y-%m-%d".into()));
        assert_eq!(s["N122"].kind, NumberFormatKind::Fraction(1));
        assert_eq!(s["N4"].kind, NumberFormatKind::Number(2));
        assert_eq!(s["N50"].kind, NumberFormatKind::DateTime("%-m/%-d/%y %H:%M".into()));
        assert_eq!(s["N153"].kind, NumberFormatKind::Currency("$".into(), 2), "a quoted symbol is still a currency");
    }

    #[test]
    fn iso_dates_become_the_serials_xlsx_stores() {
        assert_eq!(iso_to_serial("2023-03-15"), Some(45000.0));
        assert_eq!(iso_to_serial("2023-03-15T12:00:00"), Some(45000.5));
        assert_eq!(iso_to_serial("1900-01-01"), Some(1.0));
        assert_eq!(iso_to_serial("1900-03-01"), Some(61.0));
        assert_eq!(iso_to_serial("not a date"), None);
    }
}
