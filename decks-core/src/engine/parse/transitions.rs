// Slide transition parsing for the PPTX reader.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
/// A slide's `p:transition`. PowerPoint writes Morph inside
/// `mc:AlternateContent`: a `p159:morph` Choice and a plain Fallback
/// (usually a fade), so the first transition kind seen wins and Morph,
/// read first, is not overwritten by its fallback. A kind we don't draw
/// reads as a dissolve: the slide still changes with a transition.
pub fn parse_transition(xml: &str) -> Transition {
    let mut reader = Reader::from_str(xml);
    // Nesting below the open p:transition (0: not in one). Only its direct
    // children name the kind; p:sndAc, p:extLst and their insides don't.
    let mut level = 0usize;
    let mut found: Option<Transition> = None;
    let consider = |name: &str, level: usize, found: &mut Option<Transition>| {
        if level == 1 && found.is_none() && name != "p:sndAc" && name != "p:extLst" {
            *found = Some(transition_kind(name));
        }
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == "p:transition" {
                    level = 1;
                } else if level > 0 {
                    consider(e.name().as_ref(), level, &mut found);
                    level += 1;
                }
            }
            Ok(Event::Empty(e)) => {
                if level > 0 {
                    consider(e.name().as_ref(), level, &mut found);
                }
            }
            Ok(Event::End(_)) if level > 0 => level -= 1,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    found.unwrap_or_default()
}

fn transition_kind(name: &str) -> Transition {
    match name {
        n if n.ends_with(":morph") => Transition::MagicMove,
        "p:push" | "p:cover" | "p:pull" => Transition::Push,
        "p:wipe" | "p:wedge" | "p:split" => Transition::Wipe,
        _ => Transition::Fade,
    }
}
