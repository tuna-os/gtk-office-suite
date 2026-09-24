// DOCX round-trip tests — the beginning of the Word-fidelity ratchet.
// Every assertion here is a feature a Word-like processor must not lose
// on save/reload. Cases are modeled on LibreOffice Writer's ooxmlexport
// test taxonomy (assertions rewritten against our model; no LO data files).

use letters_core::docx;
use letters_core::model::*;
use letters_core::StructuredEditor;

fn round_trip(doc: &Document) -> Document {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.docx");
    let path = path.to_str().unwrap();
    docx::write(doc, path).expect("write docx");
    docx::read(path).expect("read docx")
}

#[test]
fn plain_paragraphs_survive() {
    let d = Document::from_plain_text("first paragraph\nsecond paragraph");
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "first paragraph\nsecond paragraph");
}

#[test]
fn bold_run_survives() {
    let mut d = Document::from_plain_text("normal bold normal");
    d.apply_run_style(7, 11, &StylePatch::set_bold(true));
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "normal bold normal");
    assert!(rt.style_at(7).bold, "bold lost in round trip");
    assert!(!rt.style_at(0).bold);
    assert!(!rt.style_at(12).bold);
}

#[test]
fn italic_underline_strike_survive() {
    let mut d = Document::from_plain_text("abc def ghi");
    d.apply_run_style(0, 3, &StylePatch::set_italic(true));
    d.apply_run_style(4, 7, &StylePatch::set_underline(true));
    d.apply_run_style(8, 11, &StylePatch::set_strikethrough(true));
    let rt = round_trip(&d);
    assert!(rt.style_at(0).italic);
    assert!(rt.style_at(4).underline);
    assert!(rt.style_at(8).strikethrough);
}

#[test]
fn combined_styles_survive() {
    let mut d = Document::from_plain_text("all");
    d.apply_run_style(0, 3, &StylePatch::set_bold(true));
    d.apply_run_style(0, 3, &StylePatch::set_italic(true));
    d.apply_run_style(0, 3, &StylePatch::set_underline(true));
    let rt = round_trip(&d);
    let s = rt.style_at(1);
    assert!(s.bold && s.italic && s.underline);
}

#[test]
fn heading_levels_survive() {
    let mut d = Document::from_plain_text("Title\nSection\nbody");
    d.set_heading(0, Some(1));
    d.set_heading(1, Some(2));
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.heading, Some(1));
    assert_eq!(rt.paragraphs[1].style.heading, Some(2));
    assert_eq!(rt.paragraphs[2].style.heading, None);
}

#[test]
fn empty_paragraph_between_text_survives() {
    let d = Document::from_plain_text("above\n\nbelow");
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "above\n\nbelow");
    assert_eq!(rt.paragraphs.len(), 3);
}

#[test]
fn unicode_text_survives() {
    let d = Document::from_plain_text("héllo wörld — “quotes” 中文");
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "héllo wörld — “quotes” 中文");
}

#[test]
fn alignment_survives() {
    let mut d = Document::from_plain_text("centered\nrighted\njustified\nplain");
    d.paragraphs[0].style.alignment = Alignment::Center;
    d.paragraphs[1].style.alignment = Alignment::Right;
    d.paragraphs[2].style.alignment = Alignment::Justify;
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.alignment, Alignment::Center);
    assert_eq!(rt.paragraphs[1].style.alignment, Alignment::Right);
    assert_eq!(rt.paragraphs[2].style.alignment, Alignment::Justify);
    assert_eq!(rt.paragraphs[3].style.alignment, Alignment::Left);
}

// ── RED: known fidelity gaps, kept visible until fixed ─────────────────
// Run with `cargo test -- --ignored` to see the guiding set.

#[test]
fn list_kind_survives() {
    let mut d = Document::from_plain_text("first item\nsecond item");
    d.paragraphs[0].style.list = ListKind::Bullet;
    d.paragraphs[1].style.list = ListKind::Numbered;
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.list, ListKind::Bullet);
    assert_eq!(rt.paragraphs[1].style.list, ListKind::Numbered);
}

