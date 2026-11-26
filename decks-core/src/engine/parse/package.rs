// SPDX-License-Identifier: GPL-3.0-or-later
//! Package-level relationship scans for the PPTX reader (#1668).
//!
//! Contract: these work on already-read part text. ZIP traversal stays in
//! `read_pptx`.

use quick_xml::events::Event;
use quick_xml::Reader;

/// `rId -> target` of the slide relationships in `ppt/_rels/presentation.xml.rels`.
pub(super) fn slide_targets(rels_xml: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut slide_paths = std::collections::BTreeMap::new();
    {
        let mut reader = Reader::from_str(rels_xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == "Relationship" {
                        let mut id = None;
                        let mut target = None;
                        let mut is_slide = false;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                "Id" => {
                                    id = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned());
                                }
                                "Target" => {
                                    target = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned());
                                }
                                "Type" => {
                                    if let Ok(v) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                                        if v.contains("relationships/slide") {
                                            is_slide = true;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        if is_slide {
                            if let (Some(id_val), Some(target_val)) = (id, target) {
                                slide_paths.insert(id_val, target_val);
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parsing error in presentation.xml.rels: {}", e)),
                _ => {}
            }
            buf.clear();
        }
    }
    Ok(slide_paths)
}

/// The `r:id`s of `p:sldId` in presentation order.
pub(super) fn ordered_slide_rids(presentation_xml: &str) -> Result<Vec<String>, String> {
    let mut ordered_slide_rids = Vec::new();
    {
        let mut reader = Reader::from_str(presentation_xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == "p:sldId" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "r:id" {
                                if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                                    ordered_slide_rids.push(val.into_owned());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parsing error in presentation.xml: {}", e)),
                _ => {}
            }
            buf.clear();
        }
    }
    Ok(ordered_slide_rids)
}
