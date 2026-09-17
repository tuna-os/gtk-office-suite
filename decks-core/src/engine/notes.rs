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
/// The `type` a `p:ph` declares, if any. A placeholder with no type is
/// still a placeholder, which is why the caller tracks that separately.
fn ph_type(e: &quick_xml::events::BytesStart) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "type" {
            if let Ok(v) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub(super) fn extract_notes_text(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    // No trim: a:t content is significant, including boundary spaces
    // ("café — " + "東京"); capture is gated on in_t anyway.
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut in_sp = false;
    // The placeholder this shape declares, if it declares one at all. Both
    // matter and they are not the same question: a shape with no `p:ph` is
    // a candidate, a shape holding the slide number is not.
    let mut sp_ph: Option<String> = None;
    let mut sp_has_ph = false;
    let mut in_t = false;
    let mut current = String::new();
    let mut sp_parts: Vec<String> = Vec::new();
    // Notes written into a `type="body"` placeholder, which is what our own
    // writer emits and what PowerPoint uses.
    let mut body_parts: Vec<String> = Vec::new();
    // Notes written into a shape with no placeholder at all. Impress's pptx
    // exporter writes exactly that — one `p:sp` whose `p:nvPr` is empty —
    // so requiring the body placeholder dropped the speaker notes of every
    // deck that had been through it. Used only when no body placeholder
    // produced anything, and never for a shape that declares some *other*
    // placeholder, so a slide number or footer can never be read as notes.
    let mut unplaceheld_parts: Vec<String> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                "p:sp" => {
                    in_sp = true;
                    sp_ph = None;
                    sp_has_ph = false;
                    sp_parts.clear();
                }
                "a:t" if in_sp => in_t = true,
                "p:ph" if in_sp => {
                    sp_has_ph = true;
                    sp_ph = ph_type(e);
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) if e.name().as_ref() == "a:br" && in_sp => {
                current.push('\n');
            }
            Ok(Event::Empty(ref e)) if e.name().as_ref() == "p:ph" && in_sp => {
                sp_has_ph = true;
                sp_ph = ph_type(e);
            }
            Ok(Event::Text(ref t)) if in_t => {
                current.push_str(&unescape_text(t));
            }
            Ok(Event::GeneralRef(ref r)) if in_t => {
                current.push_str(&resolve_general_ref(r));
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                "a:t" => in_t = false,
                "a:p" if in_sp => {
                    if !current.is_empty() {
                        sp_parts.push(std::mem::take(&mut current));
                    }
                }
                "p:sp" => {
                    if !sp_parts.is_empty() {
                        match (sp_has_ph, sp_ph.as_deref()) {
                            (true, Some("body")) => body_parts.append(&mut sp_parts),
                            // A placeholder that is not the body one: a slide
                            // number, date or footer. Never notes.
                            (true, _) => sp_parts.clear(),
                            (false, _) => unplaceheld_parts.append(&mut sp_parts),
                        }
                    }
                    in_sp = false;
                    sp_ph = None;
                    sp_has_ph = false;
                    current.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    if !body_parts.is_empty() {
        body_parts.join("\n")
    } else {
        unplaceheld_parts.join("\n")
    }
}


/// Parse b/i/u/strike attributes from an a:rPr element into the shared
/// RunStyle (same WYSIWYG primitive Letters uses).
pub(super) fn parse_run_style(e: &BytesStart) -> RunStyle {
    let mut st = RunStyle::default();
    for attr in e.attributes().flatten() {
        let val = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).unwrap_or_default();
        match attr.key.as_ref() {
            "b" => st.bold = val == "1" || val == "true",
            "i" => st.italic = val == "1" || val == "true",
            "u" => st.underline = val != "none" && !val.is_empty(),
            "strike" => st.strikethrough = val != "noStrike" && !val.is_empty(),
            "sz" => {
                if let Ok(hundredths) = val.parse::<u32>() {
                    st.font_size_hp = Some((hundredths / 50) as u16);
                }
            }
            _ => {}
        }
    }
    st

}
