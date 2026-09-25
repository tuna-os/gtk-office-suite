//! odp_builds.rs — object builds in ODF: a page's animation tree.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! ODF 1.2 (19.x, "presentation" animations) stores a page's effects as a
//! SMIL time tree after its shapes: a timing root, a main sequence, one
//! `anim:par` per click, and inside it an effect `anim:par` carrying
//! `presentation:preset-class` (entrance/exit), `presentation:preset-id` and
//! `presentation:preset-sub-type`, whose animations name their shape by
//! `smil:targetElement` (the shape's `xml:id`/`draw:id`). We write the
//! presets LibreOffice uses for Appear, Fade and Fly, and read any
//! entrance or exit effect (one we don't know reads as a dissolve).

use crate::builds::{Build, BuildEffect, Edge};

fn edge_name(e: Edge, out: bool) -> &'static str {
    match (e, out) {
        (Edge::Left, false) => "from-left",
        (Edge::Right, false) => "from-right",
        (Edge::Top, false) => "from-top",
        (Edge::Bottom, false) => "from-bottom",
        (Edge::Left, true) => "to-left",
        (Edge::Right, true) => "to-right",
        (Edge::Top, true) => "to-top",
        (Edge::Bottom, true) => "to-bottom",
    }
}

/// The effect a preset names.
pub(crate) fn effect_of(preset: &str, sub_type: Option<&str>) -> BuildEffect {
    if preset.ends_with("-appear") || preset.ends_with("-disappear") {
        return BuildEffect::Appear;
    }
    if preset.contains("fly") {
        let edge = match sub_type.unwrap_or("") {
            s if s.ends_with("left") => Edge::Left,
            s if s.ends_with("right") => Edge::Right,
            s if s.ends_with("top") => Edge::Top,
            _ => Edge::Bottom,
        };
        return BuildEffect::Move(edge);
    }
    BuildEffect::Dissolve
}

/// A page's `anim:par` timing tree, or `None` without builds. `id(i)` is
/// the xml:id the writer gave object `i`.
pub(crate) fn animations_xml(builds: &[Build], id: impl Fn(usize) -> String) -> Option<String> {
    if builds.is_empty() {
        return None;
    }
    let mut clicks = String::new();
    for b in builds {
        let target = id(b.object);
        let (class, preset, sub) = match (b.effect, b.out) {
            (BuildEffect::Appear, false) => ("entrance", "ooo-entrance-appear", None),
            (BuildEffect::Dissolve, false) => ("entrance", "ooo-entrance-fade-in", None),
            (BuildEffect::Move(e), false) => ("entrance", "ooo-entrance-fly-in", Some(edge_name(e, false))),
            (BuildEffect::Appear, true) => ("exit", "ooo-exit-disappear", None),
            (BuildEffect::Dissolve, true) => ("exit", "ooo-exit-fade-out", None),
            (BuildEffect::Move(e), true) => ("exit", "ooo-exit-fly-out", Some(edge_name(e, true))),
        };
        let sub_attr = sub.map(|s| format!(" presentation:preset-sub-type=\"{s}\"")).unwrap_or_default();
        let (vis, vis_begin) = if b.out { ("hidden", "0.5s") } else { ("visible", "0s") };
        let set = format!(
            "<anim:set smil:begin=\"{vis_begin}\" smil:dur=\"0.001s\" smil:fill=\"hold\" smil:targetElement=\"{target}\" \
             smil:attributeName=\"visibility\" smil:to=\"{vis}\"/>"
        );
        let motion = match b.effect {
            BuildEffect::Appear => String::new(),
            BuildEffect::Dissolve => format!(
                "<anim:transitionFilter smil:dur=\"0.5s\" smil:targetElement=\"{target}\" smil:type=\"fade\" smil:subtype=\"crossfade\"{}/>",
                if b.out { " smil:mode=\"out\"" } else { "" }
            ),
            BuildEffect::Move(e) => {
                let (attr, off) = match e {
                    Edge::Left => ("x", "0-width/2"),
                    Edge::Right => ("x", "1+width/2"),
                    Edge::Top => ("y", "0-height/2"),
                    Edge::Bottom => ("y", "1+height/2"),
                };
                let values = if b.out { format!("{attr};{off}") } else { format!("{off};{attr}") };
                format!(
                    "<anim:animate smil:dur=\"0.5s\" smil:fill=\"hold\" smil:targetElement=\"{target}\" \
                     smil:attributeName=\"{attr}\" smil:values=\"{values}\" smil:keyTimes=\"0;1\" presentation:additive=\"base\"/>"
                )
            }
        };
        let effect = if b.out { format!("{motion}{set}") } else { format!("{set}{motion}") };
        clicks.push_str(&format!(
            "<anim:par smil:begin=\"next\"><anim:par smil:begin=\"0s\">\
             <anim:par smil:begin=\"0s\" smil:fill=\"hold\" presentation:node-type=\"on-click\" \
             presentation:preset-class=\"{class}\" presentation:preset-id=\"{preset}\"{sub_attr}>{effect}</anim:par>\
             </anim:par></anim:par>"
        ));
    }
    Some(format!(
        "<anim:par presentation:node-type=\"timing-root\"><anim:seq presentation:node-type=\"main-sequence\">{clicks}</anim:seq></anim:par>"
    ))
}

