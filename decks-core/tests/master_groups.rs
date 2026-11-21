// master_groups.rs — a layout's grouped decoration is placed by its group.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The CNSC roundtable deck's layouts carry the logo as a group: the word
// "CNSC" in a text box turned 270° in the theme's accent 2, over an icon.
// The group places its children in its own child space (`a:chOff` at
// 10562335 EMU, the group itself at 11624628). The master reader read
// neither the group, the turn nor the theme colour, so the word sat a
// tenth of the slide too far left, level, and in the default colour.

use decks_core::engine::{parse_master_shapes, SlideObject};

const LAYOUT: &str = r#"<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:grpSp><p:nvGrpSpPr><p:cNvPr id="7" name="Group 6"/><p:cNvGrpSpPr/><p:nvPr userDrawn="1"/></p:nvGrpSpPr>
<p:grpSpPr><a:xfrm><a:off x="11624628" y="138853"/><a:ext cx="400110" cy="1410269"/><a:chOff x="10562335" y="743691"/><a:chExt cx="400110" cy="1410269"/></a:xfrm></p:grpSpPr>
<p:sp><p:nvSpPr><p:cNvPr id="8" name="TextBox 7"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm rot="16200000"><a:off x="10336632" y="969394"/><a:ext cx="851515" cy="400110"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>
<p:txBody><a:bodyPr wrap="none"/><a:lstStyle/><a:p><a:r><a:rPr lang="en-CA" sz="2000" b="1"><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:rPr><a:t>CNSC</a:t></a:r></a:p></p:txBody></p:sp>
</p:grpSp>
<p:sp><p:nvSpPr><p:cNvPr id="9" name="After"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:sp>
</p:spTree></p:cSld></p:sldLayout>"#;

#[test]
fn a_grouped_turned_word_is_placed_by_its_group_in_its_theme_colour() {
    let (_, shapes) = parse_master_shapes(LAYOUT);
    let (x, rotation, color) = shapes
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { x, rotation, runs, .. } => Some((*x, *rotation, runs[0].style.color.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text box: {shapes:?}"));
    // Default scale: 9525 EMU a model unit. The box's left in child space
    // (10336632) moves by the group's offset (11624628 - 10562335).
    let want = (10336632.0 + 11624628.0 - 10562335.0) / 9525.0;
    assert!((x - want).abs() < 0.5, "x {x}, want {want}");
    assert!((rotation - 270.0).abs() < 1e-6, "{rotation}");
    assert_eq!(color.as_deref(), Some("ed7d31"), "the theme's accent 2");
    // A shape after the group is not moved by it.
    let after = shapes.iter().find_map(|o| match o {
        SlideObject::Shape { x, y, .. } => Some((*x, *y)),
        _ => None,
    });
    assert_eq!(after, Some((0.0, 0.0)));
}
