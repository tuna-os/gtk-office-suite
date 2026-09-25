// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_chips.rs — smart chips in .docx, as Word's own content controls.
//
// Each chip is an inline `w:sdt` (structured document tag) around its
// label. Its `w:tag` is "letters-chip:KIND:VALUE", which is what reopens it
// as a chip. A date chip is also a Word date-picker control (`w:date` with
// the full date), so Word and LibreOffice treat it as a date, and a date
// control from any other source opens as a date chip too. A link or person
// chip keeps its hyperlink inside the control.
//
// rdocx has no builder for run-level content controls, and does not hand
// back runs inside them. So:
// - **Writing:** the writer puts a sentinel run where each chip goes, then
//   `wrap` turns each sentinel into the control.
// - **Reading:** `unwrap` turns each chip control back into a sentinel run
//   before rdocx sees the file, and `restore` makes those runs chips.
//
// A sentinel is private-use characters around an index into the list of
// chips, so no document text can be mistaken for one.

use crate::chips::{chip_run, Chip, ChipKind};
use crate::model::{Document, Run};

const OPEN: char = '\u{E000}';
const CLOSE: char = '\u{E001}';
const TAG_PREFIX: &str = "letters-chip:";

/// The sentinel standing for chip `index` in the written text.
pub fn sentinel(index: usize) -> String {
    format!("{OPEN}{index}{CLOSE}")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn kind_name(kind: ChipKind) -> &'static str {
    match kind {
        ChipKind::Date => "date",
        ChipKind::Person => "person",
        ChipKind::Link => "link",
    }
}

/// The `w:sdtPr` of `chip`.
fn properties(chip: &Chip) -> String {
    let tag = escape(&format!("{TAG_PREFIX}{}:{}", kind_name(chip.kind), chip.value));
    let (alias, date) = match chip.kind {
        ChipKind::Date => (
            "Date",
            format!(
                "<w:date w:fullDate=\"{}T00:00:00Z\"><w:dateFormat w:val=\"d MMM yyyy\"/><w:lid w:val=\"en-GB\"/>\
                 <w:storeMappedDataAs w:val=\"dateTime\"/><w:calendar w:val=\"gregorian\"/></w:date>",
                escape(&chip.value)
            ),
        ),
        ChipKind::Person => ("Person", String::new()),
        ChipKind::Link => ("Link", String::new()),
    };
    format!("<w:sdtPr><w:alias w:val=\"{alias}\"/><w:tag w:val=\"{tag}\"/>{date}</w:sdtPr>")
}

/// The start of the element named `name` ("w:r", "w:t") that begins
/// last before `pos`: an exact name, not a longer one ("w:rPr").
fn last_start(xml: &str, pos: usize, name: &str) -> Option<usize> {
    let a = xml[..pos].rfind(&format!("<{name}>"));
    let b = xml[..pos].rfind(&format!("<{name} "));
    a.max(b)
}

/// Wrap each chip's sentinel run (or the hyperlink holding it) in its
/// content control, with the chip's label as the text.
pub fn wrap(document_xml: &str, chips: &[(Chip, String)]) -> String {
    let mut xml = document_xml.to_string();
    for (i, (chip, label)) in chips.iter().enumerate() {
        let mark = sentinel(i);
        let Some(pos) = xml.find(&mark) else { continue };
        let Some(run) = last_start(&xml, pos, "w:r") else { continue };
        let Some(run_end) = xml[pos..].find("</w:r>").map(|e| pos + e + "</w:r>".len()) else { continue };
        // Inside a hyperlink (a link or person chip): wrap the hyperlink.
        let link = xml[..run].rfind("<w:hyperlink").filter(|h| !xml[*h..run].contains("</w:hyperlink>"));
        let (start, end) = match link {
            Some(h) => match xml[pos..].find("</w:hyperlink>") {
                Some(e) => (h, pos + e + "</w:hyperlink>".len()),
                None => (run, run_end),
            },
            None => (run, run_end),
        };
        let mut inner = xml[start..end].replace(&mark, &escape(label));
        // A label may begin or end with a space.
        inner = inner.replacen("<w:t>", "<w:t xml:space=\"preserve\">", 1);
        let control = format!("<w:sdt>{}<w:sdtContent>{inner}</w:sdtContent></w:sdt>", properties(chip));
        xml.replace_range(start..end, &control);
    }
    xml
}