#[test]
fn highlight_survives() {
    let mut d = Document::from_plain_text("glow");
    d.apply_run_style(0, 4, &StylePatch::set_highlight(true));
    let rt = round_trip(&d);
    assert!(rt.style_at(0).highlight);
}

#[test]
fn link_survives() {
    let mut d = Document::from_plain_text("click here");
    d.apply_run_style(6, 10, &StylePatch::set_link(Some("https://gnome.org".into())));
    let rt = round_trip(&d);
    assert_eq!(rt.style_at(6).link.as_deref(), Some("https://gnome.org"));
}

#[test]
fn style_boundaries_are_exact() {
    // Word processors classically off-by-one run boundaries.
    let mut d = Document::from_plain_text("aaabbbccc");
    d.apply_run_style(3, 6, &StylePatch::set_bold(true));
    let rt = round_trip(&d);
    assert!(!rt.style_at(2).bold);
    assert!(rt.style_at(3).bold);
    assert!(rt.style_at(5).bold);
    assert!(!rt.style_at(6).bold);
}

#[test]
fn inline_image_survives() {
    // 1x1 PNG written to a temp file, placed in a doc, round-tripped.
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
        0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x01, 0x9E, 0xDD, 0x22, 0x71, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("dot.png");
    std::fs::write(&img_path, png).unwrap();

    let mut d = Document::from_plain_text("before");
    d.paragraphs.push(Paragraph {
        style: ParaStyle::default(),
        runs: vec![Run {
            text: "a red dot".into(),
            style: RunStyle {
                image: Some(img_path.to_string_lossy().into_owned()),
                // 2in x 1.2in: the size it is shown at, not its 1x1 pixels.
                image_extent_emu: Some((1_828_800, 1_097_280)),
                ..Default::default()
            },
        }],
    });
    let rt = round_trip(&d);

    let img_run = rt.paragraphs.iter().flat_map(|p| p.runs.iter())
        .find(|r| r.style.image.is_some())
        .expect("image run lost in round trip");
    let extracted = img_run.style.image.as_ref().unwrap();
    let bytes = std::fs::read(extracted).expect("extracted image unreadable");
    assert_eq!(bytes, png, "image bytes changed in round trip");
    assert_eq!(img_run.style.image_extent_emu, Some((1_828_800, 1_097_280)), "displayed size lost");
    assert!(rt.to_plain_text().contains("before"));
}

#[test]
fn font_size_color_vertalign_survive() {
    let mut d = Document::from_plain_text("big red super");
    d.paragraphs[0].runs = vec![
        Run { text: "big ".into(), style: RunStyle { font_size_hp: Some(48), ..Default::default() } },
        Run { text: "red ".into(), style: RunStyle { color: Some("FF0000".into()), ..Default::default() } },
        Run { text: "super".into(), style: RunStyle { vert_align: Some(VertAlign::Superscript), ..Default::default() } },
    ];
    let rt = round_trip(&d);
    let runs = &rt.paragraphs[0].runs;
    let big = runs.iter().find(|r| r.text.contains("big")).expect("big run");
    assert_eq!(big.style.font_size_hp, Some(48), "font size lost");
    let red = runs.iter().find(|r| r.text.contains("red")).expect("red run");
    assert_eq!(red.style.color.as_deref(), Some("FF0000"), "color lost");
    let sup = runs.iter().find(|r| r.text.contains("super")).expect("super run");
    assert_eq!(sup.style.vert_align, Some(VertAlign::Superscript), "vert align lost");
}

#[test]
fn block_quote_survives() {
    let mut d = Document::from_plain_text("wise words\nplain after");
    d.paragraphs[0].style.block_quote = true;
    let rt = round_trip(&d);
    assert!(rt.paragraphs[0].style.block_quote, "block quote lost");
    assert!(!rt.paragraphs[1].style.block_quote);
}

