// notes.rs — speaker-notes slide XML and text extraction.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of engine.rs (issue #247).

use quick_xml::events::{Event, BytesStart};
use quick_xml::Reader;
use letters_core::model::RunStyle;

use super::parse::{resolve_general_ref, unescape_text};

/// Minimal notesSlide part with the notes text in a body placeholder.
pub(super) fn notes_slide_xml(notes: &str) -> String {
    let mut paras = String::new();
    for line in notes.split('\n') {
        let escaped = line
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        paras.push_str(&format!("<a:p><a:r><a:t>{}</a:t></a:r></a:p>", escaped));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <p:notes xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:cSld><p:spTree>\
<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>\
<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Notes Placeholder\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
<p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/>\
<p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody></p:sp>\
</p:spTree></p:cSld></p:notes>",
        paras
    )
}

/// Extract the body-placeholder text from a notesSlide part.
pub(super) fn extract_notes_text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    // No trim: a:t content is significant, including boundary spaces
    // ("café — " + "東京"); capture is gated on in_t anyway.
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut in_sp = false;
    let mut sp_is_body = false;
    let mut in_t = false;
    let mut current = String::new();
    let mut parts: Vec<String> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"p:sp" => { in_sp = true; sp_is_body = false; }
                b"a:t" if sp_is_body => in_t = true,
                _ => {}
            },
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"a:br" && sp_is_body => {
                current.push('\n');
            }
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"p:ph" && in_sp => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"type" {
                        if let Ok(v) = attr.decode_and_unescape_value(reader.decoder()) {
                            if v == "body" { sp_is_body = true; }
                        }
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_t => {
                current.push_str(&unescape_text(t));
            }
            Ok(Event::GeneralRef(ref r)) if in_t => {
                current.push_str(&resolve_general_ref(r));
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"a:t" => in_t = false,
                b"a:p" if sp_is_body => {
                    if !current.is_empty() { parts.push(std::mem::take(&mut current)); }
                }
                b"p:sp" => { in_sp = false; sp_is_body = false; }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    parts.join("\n")
}

/// Parse b/i/u/strike attributes from an a:rPr element into the shared
/// RunStyle (same WYSIWYG primitive Letters uses).
pub(super) fn parse_run_style(e: &BytesStart, reader: &Reader<&[u8]>) -> RunStyle {
    let mut st = RunStyle::default();
    for attr in e.attributes().flatten() {
        let val = attr.decode_and_unescape_value(reader.decoder()).unwrap_or_default();
        match attr.key.as_ref() {
            b"b" => st.bold = val == "1" || val == "true",
            b"i" => st.italic = val == "1" || val == "true",
            b"u" => st.underline = val != "none" && !val.is_empty(),
            b"strike" => st.strikethrough = val != "noStrike" && !val.is_empty(),
            b"sz" => {
                if let Ok(hundredths) = val.parse::<u32>() {
                    st.font_size_hp = Some((hundredths / 50) as u16);
                }
            }
            _ => {}
        }
    }
    st

}
