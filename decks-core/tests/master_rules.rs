// master_rules.rs — a master's rule under the title is kept.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The future-delivery-structure deck's masters rule a blue line under the
// title (`p:cxnSp`, a straightConnector1 of no height). The master reader
// read only `p:sp`, and only those with both a width and a height, so
// every slide on such a master lost its rule: 68 connectors across nine
// real decks' masters and layouts.

use decks_core::engine::shape::{is_line, Color, ShapeKind};
use decks_core::engine::{parse_master_shapes, SlideObject};

const MASTER: &str = r#"<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="7" name="Straight Arrow Connector 6"/><p:cNvCxnSpPr/><p:nvPr userDrawn="1"/></p:nvCxnSpPr><p:spPr><a:xfrm flipH="1"><a:off x="0" y="914363"/><a:ext cx="12191319" cy="0"/></a:xfrm><a:prstGeom prst="straightConnector1"><a:avLst/></a:prstGeom><a:ln w="28575"><a:solidFill><a:srgbClr val="00B0F0"/></a:solidFill></a:ln></p:spPr></p:cxnSp>
<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="9" name="Elbow Connector 8"/><p:cNvCxnSpPr/><p:nvPr userDrawn="1"/></p:nvCxnSpPr><p:spPr><a:xfrm><a:off x="0" y="3000000"/><a:ext cx="4000000" cy="0"/></a:xfrm><a:prstGeom prst="bentConnector3"><a:avLst/></a:prstGeom><a:ln w="12700"><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></a:ln></p:spPr></p:cxnSp>
<p:sp><p:nvSpPr><p:cNvPr id="8" name="Line"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="6000000"/><a:ext cx="6000000" cy="0"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:ln w="12700"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:ln></p:spPr></p:sp>
</p:spTree></p:cSld></p:sldMaster>"#;

#[test]
fn a_masters_connector_and_flat_line_are_kept() {
    let (_, shapes) = parse_master_shapes(MASTER);
    let lines: Vec<_> = shapes
        .iter()
        .filter_map(|o| match o {
            SlideObject::Shape { kind, w, h, style, .. } if is_line(kind) => Some((kind.clone(), *w, *h, style.stroke.map(|s| s.color))),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 3, "{shapes:?}");
    assert_eq!(lines[0].0, ShapeKind::Other("straightConnector1".into()));
    assert!(lines[0].1 > 0.0 && lines[0].2 == 0.0, "{lines:?}");
    assert_eq!(lines[0].3, Some(Color(0x00, 0xB0, 0xF0)), "the connector's own colour");
    // An elbow connector whose ends are level is a straight segment.
    assert_eq!((lines[1].0.clone(), lines[1].3), (ShapeKind::from_prst("line"), Some(Color(0x00, 0xFF, 0x00))), "{lines:?}");
    assert_eq!(lines[2].3, Some(Color(0xFF, 0x00, 0x00)), "a flat line shape too");
}
