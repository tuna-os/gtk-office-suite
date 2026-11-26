// SPDX-License-Identifier: GPL-3.0-or-later
// Tests moved out of parse.rs (#1668).

use super::*;
use crate::engine::notes::{extract_notes_text, notes_slide_xml};

#[test]
fn master_parser_skips_placeholders_keeps_decorations() {
    let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld>
        <p:bg><p:bgPr><a:solidFill><a:srgbClr val="1A2B3C"/></a:solidFill></p:bgPr></p:bg>
        <p:spTree>
        <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
          <p:spPr><a:xfrm><a:off x="9525" y="9525"/><a:ext cx="95250" cy="95250"/></a:xfrm></p:spPr>
          <p:txBody><a:p><a:r><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp>
        <p:sp><p:spPr><a:xfrm><a:off x="19050" y="28575"/><a:ext cx="190500" cy="95250"/></a:xfrm>
          <a:prstGeom prst="rect"/></p:spPr></p:sp>
        </p:spTree></p:cSld></p:sldMaster>"##;
    let (bg, shapes) = parse_master_shapes(xml);
    assert_eq!(bg.as_deref(), Some("#1a2b3c"));
    assert_eq!(shapes.len(), 1, "placeholder must be skipped: {shapes:?}");
    match &shapes[0] {
        SlideObject::Shape { kind: ShapeKind::Rect, x, y, w, h, .. } => {
            assert!((x - 2.0).abs() < 0.01 && (y - 3.0).abs() < 0.01);
            assert!((w - 20.0).abs() < 0.01 && (h - 10.0).abs() < 0.01);
        }
        other => panic!("expected rect, got {other:?}"),
    }
}

#[test]
fn a_masters_decorations_keep_their_preset_fill_gradient_and_outline() {
    use crate::engine::shape::{Color, GradientStop, LinearGradient, Stroke};
    let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
        <p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="95250" cy="95250"/></a:xfrm>
          <a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="E01B24"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>
        <p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="95250" cy="95250"/></a:xfrm>
          <a:prstGeom prst="ellipse"/><a:gradFill><a:gsLst><a:gs pos="0"><a:srgbClr val="0B3D6B"/></a:gs>
          <a:gs pos="100000"><a:srgbClr val="138D9C"/></a:gs></a:gsLst><a:lin ang="5400000"/></a:gradFill>
          <a:ln w="19050"><a:solidFill><a:srgbClr val="B55A3C"/></a:solidFill></a:ln></p:spPr></p:sp>
        </p:spTree></p:cSld></p:sldMaster>"##;
    let (_, shapes) = parse_master_shapes(xml);
    match &shapes[0] {
        SlideObject::Shape { kind: ShapeKind::Rect, style, .. } => {
            assert_eq!(style.fill, Some(Color(0xE0, 0x1B, 0x24)));
            assert_eq!(style.stroke, None);
        }
        other => panic!("expected the red bar, got {other:?}"),
    }
    match &shapes[1] {
        SlideObject::Shape { kind: ShapeKind::Ellipse, style, .. } => {
            let want = LinearGradient {
                stops: vec![GradientStop { pos: 0.0, color: Color(0x0B, 0x3D, 0x6B) }, GradientStop { pos: 1.0, color: Color(0x13, 0x8D, 0x9C) }],
                angle: 90.0,
            };
            assert_eq!(style.gradient.as_ref(), Some(&want));
            assert_eq!(style.stroke, Some(Stroke { color: Color(0xB5, 0x5A, 0x3C), width: 2.0 }));
        }
        other => panic!("expected the gradient ellipse, got {other:?}"),
    }
}

// ── notes_slide_xml ↔ extract_notes_text ─────────────────────────────

/// Impress's pptx exporter writes the notes into a shape with no
/// placeholder at all — one `p:sp` whose `p:nvPr` is empty. Requiring
/// `p:ph type="body"` dropped the speaker notes of every deck that had
/// been through it, and no test saw it because our own writer always
/// emits the body placeholder we were looking for.
#[test]
fn notes_in_a_shape_with_no_placeholder_are_read() {
    let xml = r##"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="8" name=""/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/>
<p:txBody><a:p><a:r><a:t>remember the demo</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##;
    assert_eq!(extract_notes_text(xml), "remember the demo");
}

