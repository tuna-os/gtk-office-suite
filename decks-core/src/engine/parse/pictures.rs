// Picture relationship, crop, and extraction helpers for the PPTX reader.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
pub(super) struct PendingPicture {
    pub(super) embed_id: Option<String>,
    pub(super) x: Option<f64>,
    pub(super) y: Option<f64>,
    pub(super) w: Option<f64>,
    pub(super) h: Option<f64>,
    pub(super) rotation: Option<f64>,
    pub(super) crop: crate::engine::Crop,
}

/// A picture's `a:srcRect`: each side in thousandths of a percent of the
/// source (`l="25000"` cuts a quarter from the left).
pub(super) fn parse_src_rect(e: &BytesStart) -> crate::engine::Crop {
    let mut crop = crate::engine::Crop::default();
    for a in e.attributes().filter_map(|a| a.ok()) {
        let v = a.value.parse::<f64>().unwrap_or(0.0) / 100_000.0;
        match a.key.as_ref() {
            "l" => crop.left = v,
            "t" => crop.top = v,
            "r" => crop.right = v,
            "b" => crop.bottom = v,
            _ => {}
        }
    }
    crop
}

/// A stretched picture's `a:fillRect`: insets from each edge of the box it
/// fills, in thousandths of a percent; negative reaches past the edge.
/// Each inset is bounded to ±10 boxes so a hostile value cannot place the
/// picture at infinity.
pub(super) fn parse_fill_rect(e: &BytesStart) -> crate::engine::FillRect {
    let mut fill = crate::engine::FillRect::default();
    for a in e.attributes().filter_map(|a| a.ok()) {
        let v = a.value.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map_or(0.0, |v| (v / 100_000.0).clamp(-10.0, 10.0));
        match a.key.as_ref() {
            "l" => fill.left = v,
            "t" => fill.top = v,
            "r" => fill.right = v,
            "b" => fill.bottom = v,
            _ => {}
        }
    }
    fill
}
pub(super) fn rel_target(rels_xml: &str, id: &str) -> Option<String> {
    let mut reader = Reader::from_str(rels_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "Relationship" => {
                let attr = |k: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == k)
                        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
                };
                if attr("Id").as_deref() == Some(id) {
                    return attr("Target");
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// A picture of the part `part` (relationships `rels_xml`) by its
/// relationship id, read into the media cache: its file's path.
pub(super) fn part_picture(archive: &mut zip::ZipArchive<File>, budget: &mut ZipBudget, part: &str, rels_xml: &str, id: &str) -> Option<String> {
    let dir = Path::new(part).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let target = part_path(&dir, &rel_target(rels_xml, id)?);
    let bytes = archive.part_to_bytes(&target, budget).ok()?;
    suite_common_core::media_cache::persist(&bytes).ok().map(|p| p.to_string_lossy().into_owned())
}

/// Position and crop of a picture on the slide.
pub(super) struct PictureRect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    pub(super) h: f64,
    pub(super) rotation: f64,
    pub(super) crop: crate::engine::Crop,
}

/// The package path a relationship `target` names, from a part in `dir`:
/// "../charts/chart1.xml" from "ppt/slides" is "ppt/charts/chart1.xml",
/// and "/ppt/charts/chart1.xml" is package-absolute.
pub(super) fn part_path(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

pub(super) fn resolve_and_extract_picture(
    embed_id: &str,
    rect: PictureRect,
    rels: &std::collections::HashMap<String, String>,
    archive: &mut zip::ZipArchive<File>,
    budget: &mut ZipBudget,
) -> Option<SlideObject> {
    let PictureRect { x, y, w, h, rotation, crop } = rect;
    let target = rels.get(embed_id)?;
    let relative_path = target.trim_start_matches("../");
    let full_zip_path = format!("ppt/{}", relative_path);

    // An image is the largest part a real deck has and the easiest to lie
    // about, so it draws on the same budget as the XML parts.
    let buffer = archive.part_to_bytes(&full_zip_path, budget).ok()?;

    // gh-268: the previous code wrote to a predictable /tmp/decks_img_<embed_id>.<ext>
    // path whose middle (embed_id) and suffix (extension) both came from the
    // untrusted document. A crafted PPTX could point the write anywhere via `..`
    // or a pre-created symlink. NamedTempFile gives O_EXCL + O_NOFOLLOW + an
    // unpredictable name in one step. The media cache keeps those
    // properties inside a private 0700 per-process directory. The model
    // reads the file back later, so it lives as long as the process;
    // identical pictures share one file, and the next process sweeps the
    // directory once this one is gone (#455).
    let output_path = suite_common_core::media_cache::persist(&buffer).ok()?;

    Some(SlideObject::Image {
        path: output_path.to_string_lossy().to_string(),
        x,
        y,
        w,
        h,
        rotation,
        crop,
    })
}