#[test]
fn table_structure_survives() {
    use letters_core::model::TableCell;
    let mut d = Document::from_plain_text("intro");
    for (r, c, txt) in [(0u32, 0u32, "a1"), (0, 1, "b1"), (1, 0, "a2"), (1, 1, "b2")] {
        d.paragraphs.push(Paragraph {
            style: ParaStyle {
                table_cell: Some(TableCell { table: 0, row: r, col: c }),
                ..Default::default()
            },
            runs: vec![Run::plain(txt)],
        });
    }
    let rt = round_trip(&d);
    let cells: Vec<(u32, u32, String)> = rt.paragraphs.iter()
        .filter_map(|p| p.style.table_cell.map(|tc| (tc.row, tc.col, p.text())))
        .collect();
    assert_eq!(cells.len(), 4, "cell count changed: {cells:?}");
    assert!(cells.contains(&(0, 0, "a1".into())), "{cells:?}");
    assert!(cells.contains(&(1, 1, "b2".into())), "{cells:?}");
    assert!(rt.to_plain_text().contains("intro"));
}

#[test]
fn a_table_inserted_in_the_editor_survives_docx() {
    // Closes the chain the editor actually walks: Insert Table -> model ->
    // DOCX -> model. The cells' own round trip is covered above; what this
    // adds is that what the *command* produces is a table a reader can
    // still see. (rdocx exposes tables separately from the paragraph
    // stream, so a reopened table lands after the body text — position is
    // deliberately not asserted here.)
    let mut editor = StructuredEditor::new(Document::from_plain_text("intro"));
    editor.set_cursor(5);
    let table = editor.insert_table(2, 2);
    let doc = editor.document();
    let first = doc.paragraphs.iter()
        .position(|p| p.style.table_cell.is_some()).expect("cells exist");
    let mut doc = doc.clone();
    for (offset, text) in ["a", "b", "c", "d"].iter().enumerate() {
        doc.paragraphs[first + offset].runs = vec![Run::plain(*text)];
    }
    assert_eq!(doc.table_dimensions(table), Some((2, 2)));

    let rt = round_trip(&doc);
    let cells: Vec<(u32, u32, String)> = rt.paragraphs.iter()
        .filter_map(|p| p.style.table_cell.map(|tc| (tc.row, tc.col, p.text())))
        .collect();
    assert_eq!(cells, vec![
        (0, 0, "a".into()), (0, 1, "b".into()),
        (1, 0, "c".into()), (1, 1, "d".into()),
    ]);
    assert!(rt.paragraphs.iter().any(|p| p.text() == "intro" && p.style.table_cell.is_none()));
}

#[test]
fn styled_table_cell_survives() {
    use letters_core::model::TableCell;
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].style.table_cell = Some(TableCell { table: 0, row: 0, col: 0 });
    d.paragraphs[0].runs = vec![Run {
        text: "bolded cell".into(),
        style: RunStyle { bold: true, ..Default::default() },
    }];
    let rt = round_trip(&d);
    let cell = rt.paragraphs.iter()
        .find(|p| p.style.table_cell.is_some() && p.text().contains("bolded"))
        .expect("cell lost");
    assert!(cell.runs.iter().any(|r| r.style.bold), "cell bold lost");
}

#[test]
fn page_breaks_and_named_styles_survive() {
    let mut d = Document::from_plain_text("My Title\nsubtitle here\nchapter two starts");
    d.paragraphs[0].style.named_style = Some("Title".into());
    d.paragraphs[1].style.named_style = Some("Subtitle".into());
    d.paragraphs[2].style.page_break_before = true;
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.named_style.as_deref(), Some("Title"));
    assert_eq!(rt.paragraphs[1].style.named_style.as_deref(), Some("Subtitle"));
    assert!(rt.paragraphs[2].style.page_break_before, "page break lost");
    assert!(!rt.paragraphs[0].style.page_break_before);
}

#[test]
fn header_footer_survive() {
    let mut d = Document::from_plain_text("body text");
    d.header = Some("Quarterly Report".into());
    d.footer = Some("Page {page}".into());
    let rt = round_trip(&d);
    assert_eq!(rt.header.as_deref(), Some("Quarterly Report"));
    assert_eq!(rt.footer.as_deref(), Some("Page {page}"));
}

// ── Page geometry & font family round-trip (PARITY stragglers) ───────