/// The value of attribute `attr` on the first `<name ...>` in `xml`.
fn attribute(xml: &str, name: &str, attr: &str) -> Option<String> {
    let at = xml.find(&format!("<{name} "))?;
    let tag = &xml[at..at + xml[at..].find('>')?];
    let key = format!("{attr}=\"");
    let v = tag.find(&key)? + key.len();
    Some(unescape(&tag[v..v + tag[v..].find('"')?]))
}

/// The text of every `w:t` in `xml`, in order.
fn texts(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(at) = first_t(rest) {
        let Some(open_end) = rest[at..].find('>').map(|e| at + e + 1) else { break };
        if rest[at..open_end].ends_with("/>") {
            rest = &rest[open_end..];
            continue;
        }
        let Some(close) = rest[open_end..].find("</w:t>").map(|e| open_end + e) else { break };
        out.push_str(&unescape(&rest[open_end..close]));
        rest = &rest[close..];
    }
    out
}

fn first_t(xml: &str) -> Option<usize> {
    match (xml.find("<w:t>"), xml.find("<w:t ")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// What an inline content control stands for, if it is a chip: one of
/// ours by its tag, or any date control by its date.
fn chip_of(control: &str) -> Option<(Chip, String)> {
    let props = &control[..control.find("</w:sdtPr>").unwrap_or(0)];
    let content = &control[control.find("<w:sdtContent")?..];
    let label = texts(content);
    if let Some(tag) = attribute(props, "w:tag", "w:val").and_then(|t| t.strip_prefix(TAG_PREFIX).map(str::to_string)) {
        let (kind, value) = tag.split_once(':')?;
        let kind = match kind {
            "date" => ChipKind::Date,
            "person" => ChipKind::Person,
            "link" => ChipKind::Link,
            _ => return None,
        };
        return Some((Chip { kind, value: value.to_string() }, label));
    }
    let full = attribute(props, "w:date", "w:fullDate")?;
    let date = full.get(..10)?;
    crate::chips::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some((Chip { kind: ChipKind::Date, value: date.to_string() }, label))
}

/// Turn every inline chip control in `document_xml` into a sentinel run.
/// Block-level controls (around paragraphs) are left alone.
pub fn unwrap(document_xml: &str) -> (String, Vec<(Chip, String)>) {
    let mut xml = document_xml.to_string();
    let mut chips = Vec::new();
    let mut from = 0;
    while let Some(start) = [xml[from..].find("<w:sdt>"), xml[from..].find("<w:sdt ")].into_iter().flatten().min().map(|a| from + a) {
        // Its matching end, counting nested controls.
        let mut depth = 0usize;
        let mut at = start;
        let end = loop {
            let open = [xml[at + 1..].find("<w:sdt>"), xml[at + 1..].find("<w:sdt ")].into_iter().flatten().min().map(|a| at + 1 + a);
            let Some(close) = xml[at + 1..].find("</w:sdt>").map(|a| at + 1 + a) else { break None };
            match open {
                Some(o) if o < close => {
                    depth += 1;
                    at = o;
                }
                _ if depth == 0 => break Some(close + "</w:sdt>".len()),
                _ => {
                    depth -= 1;
                    at = close;
                }
            }
        };
        let Some(end) = end else { break };
        let control = &xml[start..end];
        let body = &control[6..];
        let inline = !control.contains("<w:p>") && !control.contains("<w:p ") && !body.contains("<w:sdt>") && !body.contains("<w:sdt ");
        match chip_of(control).filter(|_| inline) {
            Some(chip) => {
                let run = format!("<w:r><w:t>{}</w:t></w:r>", sentinel(chips.len()));
                chips.push(chip);
                xml.replace_range(start..end, &run);
                from = start + run.len();
            }
            None => from = start + "<w:sdt".len(),
        }
    }
    (xml, chips)
}

/// Make the sentinel runs `unwrap` left in `doc` chips again.
pub fn restore(doc: &mut Document, chips: &[(Chip, String)]) {
    if chips.is_empty() {
        return;
    }
    for p in &mut doc.paragraphs {
        if !p.runs.iter().any(|r| r.text.contains(OPEN)) {
            continue;
        }
        let mut runs = Vec::new();
        for run in p.runs.drain(..) {
            let mut rest = run.text.as_str();
            while let Some(a) = rest.find(OPEN) {
                let Some(b) = rest[a..].find(CLOSE).map(|b| a + b) else { break };
                if a > 0 {
                    runs.push(Run { text: rest[..a].to_string(), style: run.style.clone() });
                }
                match rest[a + OPEN.len_utf8()..b].parse::<usize>().ok().and_then(|i| chips.get(i)) {
                    Some((chip, label)) => runs.push(chip_run(chip.clone(), label.clone())),
                    None => runs.push(Run { text: rest[a..b + CLOSE.len_utf8()].to_string(), style: run.style.clone() }),
                }
                rest = &rest[b + CLOSE.len_utf8()..];
            }
            if !rest.is_empty() {
                runs.push(Run { text: rest.to_string(), style: run.style.clone() });
            }
        }
        p.runs = runs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chip_run_becomes_a_content_control_and_back() {
        let date = Chip { kind: ChipKind::Date, value: "2026-10-03".into() };
        let link = Chip { kind: ChipKind::Link, value: "https://gnome.org/?a=1&b=2".into() };
        let xml = format!(
            "<w:body><w:p><w:r><w:t>Due </w:t></w:r><w:r><w:t>{}</w:t></w:r>\
             <w:hyperlink r:id=\"rId9\"><w:r><w:rPr><w:rStyle w:val=\"Hyperlink\"/></w:rPr><w:t>{}</w:t></w:r></w:hyperlink></w:p></w:body>",
            sentinel(0),
            sentinel(1)
        );
        let chips = vec![(date.clone(), "3 Oct 2026".to_string()), (link.clone(), "gnome.org".to_string())];
        let written = wrap(&xml, &chips);
        assert!(written.contains("<w:date w:fullDate=\"2026-10-03T00:00:00Z\">"), "{written}");
        assert!(written.contains("w:val=\"letters-chip:link:https://gnome.org/?a=1&amp;b=2\""), "{written}");
        assert!(written.contains("<w:sdtContent><w:hyperlink r:id=\"rId9\">"), "the hyperlink is inside the control: {written}");
        assert!(!written.contains(OPEN), "no sentinel is left");
        let (read, found) = unwrap(&written);
        assert_eq!(found, chips);
        assert!(read.contains(&format!("<w:r><w:t>{}</w:t></w:r>", sentinel(1))), "{read}");
        assert!(!read.contains("w:sdt"));
    }

    #[test]
    fn any_date_control_opens_as_a_date_chip_and_other_controls_are_left_alone() {
        let xml = "<w:p><w:sdt><w:sdtPr><w:date w:fullDate=\"2025-01-31T00:00:00Z\"/></w:sdtPr>\
                   <w:sdtContent><w:r><w:t>31/01/2025</w:t></w:r></w:sdtContent></w:sdt>\
                   <w:sdt><w:sdtPr><w:text/></w:sdtPr><w:sdtContent><w:r><w:t>plain</w:t></w:r></w:sdtContent></w:sdt></w:p>\
                   <w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>toc</w:t></w:r></w:p></w:sdtContent></w:sdt>";
        let (read, found) = unwrap(xml);
        assert_eq!(found, [(Chip { kind: ChipKind::Date, value: "2025-01-31".into() }, "31/01/2025".to_string())]);
        assert!(read.contains("<w:t>plain</w:t>") && read.contains("<w:t>toc</w:t>"), "{read}");
        assert_eq!(read.matches("<w:sdt>").count(), 2, "the plain-text and block controls stay");
    }

    #[test]
    fn restore_splits_runs_at_sentinels() {
        let chips = vec![(Chip { kind: ChipKind::Person, value: "ada@example.org".into() }, "Ada".to_string())];
        let mut doc = Document::from_plain_text(&format!("Ask {} today", sentinel(0)));
        restore(&mut doc, &chips);
        let runs = &doc.paragraphs[0].runs;
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[1].text.as_str(), runs[1].style.link.as_deref()), ("Ada", Some("mailto:ada@example.org")));
        assert_eq!(runs[2].text, " today");
    }
}
