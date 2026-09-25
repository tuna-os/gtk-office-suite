// timing.rs — object builds in pptx: a slide's p:timing.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// PowerPoint stores animations as a SMIL-like time tree (ECMA-376 Part 1,
// 19.5): a main sequence of clicks, each click a par of effects, each
// effect a cTn carrying presetClass (entr/exit), presetID and
// presetSubtype and targeting a shape by its cNvPr id. We write one effect
// per click, in the shape PowerPoint writes for Appear (preset 1), Fade
// (10) and Fly (2; subtype 8 left, 2 right, 1 top, 4 bottom), and read any
// click-sequence effect whose preset we know (others read as a dissolve,
// so the object still builds).

use crate::builds::{Build, BuildEffect, Edge};
use quick_xml::events::Event;
use quick_xml::Reader;

fn preset(effect: BuildEffect) -> (u32, u32) {
    match effect {
        BuildEffect::Appear => (1, 0),
        BuildEffect::Dissolve => (10, 0),
        BuildEffect::Move(Edge::Left) => (2, 8),
        BuildEffect::Move(Edge::Right) => (2, 2),
        BuildEffect::Move(Edge::Top) => (2, 1),
        BuildEffect::Move(Edge::Bottom) => (2, 4),
    }
}

fn effect_of(id: u32, subtype: u32) -> BuildEffect {
    match (id, subtype) {
        (1, _) => BuildEffect::Appear,
        (2, 8) => BuildEffect::Move(Edge::Left),
        (2, 2) => BuildEffect::Move(Edge::Right),
        (2, 1) => BuildEffect::Move(Edge::Top),
        (2, 4) => BuildEffect::Move(Edge::Bottom),
        _ => BuildEffect::Dissolve,
    }
}

/// The behaviours of one effect: what PowerPoint animates for it.
fn behaviours(b: &Build, spid: u32, id: &mut u32) -> String {
    let mut next = || {
        *id += 1;
        *id
    };
    let tgt = format!("<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl>");
    // Visible from the start (in) or hidden at the end (out).
    let vis_delay = if b.out { 499 } else { 0 };
    let vis = if b.out { "hidden" } else { "visible" };
    let set = format!(
        "<p:set><p:cBhvr><p:cTn id=\"{}\" dur=\"1\" fill=\"hold\"><p:stCondLst><p:cond delay=\"{vis_delay}\"/></p:stCondLst></p:cTn>{tgt}\
         <p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val=\"{vis}\"/></p:to></p:set>",
        next()
    );
    let dir = if b.out { "out" } else { "in" };
    match b.effect {
        BuildEffect::Appear => set,
        BuildEffect::Dissolve => {
            let fade = format!(
                "<p:animEffect transition=\"{dir}\" filter=\"fade\"><p:cBhvr><p:cTn id=\"{}\" dur=\"500\"/>{tgt}</p:cBhvr></p:animEffect>",
                next()
            );
            if b.out { format!("{fade}{set}") } else { format!("{set}{fade}") }
        }
        BuildEffect::Move(edge) => {
            let (attr, off) = match edge {
                Edge::Left => ("ppt_x", "0-#ppt_w/2"),
                Edge::Right => ("ppt_x", "1+#ppt_w/2"),
                Edge::Top => ("ppt_y", "0-#ppt_h/2"),
                Edge::Bottom => ("ppt_y", "1+#ppt_h/2"),
            };
            let (from, to) = if b.out { (format!("#{attr}"), off.to_string()) } else { (off.to_string(), format!("#{attr}")) };
            let anim = format!(
                "<p:anim calcmode=\"lin\" valueType=\"num\"><p:cBhvr additive=\"base\"><p:cTn id=\"{}\" dur=\"500\" fill=\"hold\"/>{tgt}\
                 <p:attrNameLst><p:attrName>{attr}</p:attrName></p:attrNameLst></p:cBhvr>\
                 <p:tavLst><p:tav tm=\"0\"><p:val><p:strVal val=\"{from}\"/></p:val></p:tav>\
                 <p:tav tm=\"100000\"><p:val><p:strVal val=\"{to}\"/></p:val></p:tav></p:tavLst></p:anim>",
                next()
            );
            if b.out { format!("{anim}{set}") } else { format!("{set}{anim}") }
        }
    }
}

/// A slide's `p:timing`, or `None` without builds. `spid(i)` is the cNvPr
/// id the writer gave object `i`.
pub(crate) fn timing_xml(builds: &[Build], spid: impl Fn(usize) -> u32) -> Option<String> {
    if builds.is_empty() {
        return None;
    }
    let mut id = 2u32;
    let mut clicks = String::new();
    for b in builds {
        let (preset_id, subtype) = preset(b.effect);
        let class = if b.out { "exit" } else { "entr" };
        let click = {
            id += 1;
            id
        };
        let group = {
            id += 1;
            id
        };
        let effect = {
            id += 1;
            id
        };
        let bhvr = behaviours(b, spid(b.object), &mut id);
        clicks.push_str(&format!(
            "<p:par><p:cTn id=\"{click}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst><p:childTnLst>\
             <p:par><p:cTn id=\"{group}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst><p:childTnLst>\
             <p:par><p:cTn id=\"{effect}\" presetID=\"{preset_id}\" presetClass=\"{class}\" presetSubtype=\"{subtype}\" fill=\"hold\" nodeType=\"clickEffect\">\
             <p:stCondLst><p:cond delay=\"0\"/></p:stCondLst><p:childTnLst>{bhvr}</p:childTnLst></p:cTn></p:par>\
             </p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"
        ));
    }
    Some(format!(
        "<p:timing><p:tnLst><p:par><p:cTn id=\"1\" dur=\"indefinite\" restart=\"never\" nodeType=\"tmRoot\"><p:childTnLst>\
         <p:seq concurrent=\"1\" nextAc=\"seek\"><p:cTn id=\"2\" dur=\"indefinite\" nodeType=\"mainSeq\"><p:childTnLst>{clicks}</p:childTnLst></p:cTn>\
         <p:prevCondLst><p:cond evt=\"onPrev\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst>\
         <p:nextCondLst><p:cond evt=\"onNext\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq>\
         </p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"
    ))
}