#[test]
fn docx_page_geometry_round_trips() {
    use letters_core::model::PageGeometry;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("geom.docx");
    let mut d = Document::from_plain_text("body");
    d.page = Some(PageGeometry {
        width_pt: 612.0,
        height_pt: 792.0,
        margin_top_pt: 36.0,
        margin_bottom_pt: 54.0,
        margin_left_pt: 90.0,
        margin_right_pt: 45.0,
        columns: 1,
        column_gap_pt: 18.0,
    });
    docx::write(&d, path.to_str().unwrap()).expect("write");
    let rt = docx::read(path.to_str().unwrap()).expect("read");
    let pg = rt.page.expect("page geometry lost");
    assert!(pg.approx_eq(&d.page.unwrap()), "geometry drifted: {pg:?}");
}

#[test]
fn docx_font_family_round_trips() {
    use letters_core::model::{Run, RunStyle};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("font.docx");
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![
        Run::plain("sans "),
        Run {
            text: "serif".into(),
            style: RunStyle { font_family: Some("Liberation Serif".into()), ..Default::default() },
        },
    ];
    docx::write(&d, path.to_str().unwrap()).expect("write");
    let rt = docx::read(path.to_str().unwrap()).expect("read");
    assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
}

#[test]
fn docx_line_spacing_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spacing.docx");
    let mut d = Document::from_plain_text("single\ndouble");
    d.paragraphs[1].style.line_spacing = 2.0;
    docx::write(&d, path.to_str().unwrap()).expect("write");
    let rt = docx::read(path.to_str().unwrap()).expect("read");
    assert!((rt.paragraphs[0].style.line_spacing - 1.0).abs() < 0.01);
    assert!(
        (rt.paragraphs[1].style.line_spacing - 2.0).abs() < 0.01,
        "line spacing lost: {}",
        rt.paragraphs[1].style.line_spacing
    );
}

// ── Footnotes (ADR 0003 §2) ──────────────────────────────────────────

#[test]
fn footnote_round_trips_through_docx() {
    use letters_core::model::{Run, RunStyle};
    let mut doc = Document::new();
    doc.paragraphs[0].runs = vec![
        Run { text: "Body text".into(), style: RunStyle::default() },
        Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } },
        Run { text: " continues.".into(), style: RunStyle::default() },
    ];
    doc.footnotes = vec!["The footnote content.".into()];

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fn.docx");
    letters_core::docx::write(&doc, path.to_str().unwrap()).unwrap();
    let rt = letters_core::docx::read(path.to_str().unwrap()).unwrap();

    assert_eq!(rt.footnotes, vec!["The footnote content.".to_string()]);
    let has_ref = rt.paragraphs[0]
        .runs
        .iter()
        .any(|r| r.style.footnote == Some(0));
    assert!(has_ref, "footnote reference lost: {:?}", rt.paragraphs[0].runs);
}

/// Paragraph indents and spacing survive a docx save.
///
/// The docx writer emitted neither, while the odt writer had carried both
/// since it was written — so the same document kept its layout as .odt and
/// lost it as .docx, silently. rdocx had the builders the whole time; they
/// were simply never called.
#[test]
fn indents_and_spacing_survive_a_docx_round_trip() {
    let mut doc = Document::new();
    doc.paragraphs[0] = Paragraph {
        style: ParaStyle {
            left_indent_pt: 36.0,
            right_indent_pt: 18.0,
            first_line_indent_pt: 24.0,
            space_before_pt: 12.0,
            space_after_pt: 18.0,
            ..Default::default()
        },
        runs: vec![Run::plain("indented and spaced")],
    };

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layout.docx");
    letters_core::docx::write(&doc, path.to_str().unwrap()).unwrap();
    let rt = letters_core::docx::read(path.to_str().unwrap()).unwrap();
    let s = &rt.paragraphs[0].style;

    assert_eq!(s.left_indent_pt, 36.0, "left indent");
    assert_eq!(s.right_indent_pt, 18.0, "right indent");
    assert_eq!(s.first_line_indent_pt, 24.0, "first-line indent");
    assert_eq!(s.space_before_pt, 12.0, "space before");
    assert_eq!(s.space_after_pt, 18.0, "space after");
}

