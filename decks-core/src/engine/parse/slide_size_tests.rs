// SPDX-License-Identifier: GPL-3.0-or-later
// Tests moved out of parse.rs (#1668).


#[test]
fn transitions_are_read_including_powerpoints_morph() {
    let sld = |t: &str| format!("<p:sld xmlns:p=\"p\"><p:cSld><p:spTree/></p:cSld>{t}</p:sld>");
    assert_eq!(parse_transition(&sld("")), Transition::None);
    assert_eq!(parse_transition(&sld("<p:transition spd=\"med\"><p:fade/></p:transition>")), Transition::Fade);
    assert_eq!(parse_transition(&sld("<p:transition><p:push dir=\"u\"/></p:transition>")), Transition::Push);
    assert_eq!(parse_transition(&sld("<p:transition><p:wipe/></p:transition>")), Transition::Wipe);
    assert_eq!(parse_transition(&sld("<p:transition><p:dissolve/></p:transition>")), Transition::Fade, "one we don't draw");
    // As PowerPoint writes Morph: the Choice first, a fade Fallback.
    let morph = "<mc:AlternateContent xmlns:mc=\"mc\"><mc:Choice xmlns:p159=\"x\" Requires=\"p159\">\
        <p:transition spd=\"slow\"><p159:morph option=\"byObject\"/></p:transition></mc:Choice>\
        <mc:Fallback><p:transition spd=\"slow\"><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>";
    assert_eq!(parse_transition(&sld(morph)), Transition::MagicMove);
    // Sound on a transition is not a kind.
    assert_eq!(parse_transition(&sld("<p:transition><p:sndAc/><p:push/></p:transition>")), Transition::Push);
    assert_eq!(
        parse_transition(&sld("<p:transition><p:sndAc><p:stSnd><p:snd/></p:stSnd></p:sndAc><p:wipe/></p:transition>")),
        Transition::Wipe
    );
}


#[test]
fn text_sizes_scale_with_the_slide_like_its_geometry() {
    use suite_common_core::document::{Run, RunStyle};
    // Our own 10in slide: sizes are untouched, including "no size".
    let ours = SlideScale::default();
    let runs = vec![Run { text: "a".into(), style: RunStyle { font_size_hp: Some(48), ..Default::default() } }];
    assert_eq!(ours.text_runs(&runs)[0].style.font_size_hp, Some(48));
    // A 13.33in (12192000 EMU) deck maps onto the 10in model at 0.75.
    let wide = SlideScale::from_emu(12_192_000.0, 6_858_000.0);
    assert!((wide.text_factor() - 0.75).abs() < 1e-9);
    let plain = vec![Run { text: "b".into(), style: RunStyle::default() }];
    assert_eq!(wide.text_runs(&runs)[0].style.font_size_hp, Some(36), "24 pt -> 18 pt");
    assert_eq!(wide.text_runs(&plain)[0].style.font_size_hp, Some(27), "the 18 pt default -> 13.5 pt");
}

use super::*;

const MODERN: &str = "<p:presentation xmlns:p=\"p\">\
    <p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>";

#[test]
fn the_declared_slide_size_is_read() {
    assert_eq!(parse_slide_size(MODERN), Some((12192000.0, 6858000.0)));
}

#[test]
fn a_package_declaring_no_size_reads_as_none() {
    assert_eq!(parse_slide_size("<p:presentation xmlns:p=\"p\"/>"), None);
    assert_eq!(parse_slide_size(""), None);
}

/// A full-bleed shape is full-bleed whatever size the slide declares.
/// On the current PowerPoint default this used to come back as
/// 1280x720 in a 960x540 space.
#[test]
fn a_full_bleed_shape_fills_the_model_space_on_any_slide_size() {
    let scale = SlideScale::from_emu(12192000.0, 6858000.0);
    assert!((12192000.0 * scale.x - 960.0).abs() < 1e-6);
    assert!((6858000.0 * scale.y - 540.0).abs() < 1e-6);
}

/// The default has to be exactly what dividing by 9525 did, or every
/// package that declares no size shifts the day this landed.
#[test]
fn the_default_is_the_old_fixed_divisor() {
    let d = SlideScale::default();
    for emu in [0.0, 9525.0, 190500.0, 9144000.0] {
        assert!((emu * d.x - emu / 9525.0).abs() < 1e-9, "x at {emu}");
        assert!((emu * d.y - emu / 9525.0).abs() < 1e-9, "y at {emu}");
    }
}

/// Both axes are normalised independently, and only a non-16:9 slide
/// can show it: on any 16:9 source — including both common PowerPoint
/// sizes — `960/cx` and `540/cy` are equal, so a mutation using the
/// width factor for both passes against every other fixture here.
///
/// A 4:3 deck cannot be represented faithfully in a fixed 16:9 model
/// space at all. Normalising per axis keeps a full-bleed shape
/// full-bleed and everything on-slide, at the cost of stretching it;
/// the alternative keeps shapes square and pushes content off the
/// bottom, which is the behaviour this replaced.
#[test]
fn a_four_three_slide_is_normalised_on_both_axes() {
    // 10in x 7.5in, the classic 4:3 slide.
    let scale = SlideScale::from_emu(9144000.0, 6858000.0);
    assert!((9144000.0 * scale.x - 960.0).abs() < 1e-6, "width");
    assert!((6858000.0 * scale.y - 540.0).abs() < 1e-6, "height");
    assert!(
        (scale.x - scale.y).abs() > 1e-12,
        "a 4:3 slide must not use one factor for both axes"
    );
}

/// A nonsense or zero extent falls back rather than dividing by it.
#[test]
fn a_zero_extent_falls_back_to_the_default() {
    let z = SlideScale::from_emu(0.0, 0.0);
    let d = SlideScale::default();
    assert!((z.x - d.x).abs() < 1e-12 && (z.y - d.y).abs() < 1e-12);
}