/// The builds a slide's `p:timing` describes, in order. `object_of(spid)`
/// is the model index of the object with that cNvPr id (None for one the
/// reader didn't keep).
pub(crate) fn read_builds(slide_xml: &str, object_of: impl Fn(u32) -> Option<usize>) -> Vec<Build> {
    let mut reader = Reader::from_str(slide_xml);
    let mut in_timing = false;
    // The effect being read: (class, preset, subtype, target spid).
    let mut effect: Option<(bool, u32, u32, Option<u32>)> = None;
    let mut depth_in_effect = 0usize;
    let mut out = Vec::new();
    let attr = |e: &quick_xml::events::BytesStart, k: &str| -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == k)
            .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
    };
    let finish = |effect: &mut Option<(bool, u32, u32, Option<u32>)>, out: &mut Vec<Build>| {
        if let Some((is_out, id, sub, Some(spid))) = effect.take() {
            if let Some(object) = object_of(spid) {
                out.push(Build { object, effect: effect_of(id, sub), out: is_out });
            }
        }
    };
    loop {
        let (e, empty) = match reader.read_event() {
            Ok(Event::Start(e)) => (e, false),
            Ok(Event::Empty(e)) => (e, true),
            Ok(Event::End(e)) => {
                if e.name().as_ref() == "p:timing" {
                    in_timing = false;
                }
                if effect.is_some() {
                    if depth_in_effect == 0 {
                        finish(&mut effect, &mut out);
                    } else {
                        depth_in_effect -= 1;
                    }
                }
                continue;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => continue,
        };
        match e.name().as_ref() {
            "p:timing" if !empty => in_timing = true,
            "p:cTn" if in_timing && effect.is_none() => {
                let class = attr(&e, "presetClass");
                if let Some(is_out) = class.as_deref().and_then(|c| match c {
                    "entr" => Some(false),
                    "exit" => Some(true),
                    _ => None,
                }) {
                    let id = attr(&e, "presetID").and_then(|v| v.parse().ok()).unwrap_or(10);
                    let sub = attr(&e, "presetSubtype").and_then(|v| v.parse().ok()).unwrap_or(0);
                    if !empty {
                        effect = Some((is_out, id, sub, None));
                        depth_in_effect = 0;
                        continue;
                    }
                }
            }
            "p:spTgt" => {
                if let Some((_, _, _, spid @ None)) = effect.as_mut() {
                    *spid = attr(&e, "spid").and_then(|v| v.parse().ok());
                }
            }
            _ => {}
        }
        if effect.is_some() && !empty {
            depth_in_effect += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_round_trip_through_the_time_tree() {
        let builds = vec![
            Build { object: 1, effect: BuildEffect::Dissolve, out: false },
            Build { object: 0, effect: BuildEffect::Move(Edge::Left), out: false },
            Build { object: 2, effect: BuildEffect::Appear, out: false },
            Build { object: 1, effect: BuildEffect::Move(Edge::Bottom), out: true },
        ];
        let xml = timing_xml(&builds, |i| 2 + i as u32).unwrap();
        let slide = format!("<p:sld xmlns:p=\"p\"><p:cSld/>{xml}</p:sld>");
        let back = read_builds(&slide, |spid| spid.checked_sub(2).map(|i| i as usize));
        assert_eq!(back, builds);
        assert_eq!(timing_xml(&[], |_| 0), None);
    }

    #[test]
    fn a_target_the_reader_dropped_has_no_build_and_an_unknown_preset_dissolves() {
        let slide = r#"<p:sld xmlns:p="p"><p:timing><p:tnLst><p:par><p:cTn id="1"><p:childTnLst>
            <p:par><p:cTn id="5" presetID="42" presetClass="entr"><p:childTnLst><p:set><p:cBhvr><p:cTn id="6"/>
              <p:tgtEl><p:spTgt spid="7"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par>
            <p:par><p:cTn id="8" presetID="10" presetClass="emph"><p:childTnLst><p:set><p:cBhvr><p:cTn id="9"/>
              <p:tgtEl><p:spTgt spid="7"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par>
            <p:par><p:cTn id="10" presetID="1" presetClass="exit"><p:childTnLst><p:set><p:cBhvr><p:cTn id="11"/>
              <p:tgtEl><p:spTgt spid="99"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par>
            </p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>"#;
        let back = read_builds(slide, |spid| (spid == 7).then_some(0));
        assert_eq!(back, vec![Build { object: 0, effect: BuildEffect::Dissolve, out: false }], "emphasis is not a build");
    }
}