/// A paragraph that asks for no indent writes no `w:ind` at all.
///
/// OOXML reads an absent `w:ind` as "inherit from the style" and an
/// explicit zero as "override the style with nothing", so writing zeros
/// unconditionally would flatten the indent of every styled paragraph.
#[test]
fn an_unindented_paragraph_writes_no_indent_at_all() {
    let doc = Document::from_plain_text("plain");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plain.docx");
    letters_core::docx::write(&doc, path.to_str().unwrap()).unwrap();

    let f = std::fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(f).unwrap();
    let mut xml = String::new();
    {
        use std::io::Read;
        zip.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    }
    assert!(!xml.contains("<w:ind"), "an explicit w:ind was written for an unindented paragraph");
    assert!(
        !xml.contains("<w:spacing w:before=\"0\""),
        "an explicit zero spacing was written"
    );
}

/// Strict-OOXML indents, which name the axis rather than the side.
///
/// ISO/IEC 29500 strict spells the horizontal indents `w:start`/`w:end`;
/// the transitional schema spells them `w:left`/`w:right`. Both are real
/// .docx — LibreOffice's "Office Open XML Text" filter writes the strict
/// pair — and the reader used to see only the transitional one, so every
/// indent in a strict file read as zero. This rewrites our own output
/// into the strict spelling so the guard does not need LibreOffice; the
/// oracle covers the same ground against the real exporter.
#[test]
fn strict_ooxml_indents_are_read() {
    let mut d = Document::from_plain_text("");
    d.paragraphs[0] = Paragraph {
        style: ParaStyle {
            left_indent_pt: 36.0,
            right_indent_pt: 24.0,
            ..Default::default()
        },
        runs: vec![Run::plain("indented")],
    };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("transitional.docx");
    docx::write(&d, &src).expect("write docx");

    // Re-spell the indents and repack.
    let strict = dir.path().join("strict.docx");
    {
        let mut zin = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
        let mut zout = zip::ZipWriter::new(std::fs::File::create(&strict).unwrap());
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
            if name == "word/document.xml" {
                let xml = String::from_utf8(buf).unwrap();
                assert!(xml.contains("w:left="), "fixture needs a transitional indent: {xml}");
                buf = xml.replace("w:left=", "w:start=").replace("w:right=", "w:end=").into_bytes();
            }
            zout.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zout, &buf).unwrap();
        }
        zout.finish().unwrap();
    }

    let rt = docx::read(strict.to_str().unwrap()).expect("read strict docx");
    let s = &rt.paragraphs[0].style;
    assert!((s.left_indent_pt - 36.0).abs() < 0.01, "strict left indent: {}", s.left_indent_pt);
    assert!((s.right_indent_pt - 24.0).abs() < 0.01, "strict right indent: {}", s.right_indent_pt);
}

/// Multi-column sections, which the writer never emitted.
///
/// `PageGeometry::columns` was carried by the odt writer and dropped by
/// this one: `w:cols` appeared nowhere, so a two-column document saved
/// as `.docx` came back as one column with no warning. rdocx has exposed
/// `set_columns` and `section_properties().columns` the whole time.
#[test]
fn column_count_and_gap_survive() {
    let mut d = Document::from_plain_text("two columns");
    d.page = Some(PageGeometry { columns: 2, column_gap_pt: 24.0, ..Default::default() });
    let rt = round_trip(&d);
    let pg = rt.page.expect("page geometry");
    assert_eq!(pg.columns, 2, "column count lost");
    assert!((pg.column_gap_pt - 24.0).abs() < 0.1, "column gap: {}", pg.column_gap_pt);
}

/// A single-column section writes no `w:cols` at all.
///
/// One column is the default section layout, so an explicit `w:num="1"`
/// is noise in every ordinary document. The count still reads back as 1.
#[test]
fn single_column_writes_no_cols_element() {
    let mut d = Document::from_plain_text("one column");
    d.page = Some(PageGeometry::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.docx");
    docx::write(&d, &path).expect("write docx");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml).unwrap();
    assert!(!xml.contains("<w:cols"), "wrote a redundant single-column w:cols");
    assert_eq!(docx::read(path.to_str().unwrap()).unwrap().page.unwrap().columns, 1);
}