/// But a placeholder that is *not* the body one is never notes. A slide
/// number read as speaker notes would be worse than losing them, so the
/// fallback widens only to shapes that declare no placeholder.
#[test]
fn a_slide_number_placeholder_is_never_read_as_notes() {
    let xml = r##"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum" idx="10"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>7</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##;
    assert_eq!(extract_notes_text(xml), "");
}

/// And when a body placeholder is present it still wins, so a notes
/// part carrying both does not concatenate them.
#[test]
fn the_body_placeholder_wins_over_an_unplaceheld_shape() {
    let xml = r##"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:nvPr/></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>slide furniture</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>the real notes</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##;
    assert_eq!(extract_notes_text(xml), "the real notes");
}


/// Build a minimal notesSlide part around raw txBody XML so entity and
/// break handling can be tested without going through `notes_slide_xml`.
fn notes_with_txbody(txbody: &str) -> String {
    format!(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes Placeholder"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>
<p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>
<p:txBody><a:bodyPr/><a:lstStyle/>{txbody}</p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"##
    )
}

#[test]
fn notes_round_trip_plain_and_multiline() {
    assert_eq!(extract_notes_text(&notes_slide_xml("Hello")), "Hello");
    assert_eq!(
        extract_notes_text(&notes_slide_xml("line one\nline two")),
        "line one\nline two"
    );
    assert_eq!(extract_notes_text(&notes_slide_xml("")), "");
}

#[test]
fn notes_round_trip_escapes_and_unescapes_entities() {
    let notes = "café & <b> > \"quotes\" 東京";
    assert_eq!(extract_notes_text(&notes_slide_xml(notes)), notes);
}

#[test]
fn notes_blank_lines_survive_round_trip() {
    // Blank lines are the presenter's; they used to collapse.
    assert_eq!(extract_notes_text(&notes_slide_xml("a\n\nb")), "a\n\nb");
    assert_eq!(extract_notes_text(&notes_slide_xml("a\n")), "a\n");
    // A shape of nothing but blank paragraphs holds no notes.
    assert_eq!(extract_notes_text(&notes_slide_xml("\n")), "");
}

#[test]
fn notes_extract_resolves_numeric_char_refs() {
    // quick-xml 0.41 surfaces &#NN; as a GeneralRef event;
    // resolve_general_ref must turn it back into the character.
    let xml = notes_with_txbody("<a:p><a:r><a:t>&#65;&#x42;</a:t></a:r></a:p>");
    assert_eq!(extract_notes_text(&xml), "AB");
}

#[test]
fn notes_extract_preserves_unknown_entities() {
    let xml = notes_with_txbody("<a:p><a:r><a:t>a &bogus; b</a:t></a:r></a:p>");
    assert_eq!(extract_notes_text(&xml), "a &bogus; b");
}

#[test]
fn notes_extract_soft_break_becomes_newline() {
    let xml =
        notes_with_txbody("<a:p><a:r><a:t>one</a:t></a:r><a:br/><a:r><a:t>two</a:t></a:r></a:p>");
    assert_eq!(extract_notes_text(&xml), "one\ntwo");
}

// ── parse_master_shapes ──────────────────────────────────────────────

#[test]
fn master_parse_empty_xml_has_no_background_or_shapes() {
    let (bg, shapes) = parse_master_shapes("");
    assert!(bg.is_none());
    assert!(shapes.is_empty());
}

#[test]
fn master_parse_missing_background_returns_none() {
    let xml = r##"<p:sldMaster xmlns:p="x"><p:cSld><p:spTree/></p:cSld></p:sldMaster>"##;
    let (bg, shapes) = parse_master_shapes(xml);
    assert!(bg.is_none());
    assert!(shapes.is_empty());
}