/// Reading a page's builds, event by event, alongside the page walker.
#[derive(Default)]
pub(crate) struct BuildReader {
    /// (out, preset, sub-type, target) of the effect being read, and the
    /// elements open inside it.
    effect: Option<(bool, String, Option<String>, Option<String>)>,
    depth: usize,
    /// Effects read, with their target xml:ids.
    pub found: Vec<(bool, BuildEffect, String)>,
}

impl BuildReader {
    pub(crate) fn start(&mut self, e: &quick_xml::events::BytesStart, empty: bool) {
        let attr = |k: &str| crate::odp::attr_of(e, k);
        if let Some(effect) = self.effect.as_mut() {
            if effect.3.is_none() {
                effect.3 = attr("smil:targetElement");
            }
            if !empty {
                self.depth += 1;
            }
            return;
        }
        if e.name().as_ref() == "anim:par" {
            let out = match attr("presentation:preset-class").as_deref() {
                Some("entrance") => false,
                Some("exit") => true,
                _ => return,
            };
            if empty {
                return;
            }
            self.effect = Some((out, attr("presentation:preset-id").unwrap_or_default(), attr("presentation:preset-sub-type"), None));
            self.depth = 0;
        }
    }

    pub(crate) fn end(&mut self) {
        if self.effect.is_none() {
            return;
        }
        if self.depth > 0 {
            self.depth -= 1;
            return;
        }
        if let Some((out, preset, sub, Some(target))) = self.effect.take() {
            self.found.push((out, effect_of(&preset, sub.as_deref()), target));
        }
        self.effect = None;
    }

    /// The builds, with targets mapped to object indices by `ids` (each
    /// object's xml:id, as read).
    pub(crate) fn builds(&self, ids: &[Option<String>]) -> Vec<Build> {
        self.found
            .iter()
            .filter_map(|(out, effect, target)| {
                let object = ids.iter().position(|i| i.as_deref() == Some(target.as_str()))?;
                Some(Build { object, effect: *effect, out: *out })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::Event;

    fn read(xml: &str) -> Vec<(bool, BuildEffect, String)> {
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut r = BuildReader::default();
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => r.start(&e, false),
                Ok(Event::Empty(e)) => r.start(&e, true),
                Ok(Event::End(_)) => r.end(),
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        r.found
    }

    #[test]
    fn builds_round_trip_through_the_animation_tree() {
        let builds = vec![
            Build { object: 1, effect: BuildEffect::Dissolve, out: false },
            Build { object: 0, effect: BuildEffect::Move(Edge::Left), out: false },
            Build { object: 2, effect: BuildEffect::Appear, out: false },
            Build { object: 1, effect: BuildEffect::Move(Edge::Bottom), out: true },
            Build { object: 0, effect: BuildEffect::Dissolve, out: true },
        ];
        let xml = animations_xml(&builds, |i| format!("o{i}")).unwrap();
        let found = read(&xml);
        let ids: Vec<Option<String>> = (0..3).map(|i| Some(format!("o{i}"))).collect();
        let r = BuildReader { found, ..Default::default() };
        assert_eq!(r.builds(&ids), builds);
        assert_eq!(animations_xml(&[], |_| String::new()), None);
    }

    #[test]
    fn libreoffices_presets_read_as_the_nearest_effect() {
        assert_eq!(effect_of("ooo-entrance-wipe", Some("from-left")), BuildEffect::Dissolve);
        assert_eq!(effect_of("ooo-entrance-fly-in", Some("from-top")), BuildEffect::Move(Edge::Top));
        assert_eq!(effect_of("ooo-exit-fly-out", Some("from-right")), BuildEffect::Move(Edge::Right));
        assert_eq!(effect_of("ooo-exit-disappear", None), BuildEffect::Appear);
    }
}