/// Tab stops, which neither format persisted.
///
/// `ParaStyle::tab_stops_pt` is settable from the paragraph layout UI and
/// was dropped by both writers on every save, self round trip included.
/// rdocx writes stops through `add_tab_stop` but exposes only
/// `tab_stop_count()` when reading, so the positions come from the same
/// scan of `word/document.xml` that reads the strict indents.
#[test]
fn tab_stops_survive() {
    let mut d = Document::from_plain_text("tabbed");
    d.paragraphs[0].style.tab_stops_pt = vec![36.0, 108.0, 180.0];
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.tab_stops_pt.len(), 3, "stops lost");
    for (got, want) in rt.paragraphs[0].style.tab_stops_pt.iter().zip([36.0, 108.0, 180.0]) {
        assert!((got - want).abs() < 0.1, "stop {got} != {want}");
    }
}

/// A tab *character* is not a tab stop.
///
/// OOXML spells both `w:tab`: a stop inside `w:tabs` in the paragraph
/// properties, and a literal tab inside a run. Only the first has a
/// position, so a paragraph containing a tab in its text must come back
/// with no stops rather than an invented one.
#[test]
fn a_tab_character_does_not_become_a_tab_stop() {
    let d = Document::from_plain_text("before\tafter");
    let rt = round_trip(&d);
    assert!(
        rt.paragraphs[0].style.tab_stops_pt.is_empty(),
        "a tab in the text invented stops: {:?}",
        rt.paragraphs[0].style.tab_stops_pt
    );
    assert!(
        rt.paragraphs[0].runs.iter().map(|r| r.text.as_str()).collect::<String>().contains('\t'),
        "the tab character itself was lost"
    );
}

/// A `w:val="clear"` entry removes an inherited stop; it is not one.
///
/// LibreOffice writes `<w:tab w:val="clear" w:pos="1134"/>` to drop the
/// 2cm default before listing the real stops, and `bar` and `num` are a
/// vertical rule and a list's numbering gap. Collecting every
/// `w:tab@w:pos` turns each of those into a stop the document never had.
/// This rewrites our own output so the guard needs no LibreOffice.
#[test]
fn a_cleared_or_non_stop_tab_entry_is_not_read_as_a_stop() {
    let mut d = Document::from_plain_text("tabbed");
    d.paragraphs[0].style.tab_stops_pt = vec![36.0];
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("plain.docx");
    docx::write(&d, &src).expect("write docx");

    let doctored = dir.path().join("doctored.docx");
    {
        let mut zin = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
        let mut zout = zip::ZipWriter::new(std::fs::File::create(&doctored).unwrap());
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
            if name == "word/document.xml" {
                let xml = String::from_utf8(buf).unwrap();
                assert!(xml.contains("<w:tabs>"), "fixture needs a w:tabs: {xml}");
                buf = xml
                    .replace(
                        "<w:tabs>",
                        "<w:tabs><w:tab w:val=\"clear\" w:pos=\"1134\"/>\
                         <w:tab w:val=\"bar\" w:pos=\"2000\"/>\
                         <w:tab w:val=\"num\" w:pos=\"3000\"/>",
                    )
                    .into_bytes();
            }
            zout.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zout, &buf).unwrap();
        }
        zout.finish().unwrap();
    }

    let rt = docx::read(doctored.to_str().unwrap()).expect("read doctored docx");
    let stops = &rt.paragraphs[0].style.tab_stops_pt;
    assert_eq!(stops.len(), 1, "non-stop entries were read as stops: {stops:?}");
    assert!((stops[0] - 36.0).abs() < 0.1, "the real stop was lost: {stops:?}");
}

/// Collapse whitespace between XML elements (`>   <` becomes `><`).
fn regex_lite_collapse(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut chars = xml.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '>' {
            let mut ws = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_whitespace() {
                    ws.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            // Keep the whitespace unless the next thing is another tag.
            if chars.peek() != Some(&'<') {
                out.push_str(&ws);
            }
        }
    }
    out
}