#[test]
fn master_parse_background_without_shapes() {
    let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld>
<p:bg><p:bgPr><a:solidFill><a:srgbClr val="AABBCC"/></a:solidFill></p:bgPr></p:bg>
<p:spTree/></p:cSld></p:sldMaster>"##;
    let (bg, shapes) = parse_master_shapes(xml);
    assert_eq!(bg.as_deref(), Some("#aabbcc"));
    assert!(shapes.is_empty());
}

#[test]
fn master_parse_text_box_shape() {
    let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
<p:sp><p:spPr><a:xfrm><a:off x="19050" y="28575"/><a:ext cx="190500" cy="95250"/></a:xfrm></p:spPr>
<p:txBody><a:p><a:r><a:t>Deck title</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sldMaster>"##;
    let (_, shapes) = parse_master_shapes(xml);
    assert_eq!(shapes.len(), 1);
    match &shapes[0] {
        SlideObject::TextBox { text, x, y, w, h, runs, .. } => {
            assert_eq!(text, "Deck title");
            // The decoration's styling is read, not discarded: this
            // asserted `runs.is_empty()` while the master walker parsed
            // text as plain, which is what let a styled master
            // decoration lose its emphasis on every open.
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].text, "Deck title");
            assert!((x - 2.0).abs() < 0.01 && (y - 3.0).abs() < 0.01);
            assert!((w - 20.0).abs() < 0.01 && (h - 10.0).abs() < 0.01);
        }
        other => panic!("expected text box, got {other:?}"),
    }
}

/// A shape with one geometry, so each case differs only in its text.
fn master_with(txbody: &str) -> Vec<SlideObject> {
    let xml = format!(
        r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
<p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="190500" cy="95250"/></a:xfrm></p:spPr>
<p:txBody>{txbody}</p:txBody></p:sp>
</p:spTree></p:cSld></p:sldMaster>"##
    );
    parse_master_shapes(&xml).1
}

fn text_and_runs(shapes: &[SlideObject]) -> (String, Vec<Run>) {
    match shapes.first() {
        Some(SlideObject::TextBox { text, runs, .. }) => (text.clone(), runs.clone()),
        other => panic!("expected a text box, got {other:?}"),
    }
}

/// Two runs inside ONE paragraph are one line.
///
/// This is the case that was wrong: the walker recorded one entry per
/// `a:t` and joined them with `\n`, so a decoration reading
/// "Plain **Bold**" came back as two lines — and, because masters are
/// written back from this text, the break was then saved into the file.
#[test]
fn master_runs_in_one_paragraph_do_not_become_separate_lines() {
    let (text, runs) = text_and_runs(&master_with(
        "<a:p><a:r><a:t>Plain </a:t></a:r>\
         <a:r><a:rPr b=\"1\"/><a:t>Bold</a:t></a:r></a:p>",
    ));
    assert_eq!(text, "Plain Bold", "runs of one paragraph are one line");
    assert_eq!(runs.len(), 2);
    assert!(!runs[0].style.bold);
    assert!(runs[1].style.bold, "the master's own emphasis is read");
}

/// Separate paragraphs ARE separate lines — the half that was already
/// right, and which a fix for the case above could easily break.
#[test]
fn master_separate_paragraphs_stay_separate_lines() {
    let (text, runs) = text_and_runs(&master_with(
        "<a:p><a:r><a:t>Line one</a:t></a:r></a:p>\
         <a:p><a:r><a:t>Line two</a:t></a:r></a:p>",
    ));
    assert_eq!(text, "Line one\nLine two");
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        text,
        "concatenated runs must equal `text` (SlideObject::TextBox's invariant)"
    );
}

/// `a:t` content is significant, so a run's trailing space survives.
/// Trimming it welds the words together, which the newline above was
/// masking: "Plain " + "Bold" read back as "PlainBold".
#[test]
fn master_keeps_the_space_a_run_ends_on() {
    let (text, _) = text_and_runs(&master_with(
        "<a:p><a:r><a:t>Plain </a:t></a:r><a:r><a:t>Bold</a:t></a:r></a:p>",
    ));
    assert_eq!(text, "Plain Bold");
}

