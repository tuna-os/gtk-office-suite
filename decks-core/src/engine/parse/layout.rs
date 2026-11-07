// Slide layout and theme relationship parsing for the PPTX reader.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
pub(super) fn read_layout_placeholders(
    archive: &mut zip::ZipArchive<File>,
    budget: &mut ZipBudget,
    layout_path: &str,
    theme: &Theme,
) -> (Vec<Placeholder>, Vec<Placeholder>, Inherited) {
    let layout_xml = archive.optional_part_to_string(layout_path, budget);
    let dir = Path::new(layout_path).parent().unwrap_or(Path::new("ppt"));
    let file = Path::new(layout_path).file_name().unwrap_or_default().to_string_lossy();
    let rels = archive.optional_part_to_string(&format!("{}/_rels/{}.rels", dir.to_string_lossy(), file), budget);
    let master_xml = rels
        .split("Target=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .find(|t| t.contains("slideMaster"))
        .map(|t| archive.optional_part_to_string(&format!("ppt/{}", t.trim_start_matches("../")), budget))
        .unwrap_or_default();
    let inherited = Inherited::read(&layout_xml, &master_xml, theme);
    (parse_placeholders(&layout_xml), parse_placeholders(&master_xml), inherited)
}

/// A layout's master and theme parts, through its relationships: the
/// layout names its master, the master names its theme (ECMA-376 gives
/// every `p:sldMaster` exactly one theme relationship). Either link may
/// be absent — what this writer used to emit — and then there is simply
/// no part on that side.
pub(super) fn layout_master_theme_paths(
    archive: &mut zip::ZipArchive<File>,
    budget: &mut ZipBudget,
    layout_path: &str,
) -> (Option<String>, Option<String>) {
    let rels_of = |archive: &mut zip::ZipArchive<File>, budget: &mut ZipBudget, part: &str| -> String {
        let dir = Path::new(part).parent().unwrap_or(Path::new("ppt"));
        let file = Path::new(part).file_name().unwrap_or_default().to_string_lossy().to_string();
        archive.optional_part_to_string(&format!("{}/_rels/{}.rels", dir.to_string_lossy(), file), budget)
    };
    let target = |base: &str, rels: &str, kind: &str| -> Option<String> {
        rels.split("Target=\"")
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .find(|t| t.contains(kind))
            .map(|t| part_path(base, t))
    };
    let dir = Path::new(layout_path).parent().unwrap_or(Path::new("ppt")).to_string_lossy().into_owned();
    let layout_rels = rels_of(archive, budget, layout_path);
    let master = target(&dir, &layout_rels, "slideMaster");
    let theme = master.as_deref().and_then(|mp| {
        let dir = Path::new(mp).parent().unwrap_or(Path::new("ppt")).to_string_lossy().into_owned();
        target(&dir, &rels_of(archive, budget, mp), "theme")
    });
    (master, theme)
}

/// The theme the slides on `layout_path` answer to (DCK-006): the theme
/// part their master names, or the default theme when the chain names
/// none. Theme parts are cached by part path and shared with the master
/// builder below, so slides and masters resolve against the same theme.
/// Returns the theme and its part path, if it names one.
pub(super) fn layout_theme(
    archive: &mut zip::ZipArchive<File>,
    budget: &mut ZipBudget,
    layout_path: &str,
    theme_path_of: &mut std::collections::HashMap<String, Option<String>>,
    themes: &mut std::collections::HashMap<String, Theme>,
) -> (Theme, Option<String>) {
    if !theme_path_of.contains_key(layout_path) {
        let (_, theme) = layout_master_theme_paths(archive, budget, layout_path);
        theme_path_of.insert(layout_path.to_string(), theme);
    }
    let path = theme_path_of.get(layout_path).and_then(|t| t.clone());
    let theme = path
        .as_deref()
        .map(|p| {
            if !themes.contains_key(p) {
                let xml = archive.optional_part_to_string(p, budget);
                themes.insert(p.to_string(), read_theme(&xml));
            }
            themes.get(p).cloned().unwrap_or_default()
        })
        .unwrap_or_default();
    (theme, path)
}

/// A master part's colour map (`p:clrMap`): presentation names (`bg1`,
/// `tx1` …) to the theme slots they draw from. Empty when the part
/// states none — the identity mapping every such package assumes.
pub(super) fn parse_color_map(xml: &str) -> std::collections::BTreeMap<String, String> {
    if xml.is_empty() {
        return Default::default();
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) if e.name().as_ref() == "p:clrMap" => {
                let mut map = std::collections::BTreeMap::new();
                for attr in e.attributes().flatten() {
                    let key: String = attr.key.as_ref().to_string();
                    if key.contains(':') || key.eq_ignore_ascii_case("xmlns") {
                        continue;
                    }
                    if let Ok(v) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                        let v = v.trim().to_string();
                        if !v.is_empty() {
                            map.insert(key, v);
                        }
                    }
                }
                return map;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    Default::default()
}

/// The slideLayout part a slide's relationships name, if any.
pub(super) fn layout_path_of(rels: &std::collections::HashMap<String, String>) -> Option<String> {
    rels.values().find(|t| t.contains("slideLayout")).map(|t| format!("ppt/{}", t.trim_start_matches("../")))
}

/// A layout part as the model's layout: its name and kind, its own
/// background (when it differs from the master's), its decorations, and the
/// places of its title, subtitle and body placeholders (a place it leaves
/// to the master is the master's).
pub(super) fn read_layout(
    xml: &str,
    path: &str,
    master_phs: &[Placeholder],
    master_bg: Option<&str>,
    scale: SlideScale,
    theme: &Theme,
    pictures: &mut dyn FnMut(&str) -> Option<String>,
) -> crate::layouts::Layout {
    use crate::layouts::{Layout, LayoutKind, LayoutPlaceholder};
    let (bg, shapes) = master_shapes(xml, scale, theme, pictures);
    let kind = {
        let mut reader = Reader::from_str(xml);
        let mut kind = LayoutKind::Custom;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "p:sldLayout" => {
                    let ty = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "type")
                        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.to_string()));
                    kind = LayoutKind::from_pptx(ty.as_deref());
                    break;
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        kind
    };
    let layout_phs = parse_placeholders(xml);
    let placeholders = layout_phs
        .iter()
        .filter_map(|p| {
            let role = crate::layouts::Placeholder::from_pptx(Some(&p.key.ty))?;
            let r = p.rect.or_else(|| inherited_rect(&p.key, &layout_phs, master_phs))?;
            Some(LayoutPlaceholder { id: 0, role, x: r.x * scale.x, y: r.y * scale.y, w: r.w * scale.x, h: r.h * scale.y, source_ty: p.key.ty.clone(), source_idx: p.key.idx })
        })
        .collect();
    let name = parse_c_sld_name(xml).unwrap_or_else(|| {
        Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Layout".into())
    });
    // Ids are stamped by the caller from the session space (DCK-005):
    // 0 here means not assigned yet.
    Layout { id: 0, name, kind, background: bg.filter(|b| Some(b.as_str()) != master_bg), shapes, placeholders }
}

/// A part's own background, if it states one.
pub(super) fn parse_layout_background(xml: &str, scale: SlideScale, theme: &Theme) -> Option<String> {
    master_shapes(xml, scale, theme, &mut |_| None).0
}