/// Insert raw XML into our own docx's body, for the break placements
/// LibreOffice and Word produce that our writer never emits.
fn doctor_document_xml(d: &Document, edit: impl Fn(String) -> String) -> Document {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.docx");
    docx::write(d, &src).expect("write docx");
    let out = dir.path().join("out.docx");
    {
        let mut zin = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
        let mut zout = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
            if name == "word/document.xml" {
                // Our writer pretty-prints, so collapse the whitespace
                // between elements first: it is insignificant here, and
                // it keeps these fixtures from depending on the writer's
                // indentation.
                let xml = String::from_utf8(buf).unwrap();
                let collapsed = regex_lite_collapse(&xml);
                buf = edit(collapsed).into_bytes();
            }
            zout.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zout, &buf).unwrap();
        }
        zout.finish().unwrap();
    }
    docx::read(out.to_str().unwrap()).expect("read doctored docx")
}

/// A run-level break after a paragraph's text belongs to the next one.
///
/// This is what LibreOffice writes when it converts an ODF
/// `fo:break-before="page"`: the break is the last run of the paragraph
/// *before* it. Reading it as this paragraph's own break puts it a page
/// too early.
#[test]
fn a_trailing_run_page_break_marks_the_next_paragraph() {
    let d = Document::from_plain_text("first page\nsecond page");
    let rt = doctor_document_xml(&d, |xml| {
        let marker = "<w:t>first page</w:t></w:r>";
        assert!(xml.contains(marker), "fixture shape changed: {xml}");
        xml.replace(marker, "<w:t>first page</w:t></w:r><w:r><w:br w:type=\"page\"/></w:r>")
    });
    assert!(!rt.paragraphs[0].style.page_break_before, "break landed a page early");
    assert!(rt.paragraphs[1].style.page_break_before, "break was dropped");
    assert_eq!(
        rt.paragraphs[0].runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "first page",
        "the break leaked into the text"
    );
}

/// A paragraph holding only a page break is the break, not an empty line.
///
/// python-docx's `add_paragraph().add_run().add_break(WD_BREAK.PAGE)`
/// writes this; LibreOffice starts the next paragraph at the top of the
/// new page (render-lab `letters/page-break`), where an empty paragraph
/// kept in the model put a blank line first.
#[test]
fn a_paragraph_that_is_only_a_page_break_breaks_before_the_next() {
    let d = Document::from_plain_text("first page\nsecond page");
    let rt = doctor_document_xml(&d, |xml| {
        let marker = "<w:p><w:r><w:t>second page</w:t>";
        let marker = if xml.contains(marker) { marker.to_string() } else {
            // Our writer may give the paragraph properties first.
            let at = xml.find("<w:t>second page</w:t>").expect("fixture shape changed");
            let p = xml[..at].rfind("<w:p>").or_else(|| xml[..at].rfind("<w:p ")).unwrap();
            xml[p..at].to_string()
        };
        xml.replacen(&marker, &format!("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>{marker}"), 1)
    });
    let texts: Vec<String> = rt.paragraphs.iter().map(|p| p.text()).collect();
    assert_eq!(texts, vec!["first page", "second page"], "no empty paragraph for the break");
    assert!(rt.paragraphs[1].style.page_break_before, "the break was lost");
    assert!(!rt.paragraphs[0].style.page_break_before);
}

/// A break before a paragraph's own text is that paragraph's break.
///
/// Word writes this when the break is inserted at the start of a line.
#[test]
fn a_leading_run_page_break_marks_its_own_paragraph() {
    let d = Document::from_plain_text("first page\nsecond page");
    let rt = doctor_document_xml(&d, |xml| {
        let marker = "<w:r><w:t>second page</w:t>";
        assert!(xml.contains(marker), "fixture shape changed: {xml}");
        xml.replace(marker, &format!("<w:r><w:br w:type=\"page\"/></w:r>{marker}"))
    });
    assert!(rt.paragraphs[1].style.page_break_before, "break was dropped");
    assert!(!rt.paragraphs[0].style.page_break_before, "break landed on the wrong paragraph");
}

/// A break with text on both sides splits one paragraph across pages.
///
/// The model has no way to say that, so it must not be reported as a
/// paragraph-level break on either paragraph — a guess in either
/// direction moves text to the wrong page.
#[test]
fn a_mid_paragraph_run_page_break_is_not_a_paragraph_break() {
    let d = Document::from_plain_text("first page\nsecond page");
    let rt = doctor_document_xml(&d, |xml| {
        let marker = "<w:t>first page</w:t></w:r>";
        xml.replace(
            marker,
            "<w:t>first page</w:t></w:r><w:r><w:br w:type=\"page\"/></w:r>\
             <w:r><w:t>still the same paragraph</w:t></w:r>",
        )
    });
    assert!(!rt.paragraphs[0].style.page_break_before, "invented a break on the paragraph");
    assert!(
        !rt.paragraphs[1].style.page_break_before,
        "a mid-paragraph break became the next paragraph's"
    );
}

