// SPDX-License-Identifier: GPL-3.0-or-later
// Tests moved out of parse.rs (#1668).

use super::super::shape_xml::theme;

/// The heading and body fonts differ, which is the only arrangement
/// that can tell a correct reader from one that returns the first
/// `a:latin` it sees. With both set to the same face this test would
/// pass against either.
#[test]
fn the_body_font_is_read_not_the_heading_font() {
    let xml = "<a:theme><a:themeElements><a:fontScheme>\
        <a:majorFont><a:latin typeface=\"Heading Face\"/></a:majorFont>\
        <a:minorFont><a:latin typeface=\"Body Face\"/></a:minorFont>\
        </a:fontScheme></a:themeElements></a:theme>";
    assert_eq!(theme(xml).fonts(), (Some("Heading Face".into()), Some("Body Face".into())));
}

/// `a:cs` and `a:ea` sit beside `a:latin` inside the same scheme, and
/// the writer emits them empty.
#[test]
fn the_latin_typeface_is_read_not_its_siblings() {
    let xml = "<a:fontScheme><a:minorFont>\
        <a:latin typeface=\"Body Face\"/><a:ea typeface=\"\"/>\
        <a:cs typeface=\"\"/></a:minorFont></a:fontScheme>";
    assert_eq!(theme(xml).fonts().1.as_deref(), Some("Body Face"));
}

#[test]
fn an_empty_typeface_is_no_answer_rather_than_an_empty_one() {
    let xml = "<a:fontScheme><a:minorFont><a:latin typeface=\"\"/>\
        </a:minorFont></a:fontScheme>";
    assert_eq!(theme(xml).fonts(), (None, None));
}

/// A package written before the theme part existed — which is every
/// pptx this writer produced until now — has to fall back, not fail.
#[test]
fn a_part_with_no_font_scheme_answers_nothing() {
    assert_eq!(theme("<a:theme><a:themeElements/></a:theme>").fonts(), (None, None));
    assert_eq!(theme("").fonts(), (None, None));
    assert_eq!(theme("not xml at all <<<").fonts(), (None, None));
}
