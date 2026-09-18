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
            style: RunStyle { image: Some(img_path.to_string_lossy().into_owned()), ..Default::default() },
        }],
    });
    let rt = round_trip(&d);

    let img_run = rt.paragraphs.iter().flat_map(|p| p.runs.iter())
        .find(|r| r.style.image.is_some())
        .expect("image run lost in round trip");
    let extracted = img_run.style.image.as_ref().unwrap();
    let bytes = std::fs::read(extracted).expect("extracted image unreadable");
    assert_eq!(bytes, png, "image bytes changed in round trip");
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