/// Rewrite the XML parts of our own docx, for producer shapes our writer
/// never emits. `edit` sees every `.xml` part at once (name → XML, with the
/// whitespace between elements collapsed) and may change any of them.
fn doctor_parts(d: &Document, edit: impl FnOnce(&mut std::collections::BTreeMap<String, String>)) -> Document {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.docx");
    docx::write(d, &src).expect("write docx");
    let mut zin = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
        parts.push((f.name().to_string(), buf));
    }
    let mut xml: std::collections::BTreeMap<String, String> = parts
        .iter()
        .filter(|(n, _)| n.ends_with(".xml"))
        .map(|(n, b)| (n.clone(), regex_lite_collapse(std::str::from_utf8(b).unwrap())))
        .collect();
    edit(&mut xml);
    let out = dir.path().join("out.docx");
    let mut zout = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for (name, bytes) in parts {
        let bytes = xml.get(&name).map(|s| s.clone().into_bytes()).unwrap_or(bytes);
        zout.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zout, &bytes).unwrap();
    }
    zout.finish().unwrap();
    docx::read(out.to_str().unwrap()).expect("read doctored docx")
}

/// Word's built-in list styles carry the numbering on the style.
///
/// python-docx's `style="List Bullet"`, Word's style gallery and many
/// templates write `<w:pStyle w:val="ListBullet"/>` and no `w:numPr` on the
/// paragraph. Those items used to read as plain paragraphs: no bullet, no
/// indent (render-lab `letters/bullet-list`, `nested-list`, `numbered-list`).
#[test]
fn list_numbering_inherited_from_a_paragraph_style_is_read() {
    let mut d = Document::from_plain_text("one\ntwo\nthree\nfirst\nplain");
    for p in &mut d.paragraphs[..3] {
        p.style.list = ListKind::Bullet;
    }
    d.paragraphs[3].style.list = ListKind::Numbered;
    let rt = doctor_parts(&d, |parts| {
        // Move each paragraph's numbering onto a named list style, keeping
        // the numId our writer allocated so bullet-vs-number still resolves.
        let body = parts.get_mut("word/document.xml").unwrap();
        let names = [("ListBullet", "List Bullet"), ("ListBullet2", "List Bullet 2"),
            ("ListBullet3", "List Bullet 3"), ("ListNumber", "List Number")];
        let mut styles = String::new();
        for (id, name) in names {
            let start = body.find("<w:numPr>").expect("fixture shape changed");
            let end = body[start..].find("</w:numPr>").unwrap() + start + "</w:numPr>".len();
            let num_pr = body[start..end].to_string();
            let num_id = num_pr.split("<w:numId w:val=\"").nth(1).unwrap().split('"').next().unwrap();
            styles.push_str(&format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/>\
                 <w:pPr><w:numPr><w:numId w:val=\"{num_id}\"/></w:numPr></w:pPr></w:style>"
            ));
            body.replace_range(start..end, &format!("<w:pStyle w:val=\"{id}\"/>"));
        }
        assert!(!body.contains("<w:numPr>"), "fixture shape changed: {body}");
        let st = parts.get_mut("word/styles.xml").unwrap();
        *st = st.replace("</w:styles>", &format!("{styles}</w:styles>"));
    });
    let lists: Vec<(ListKind, u8, String)> =
        rt.paragraphs.iter().map(|p| (p.style.list, p.style.list_level, p.text())).collect();
    assert_eq!(
        lists,
        vec![
            (ListKind::Bullet, 0, "one".into()),
            (ListKind::Bullet, 1, "two".into()),
            (ListKind::Bullet, 2, "three".into()),
            (ListKind::Numbered, 0, "first".into()),
            (ListKind::None, 0, "plain".into()),
        ]
    );
}