/// An empty paragraph *between* two others is a blank line and has to
/// survive. Only a foreign file produces one — our own writer puts a
/// whole multi-line box in a single `a:t` — so this is import-only, and
/// counting the closes rather than flagging them is what keeps it.
#[test]
fn master_keeps_a_blank_line_between_two_paragraphs() {
    let (text, runs) = text_and_runs(&master_with(
        "<a:p><a:r><a:t>a</a:t></a:r></a:p><a:p/>\
         <a:p><a:r><a:t>b</a:t></a:r></a:p>",
    ));
    assert_eq!(text, "a\n\nb", "the blank line was swallowed");
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        text
    );
}

/// A trailing empty paragraph must not leave a dangling break, which is
/// why the newline is applied when the next run arrives rather than
/// when the paragraph closes.
#[test]
fn master_trailing_empty_paragraph_adds_no_line() {
    let (text, _) = text_and_runs(&master_with(
        "<a:p><a:r><a:t>Only line</a:t></a:r></a:p><a:p/>",
    ));
    assert_eq!(text, "Only line");
}


/// A master's or layout's text box keeps its paragraphs' look and its
/// body's, as a slide's does: a right-aligned classification label
/// ("PROTECTED B") comes back right-aligned, under a placeholder ahead of
/// it in the part, each paragraph its own, where it was drawn from its
/// box's left edge (future-delivery-structure: clear of the white patch
/// its slides put over it).
#[test]
fn a_masters_text_box_keeps_its_paragraphs_alignment() {
    use crate::engine::ParaAlign;
    let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
        <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
          <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="95250" cy="95250"/></a:xfrm></p:spPr>
          <p:txBody><a:p><a:r><a:t>Title</a:t></a:r></a:p></p:txBody></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr id="10" name="TextBox 9"/><p:cNvSpPr txBox="1"/><p:nvPr userDrawn="1"/></p:nvSpPr>
          <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="190500" cy="95250"/></a:xfrm><a:prstGeom prst="rect"/><a:noFill/></p:spPr>
          <p:txBody><a:bodyPr wrap="square" anchor="t"/><a:lstStyle/>
          <a:p><a:pPr algn="r"/><a:r><a:rPr sz="1200"/><a:t>PROTECTED B</a:t></a:r></a:p>
          <a:p><a:pPr algn="ctr"/><a:r><a:t>second</a:t></a:r></a:p></p:txBody></p:sp>
        </p:spTree></p:cSld></p:sldMaster>"##;
    let (_, shapes) = parse_master_shapes(xml);
    match &shapes[..] {
        [SlideObject::TextBox { text, body, .. }] => {
            assert_eq!(text, "PROTECTED B\nsecond");
            let aligns: Vec<ParaAlign> = body.paras.iter().map(|p| p.align).collect();
            assert_eq!(aligns, [ParaAlign::Right, ParaAlign::Center]);
        }
        other => panic!("expected the label alone, got {other:?}"),
    }
}

/// A hidden shape on a master (`p:cNvPr hidden="1"`: think-cell's data
/// store) is not drawn, as PowerPoint and Impress do not draw it; the
/// shapes around it are.
#[test]
fn a_masters_hidden_shape_is_not_drawn() {
    let xml = r##"<p:sldMaster xmlns:p="x" xmlns:a="y"><p:cSld><p:spTree>
        <p:sp><p:nvSpPr><p:cNvPr id="12" name="think-cell data - do not delete" hidden="1"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
          <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="95250" cy="95250"/></a:xfrm><a:prstGeom prst="rect"/>
          <a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></p:spPr></p:sp>
        <p:sp><p:nvSpPr><p:cNvPr id="13" name="Bar"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
          <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="190500" cy="95250"/></a:xfrm><a:prstGeom prst="rect"/>
          <a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></p:spPr></p:sp>
        </p:spTree></p:cSld></p:sldMaster>"##;
    let (_, shapes) = parse_master_shapes(xml);
    assert_eq!(shapes.len(), 1, "{shapes:?}");
    assert!(matches!(&shapes[0], SlideObject::Shape { w, .. } if (w - 20.0).abs() < 0.01), "{shapes:?}");
}
