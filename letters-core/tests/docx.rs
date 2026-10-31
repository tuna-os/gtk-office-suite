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

/// The Latin typefaces of a saved package's theme: (major, minor).
fn theme_fonts(path: &std::path::Path) -> (String, String) {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/theme/theme1.xml").expect("a theme part"), &mut xml).unwrap();
    let face = |scheme: &str| {
        let at = xml.find(scheme).unwrap();
        let latin = at + xml[at..].find("<a:latin typeface=\"").unwrap() + "<a:latin typeface=\"".len();
        xml[latin..latin + xml[latin..].find('"').unwrap()].to_string()
    };
    (face("<a:majorFont>"), face("<a:minorFont>"))
}

/// A new document's theme names Letters' fonts, not the Aptos of the
/// Office theme rdocx writes, and it reopens in them.
#[test]
fn the_theme_names_the_documents_fonts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.docx");
    docx::write(&Document::from_plain_text("hello"), &path).unwrap();
    assert_eq!(theme_fonts(&path), ("Liberation Serif".to_string(), "Liberation Serif".to_string()));
    let rt = docx::read(path.to_str().unwrap()).unwrap();
    assert_eq!(rt.base_font.family.as_deref(), Some("Liberation Serif"));
    assert!(rt.paragraphs[0].runs.iter().all(|r| r.style.font_family.is_none()), "text is in the default font");

    // A document's own base font is its theme's too.
    let mut d = Document::from_plain_text("hello");
    d.base_font.family = Some("DejaVu Sans".into());
    let path = dir.path().join("own.docx");
    docx::write(&d, &path).unwrap();
    assert_eq!(theme_fonts(&path).1, "DejaVu Sans");
    assert_eq!(docx::read(path.to_str().unwrap()).unwrap().base_font.family.as_deref(), Some("DejaVu Sans"));
}

/// Tracked changes reopen as they were: insertions, deletions (their text
/// kept) and a deletion of someone else's insertion, with authors and
/// dates.
#[test]
fn tracked_changes_survive() {
    let d = letters_core::track::sample_document();
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
}

/// Comments reopen as they were: overlapping anchors, a reply in its
/// thread, a resolved thread, and a thread whose text was deleted.
#[test]
fn comments_survive() {
    let mut d = letters_core::comments::sample_document();
    let (ops, _) = letters_core::comments::add(&d, 0, 3, "Ada Lovelace", "2026-09-26T11:00:00Z", "Two lines\nof comment").unwrap();
    letters_core::edit::apply_all(&mut d, &ops).unwrap();
    // Deleting "The" leaves that comment without text.
    letters_core::edit::apply_all(&mut d, &[letters_core::edit::Op::Delete { at: 0, len: 3 }]).unwrap();
    let rt = round_trip(&d);
    assert_eq!(letters_core::comments::threads(&rt), letters_core::comments::threads(&d));
    assert_eq!(rt.paragraphs, d.paragraphs);
}

/// A table of contents is Word's TOC field around its entries, and reopens
/// as the same entries (levels, indents, titles and page numbers).
#[test]
fn a_table_of_contents_survives() {
    let d = letters_core::toc::sample_document();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("toc.docx");
    docx::write(&d, &path).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap().by_name("word/document.xml").unwrap(), &mut xml).unwrap();
    assert_eq!(xml.matches("w:fldCharType=\"begin\"").count(), 1, "one TOC field");
    assert!(xml.contains("TOC \\o") && xml.contains("w:leader=\"dot\""), "{xml}");
    // A tab is Word's element: a raw one in w:t reads as a space elsewhere.
    assert!(xml.contains("Introduction</w:t><w:tab/>") && !xml.contains('\t'), "{xml}");
    let rt = docx::read(path.to_str().unwrap()).unwrap();
    let entries = |d: &Document| letters_core::toc::blocks(d).into_iter().flat_map(|b| d.paragraphs[b].to_vec()).collect::<Vec<_>>();
    assert_eq!(entries(&rt), entries(&d));
    assert_eq!(rt.paragraphs.len(), d.paragraphs.len());
}

/// Smart chips reopen as chips: a date is a Word date content control, a
/// link or person chip its hyperlink in a tagged control.
#[test]
fn smart_chips_survive() {
    let d = letters_core::chips::sample_document();
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
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

/// A floating picture (the Ofsted logo of the audit-committee minutes,
/// right-aligned in the margins and raised above its paragraph) is saved
/// in its own paragraph, as a `wp:anchor`, and reads back placed the same.
#[test]
fn a_floating_image_keeps_its_paragraph_and_placement() {
    use letters_core::model::{AnchorAlign, AnchorFrame, ImageAnchor};
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
        0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x01, 0x9E, 0xDD, 0x22, 0x71, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("logo.png");
    std::fs::write(&img_path, png).unwrap();
    let anchor = ImageAnchor {
        h_from: AnchorFrame::Margin,
        h_align: Some(AnchorAlign::End),
        v_from: AnchorFrame::Text,
        y_emu: -660_400,
        ..Default::default()
    };
    let mut d = Document::from_plain_text("Minutes\nPresent:");
    d.paragraphs[0].runs.push(Run {
        text: "Ofsted logo".into(),
        style: RunStyle {
            image: Some(img_path.to_string_lossy().into_owned()),
            image_extent_emu: Some((1_296_035, 1_097_915)),
            image_anchor: Some(anchor),
            ..Default::default()
        },
    });
    let rt = round_trip(&d);
    let texts: Vec<String> = rt.paragraphs.iter().map(|p| p.runs.iter().filter(|r| r.style.image.is_none()).map(|r| r.text.as_str()).collect()).collect();
    assert_eq!(texts, ["Minutes", "Present:"], "no paragraph split off");
    let logo = rt.paragraphs[0].runs.iter().find(|r| r.style.image.is_some()).expect("the logo stays in its paragraph");
    assert_eq!(logo.style.image_anchor, Some(anchor));
    assert_eq!(logo.style.image_extent_emu, Some((1_296_035, 1_097_915)));
    assert_eq!(logo.text, "Ofsted logo", "alt text");
    assert_eq!(std::fs::read(logo.style.image.as_ref().unwrap()).unwrap(), png);
}

/// A paragraph holding only a floating picture is a line of its mark's
/// font, as a blank one is: the audit-committee minutes float their logo
/// from a paragraph whose mark is 14pt, and Writer sizes that line by the
/// mark whatever the drawing run's own size. Read at the body size, it
/// stood the title 3.5pt high and pulled the next page's first paragraph,
/// with its rule, onto the first.
#[test]
fn a_floating_pictures_paragraph_keeps_its_marks_size() {
    use letters_core::model::ImageAnchor;
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
        0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x01, 0x9E, 0xDD, 0x22, 0x71, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("logo.png");
    std::fs::write(&img_path, png).unwrap();
    let mut d = Document::from_plain_text("\nMinutes");
    d.paragraphs[0].runs = vec![Run {
        text: "logo".into(),
        style: RunStyle {
            image: Some(img_path.to_string_lossy().into_owned()),
            image_extent_emu: Some((914_400, 914_400)),
            image_anchor: Some(ImageAnchor::default()),
            ..Default::default()
        },
    }];
    let doctored = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let p = body.find("<w:body>").expect("a body") + body[body.find("<w:body>").unwrap()..].find("<w:p").unwrap();
        let open = p + body[p..].find('>').unwrap() + 1;
        let mark = "<w:rPr><w:rFonts w:ascii=\"Tahoma\" w:hAnsi=\"Tahoma\"/><w:sz w:val=\"28\"/></w:rPr>";
        if body[open..].starts_with("<w:pPr>") {
            let close = open + body[open..].find("</w:pPr>").unwrap();
            body.insert_str(close, mark);
        } else {
            body.insert_str(open, &format!("<w:pPr>{mark}</w:pPr>"));
        }
    });
    let logo = doctored.paragraphs[0].runs.iter().find(|r| r.style.image_anchor.is_some()).expect("the floating logo");
    assert_eq!(logo.style.font_size_hp, Some(28), "its line is the 14pt mark's");
    assert_eq!(logo.style.font_family.as_deref(), Some("Tahoma"));
    assert_eq!(doctored.paragraphs[1].text(), "Minutes");
}

/// A letterhead's logo comes back in the header, above its text with its
/// page field, and a footer's floating picture keeps its place.
#[test]
fn header_and_footer_pictures_survive() {
    use letters_core::model::{AnchorAlign, AnchorFrame, ImageAnchor};
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
        0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x01, 0x9E, 0xDD, 0x22, 0x71, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("logo.png");
    std::fs::write(&img_path, png).unwrap();
    let picture = |alt: &str, anchor| Run {
        text: alt.into(),
        style: RunStyle {
            image: Some(img_path.to_string_lossy().into_owned()),
            image_extent_emu: Some((1_304_925, 542_925)),
            image_anchor: anchor,
            ..Default::default()
        },
    };
    let seal = ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, ..Default::default() };
    let mut d = Document::from_plain_text("body");
    d.header = Some("\nOFSI, page {page}".into());
    d.header_pictures = vec![picture("HM Treasury", None)];
    d.footer = Some("October 2025".into());
    d.footer_pictures = vec![picture("seal", Some(seal))];
    let rt = round_trip(&d);
    assert_eq!(rt.header.as_deref(), Some("\nOFSI, page {page}"), "the logo's own line, then the text");
    assert_eq!(rt.footer.as_deref(), Some("October 2025"));
    let logo = &rt.header_pictures[..];
    assert_eq!(logo.len(), 1, "{:?}", rt.header_pictures);
    assert_eq!((logo[0].text.as_str(), logo[0].style.image_extent_emu, logo[0].style.image_anchor), ("HM Treasury", Some((1_304_925, 542_925)), None));
    assert_eq!(std::fs::read(logo[0].style.image.as_ref().unwrap()).unwrap(), png);
    assert_eq!(rt.footer_pictures.iter().map(|r| r.style.image_anchor).collect::<Vec<_>>(), [Some(seal)]);
    assert!(rt.paragraphs.iter().all(|p| p.runs.iter().all(|r| r.style.image.is_none())), "the pictures stay out of the body");
}

/// A header's and footer's alignment survive a save and reopen: a bill's
/// "FINANCE BILL" at the right, its classification centred, with a page
/// field.
#[test]
fn header_and_footer_alignment_survive() {
    let mut d = Document::from_plain_text("body");
    d.header = Some("FINANCE BILL".into());
    d.header_alignment = Alignment::Right;
    d.footer = Some("OFFICIAL, page {page}".into());
    d.footer_alignment = Alignment::Center;
    let rt = round_trip(&d);
    assert_eq!((rt.header.as_deref(), rt.header_alignment), (Some("FINANCE BILL"), Alignment::Right));
    assert_eq!((rt.footer.as_deref(), rt.footer_alignment), (Some("OFFICIAL, page {page}"), Alignment::Center));
    let mut d = Document::from_plain_text("body");
    d.header = Some("Report".into());
    assert_eq!(round_trip(&d).header_alignment, Alignment::Left);
}

/// Reopening a document reuses the image it already extracted rather than
/// leaving another file in the temp dir on every open (#455). The file
/// lives in the process's private media cache, not loose in /tmp.
#[test]
fn reopening_a_document_does_not_extract_its_image_again() {
    let png: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("dot.png");
    std::fs::write(&img_path, png).unwrap();
    let mut d = Document::from_plain_text("before");
    d.paragraphs.push(Paragraph {
        style: ParaStyle::default(),
        runs: vec![Run {
            text: "pic".into(),
            style: RunStyle {
                image: Some(img_path.to_string_lossy().into_owned()),
                ..Default::default()
            },
        }],
    });
    let path = dir.path().join("t.docx");
    let path = path.to_str().unwrap();
    docx::write(&d, path).expect("write docx");

    let extracted = |doc: &Document| {
        doc.paragraphs.iter().flat_map(|p| p.runs.iter())
            .find_map(|r| r.style.image.clone())
            .expect("image run lost")
    };
    let first = extracted(&docx::read(path).expect("read docx"));
    assert_eq!(std::fs::read(&first).unwrap(), png);
    for _ in 0..5 {
        assert_eq!(extracted(&docx::read(path).expect("read docx")), first);
    }
    let cache = suite_common_core::media_cache::process_dir().expect("cache exists");
    assert!(std::path::Path::new(&first).starts_with(&cache), "{first} not in {cache:?}");
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
    // It reopens looking as it did: the Quote style written is the
    // built-in quote's look, 18pt in and italic, now the file's own.
    let q = &rt.paragraphs[0];
    assert!(q.style.own_look);
    assert_eq!(q.style.left_indent_pt, letters_core::layout::QUOTE_INDENT_PT);
    assert!(q.runs.iter().all(|r| r.style.italic), "{:?}", q.runs);
}

/// A file's own Quote style is the quote's look: Writer draws the render
/// lab's block quote at the margin, its Quote style setting italics but no
/// indent, so none is added to it.
#[test]
fn a_files_own_quote_style_is_its_look() {
    let d = doctor_parts(&Document::from_plain_text("wise words"), |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        *body = body.replacen("<w:p>", "<w:p><w:pPr><w:pStyle w:val=\"Quote\"/></w:pPr>", 1);
        let styles = parts.get_mut("word/styles.xml").unwrap();
        let at = styles.find("w:styleId=\"Quote\"").and_then(|i| styles[..i].rfind("<w:style ")).unwrap();
        let end = at + styles[at..].find("</w:style>").unwrap() + "</w:style>".len();
        styles.replace_range(at..end, "<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:rPr><w:i/></w:rPr></w:style>");
    });
    let q = &d.paragraphs[0].style;
    assert!(q.block_quote && q.own_look, "{q:?}");
    assert_eq!(letters_core::layout::paragraph_box_x(&d.paragraphs[0]), 0.0, "no indent of its own, none added");
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
    // Letters' own Title comes back with Letters' look, not a file's.
    assert!(!rt.paragraphs[0].style.own_look && !rt.paragraphs[1].style.own_look);
    assert!(rt.paragraphs[2].style.page_break_before, "page break lost");
    assert!(!rt.paragraphs[0].style.page_break_before);
}

/// A border's gap to the text (`w:space`, points) is read and written:
/// it sets where Writer draws the border and how tall the paragraph is.
#[test]
fn border_gaps_survive() {
    let mut d = Document::from_plain_text("ruled\nboxed");
    let edge = |gap: f64| Some(ParaBorder { style: BorderStyle::Single, color: "4F81BD".into(), width_pt: 1.0, space_pt: gap });
    d.paragraphs[0].style.borders = ParaBorders { bottom: edge(4.0), ..Default::default() };
    d.paragraphs[1].style.borders = ParaBorders { top: edge(1.0), left: edge(0.0), ..Default::default() };
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.borders, d.paragraphs[0].style.borders);
    assert_eq!(rt.paragraphs[1].style.borders, d.paragraphs[1].style.borders);
}

/// A file's own Title and Subtitle styles are its look: their runs carry
/// the style's size, colour and letter spacing (python-docx's template:
/// 26pt plain, 12pt italic spaced 0.75pt), and layout adds none of
/// Letters' built-in bold, scaled look on top (render-lab
/// `letters/named-styles`).
#[test]
fn a_files_own_title_and_subtitle_styles_are_its_look() {
    let mut d = Document::from_plain_text("Quarterly Report\nThird quarter\nbody");
    d.paragraphs[0].style.named_style = Some("Title".into());
    d.paragraphs[1].style.named_style = Some("Subtitle".into());
    let rt = doctor_parts(&d, |parts| {
        let st = parts.get_mut("word/styles.xml").unwrap();
        for id in ["Title", "Subtitle"] {
            if let Some(a) = st.find(&format!("<w:style w:type=\"paragraph\" w:styleId=\"{id}\"")) {
                let b = st[a..].find("</w:style>").unwrap() + a + "</w:style>".len();
                st.replace_range(a..b, "");
            }
        }
        *st = st.replace(
            "</w:styles>",
            "<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/>\
             <w:rPr><w:color w:val=\"17365D\"/><w:spacing w:val=\"5\"/><w:sz w:val=\"52\"/></w:rPr></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Subtitle\"><w:name w:val=\"Subtitle\"/><w:basedOn w:val=\"Normal\"/>\
             <w:rPr><w:i/><w:color w:val=\"4F81BD\"/><w:spacing w:val=\"15\"/></w:rPr></w:style></w:styles>",
        );
    });
    let (title, sub) = (&rt.paragraphs[0], &rt.paragraphs[1]);
    assert_eq!(title.style.named_style.as_deref(), Some("Title"));
    assert!(title.style.own_look && sub.style.own_look, "the file defines both styles");
    assert!(!rt.paragraphs[2].style.own_look);
    assert_eq!(title.runs[0].style.font_size_hp, Some(52));
    assert!(!title.runs[0].style.bold);
    assert_eq!(title.runs[0].style.letter_spacing_tw, Some(5));
    assert_eq!(sub.runs[0].style.letter_spacing_tw, Some(15));
    assert!(sub.runs[0].style.italic);
    // Laid out, neither is scaled: the Subtitle stays at body size.
    let tree = letters_core::layout::layout(&rt, &Default::default(), &mut letters_core::layout::MonoShaper);
    let heights: Vec<f64> = tree.pages[0]
        .lines()
        .filter_map(|l| match l {
            letters_core::layout::Item::Line { height_pt, .. } => Some(*height_pt),
            _ => None,
        })
        .collect();
    assert!((heights[1] - heights[2]).abs() < 1e-9, "{heights:?}");
    // Letter spacing survives a save.
    assert_eq!(round_trip(&rt).paragraphs[1].runs[0].style.letter_spacing_tw, Some(15));
}

/// A page break in a table cell breaks nothing, as in Writer: the WGA
/// form's logo cell opens with two, and its logo sits at the cell's top.
/// They read as two blank lines and a page break before the table.
#[test]
fn a_page_break_in_a_table_cell_is_dropped() {
    use letters_core::model::TableCell;
    let mut d = Document::from_plain_text("");
    d.paragraphs = vec![
        Paragraph { style: ParaStyle { table_cell: Some(TableCell { table: 0, row: 0, col: 0 }), ..Default::default() }, runs: vec![Run::plain("logo")] },
        Paragraph { style: ParaStyle { table_cell: Some(TableCell { table: 0, row: 0, col: 1 }), ..Default::default() }, runs: vec![Run::plain("Form CG02")] },
        Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("after")] },
    ];
    let rt = doctor_parts(&d, |parts| {
        let doc = parts.get_mut("word/document.xml").unwrap();
        let at = doc.find(">logo<").expect("the cell's text");
        let run = doc[..at].rfind("<w:r>").or_else(|| doc[..at].rfind("<w:r ")).expect("its run");
        doc.insert_str(run, "<w:r><w:br w:type=\"page\"/></w:r><w:r><w:br w:type=\"page\"/></w:r>");
    });
    let cell = &rt.paragraphs[0];
    assert!(cell.style.table_cell.is_some());
    assert_eq!(cell.text(), "logo");
    assert!(!cell.style.page_break_before);
}

/// A border edge Word leaves off (`w:val="none"`) but gives a gap pads
/// the paragraph above and below, as Writer does; the side edges do not
/// set the text in. The questionnaire's Normal style puts 31pt of such
/// padding on every paragraph that does not clear it.
#[test]
fn an_invisible_border_keeps_its_gap_above_and_below() {
    let d = Document::from_plain_text("before\n\nafter");
    let rt = doctor_parts(&d, |parts| {
        let edges: String = ["top", "left", "bottom", "right"]
            .iter()
            .map(|s| format!("<w:{s} w:val=\"none\" w:sz=\"96\" w:space=\"31\" w:color=\"FFFFFF\" w:frame=\"1\"/>"))
            .collect();
        let st = parts.get_mut("word/styles.xml").unwrap();
        let name = "<w:name w:val=\"Normal\"/>";
        assert!(st.contains(name));
        *st = st.replacen(name, &format!("{name}<w:pPr><w:pBdr>{edges}</w:pBdr></w:pPr>"), 1);
    });
    // The blank line between the two takes the Normal style's edges.
    let b = &rt.paragraphs[1].style.borders;
    let top = b.top.as_ref().expect("the top edge's gap is kept");
    assert_eq!((top.style, top.space_pt), (BorderStyle::None, 31.0));
    assert_eq!(b.bottom.as_ref().map(|e| e.style), Some(BorderStyle::None));
    assert!(b.left.is_none() && b.right.is_none(), "{b:?}");
    // It survives a save.
    assert_eq!(round_trip(&rt).paragraphs[1].style.borders, *b);
}

#[test]
fn header_footer_survive() {
    let mut d = Document::from_plain_text("body text");
    d.header = Some("Quarterly Report".into());
    d.footer = Some("Page {page} of {total}".into());
    let rt = round_trip(&d);
    assert_eq!(rt.header.as_deref(), Some("Quarterly Report"));
    assert_eq!(rt.footer.as_deref(), Some("Page {page} of {total}"));
}

/// "{page}" and "{total}" are saved as Word's PAGE and NUMPAGES fields, so
/// Word and LibreOffice number each page; they were written as literal
/// "{page}" text.
#[test]
fn page_numbers_are_saved_as_fields() {
    let mut d = Document::from_plain_text("body text");
    d.footer = Some("Page {page} of {total}".into());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.docx");
    docx::write(&d, &path).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut footers = String::new();
    for i in 0..zip.len() {
        let mut part = zip.by_index(i).unwrap();
        if part.name().unwrap().starts_with("word/footer") {
            std::io::Read::read_to_string(&mut part, &mut footers).unwrap();
        }
    }
    assert!(footers.contains("w:instr=\" PAGE \"") && footers.contains("w:instr=\" NUMPAGES \""), "{footers}");
    assert!(!footers.contains("{page}"), "no literal placeholder is left");
}

// ── Page geometry & font family round-trip (PARITY stragglers) ───────

/// A page border survives a save and reopen, measured from the text
/// (`w:offsetFrom="text"`), the margins untouched.
#[test]
fn a_page_border_round_trips() {
    use letters_core::model::{PageBorder, PageGeometry};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("border.docx");
    let mut d = Document::from_plain_text("body");
    let border = PageBorder { width_pt: 0.75, color: 0x80_00_00, padding_pt: [1.0, 4.0, 2.0, 4.0] };
    d.page = Some(PageGeometry { border: Some(border), ..PageGeometry::default() });
    docx::write(&d, path.to_str().unwrap()).expect("write");
    let pg = docx::read(path.to_str().unwrap()).expect("read").page.expect("page geometry lost");
    assert!(pg.approx_eq(&d.page.unwrap()), "geometry drifted: {pg:?}");
    assert_eq!(pg.border, Some(border));
}

/// A border measured from the page's edge (`w:offsetFrom="page"`, as
/// Word's own dialog writes it) is as far from the text as the margin
/// leaves.
#[test]
fn a_page_border_from_the_page_edge_is_read_against_the_margin() {
    let mut d = Document::from_plain_text("body");
    d.page = Some(letters_core::model::PageGeometry::default());
    let d = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let at = body.find("<w:pgMar").expect("the section's margins");
        let at = at + body[at..].find("/>").unwrap() + 2;
        let edge = |side: &str| format!(r#"<w:{side} w:val="single" w:sz="8" w:space="24" w:color="auto"/>"#);
        let borders = format!(r#"<w:pgBorders w:offsetFrom="page">{}{}{}{}</w:pgBorders>"#, edge("top"), edge("left"), edge("bottom"), edge("right"));
        body.insert_str(at, &borders);
    });
    let b = d.page.and_then(|p| p.border).expect("the page's border");
    assert_eq!((b.width_pt, b.color), (1.0, 0));
    assert_eq!(b.padding_pt, [72.0 - 24.0 - 1.0; 4]);
}

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
        header_pt: Some(18.0),
        footer_pt: Some(27.5),
        border: None,
    });
    docx::write(&d, path.to_str().unwrap()).expect("write");
    let rt = docx::read(path.to_str().unwrap()).expect("read");
    let pg = rt.page.expect("page geometry lost");
    assert!(pg.approx_eq(&d.page.unwrap()), "geometry drifted: {pg:?}");
    assert_eq!((pg.header_pt, pg.footer_pt), (Some(18.0), Some(27.5)), "the header and footer distances");
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
            let name = f.name().unwrap().into_owned();
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

/// Strict-OOXML indents inside table cells (#1204).
///
/// The scan that reads the strict spelling walked the body only and
/// skipped `w:tbl`, so an indent in a cell of a LibreOffice-exported
/// table read as zero. Cells are paired with their paragraphs by position
/// as the body is, so the check uses two cells, an empty one between them,
/// and a body paragraph before the table: a misaligned pairing would put
/// an indent on the wrong cell.
#[test]
fn strict_ooxml_indents_inside_table_cells_are_read() {
    use letters_core::model::TableCell;
    let cell = |row, col, left: f64, right: f64, text: &str| Paragraph {
        style: ParaStyle {
            left_indent_pt: left,
            right_indent_pt: right,
            table_cell: Some(TableCell { table: 0, row, col }),
            ..Default::default()
        },
        runs: if text.is_empty() { vec![] } else { vec![Run::plain(text)] },
    };
    let mut d = Document::from_plain_text("");
    d.paragraphs = vec![
        Paragraph {
            style: ParaStyle { left_indent_pt: 6.0, ..Default::default() },
            runs: vec![Run::plain("before")],
        },
        cell(0, 0, 0.0, 0.0, "plain"),
        cell(0, 1, 36.0, 0.0, "left"),
        cell(1, 0, 0.0, 0.0, ""),
        cell(1, 1, 0.0, 24.0, "right"),
    ];
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("transitional.docx");
    docx::write(&d, &src).expect("write docx");

    let strict = dir.path().join("strict.docx");
    {
        let mut zin = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
        let mut zout = zip::ZipWriter::new(std::fs::File::create(&strict).unwrap());
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().unwrap().into_owned();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
            if name == "word/document.xml" {
                let xml = String::from_utf8(buf).unwrap();
                let at = xml.find("<w:tbl>").expect("fixture needs a table");
                let table = &xml[at..at + xml[at..].find("</w:tbl>").expect("a closed table")];
                assert!(table.contains("w:left=") && table.contains("w:right="),
                        "fixture needs transitional indents inside the table: {xml}");
                buf = xml.replace("w:left=", "w:start=").replace("w:right=", "w:end=").into_bytes();
            }
            zout.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zout, &buf).unwrap();
        }
        zout.finish().unwrap();
    }

    let rt = docx::read(strict.to_str().unwrap()).expect("read strict docx");
    let indents = |row, col| {
        let p = rt.paragraphs.iter()
            .find(|p| p.style.table_cell.is_some_and(|c| (c.row, c.col) == (row, col)))
            .unwrap_or_else(|| panic!("no cell {row},{col}"));
        (p.style.left_indent_pt, p.style.right_indent_pt)
    };
    assert_eq!(indents(0, 0), (0.0, 0.0), "the unindented cell");
    assert_eq!(indents(0, 1), (36.0, 0.0), "the left-indented cell");
    assert_eq!(indents(1, 1), (0.0, 24.0), "the right-indented cell");
    assert_eq!(rt.paragraphs[0].style.left_indent_pt, 6.0, "the body paragraph before the table");
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
    d.paragraphs[0].style.tab_stops_pt = vec![TabStop::left(36.0), TabStop::left(108.0), TabStop::left(180.0)];
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.tab_stops_pt.len(), 3, "stops lost");
    for (got, want) in rt.paragraphs[0].style.tab_stops_pt.iter().zip([36.0, 108.0, 180.0]) {
        assert!((got.pos_pt - want).abs() < 0.1, "stop {} != {want}", got.pos_pt);
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
    d.paragraphs[0].style.tab_stops_pt = vec![TabStop::left(36.0)];
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("plain.docx");
    docx::write(&d, &src).expect("write docx");

    let doctored = dir.path().join("doctored.docx");
    {
        let mut zin = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
        let mut zout = zip::ZipWriter::new(std::fs::File::create(&doctored).unwrap());
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().unwrap().into_owned();
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
    assert!((stops[0].pos_pt - 36.0).abs() < 0.1, "the real stop was lost: {stops:?}");
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
            let name = f.name().unwrap().into_owned();
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
        parts.push((f.name().unwrap().into_owned(), buf));
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

/// Merged cells survive a save: a heading across the table, a cell down
/// two rows, and one across two columns and down two rows.
///
/// The reader put every cell at its place in the row, not in the grid: the
/// cells after a merged one moved left, and a heading across the table
/// was squeezed into its first column (render-real `crime-supervisor`).
#[test]
fn merged_cells_survive_a_docx_save() {
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 4, 3);
    for p in d.paragraphs.iter_mut() {
        if let Some(c) = p.style.table_cell {
            p.runs = vec![Run::plain(format!("r{}c{}", c.row, c.col))];
        }
    }
    let spans = vec![
        CellSpan { row: 0, col: 0, rows: 1, cols: 3 },
        CellSpan { row: 1, col: 0, rows: 2, cols: 1 },
        CellSpan { row: 1, col: 1, rows: 2, cols: 2 },
    ];
    // Covered positions hold nothing.
    for p in d.paragraphs.iter_mut() {
        if let Some(c) = p.style.table_cell {
            if spans.iter().any(|s| s.contains(c.row, c.col) && (s.row, s.col) != (c.row, c.col)) {
                p.runs.clear();
            }
        }
    }
    d.table_spans.insert(table, spans.clone());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merged.docx");
    docx::write(&d, &path).unwrap();
    let rt = docx::read(path.to_str().unwrap()).unwrap();
    assert_eq!(rt.table_spans.values().collect::<Vec<_>>(), [&spans], "the reader numbers tables by their order");
    let texts = |d: &Document| -> Vec<(u32, u32, String)> {
        d.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| (c.row, c.col, p.text()))).filter(|(_, _, t)| !t.is_empty()).collect()
    };
    assert_eq!(texts(&rt), texts(&d));
    assert_eq!(texts(&rt), [(0, 0, "r0c0".into()), (1, 0, "r1c0".into()), (1, 1, "r1c1".into()), (3, 0, "r3c0".into()), (3, 1, "r3c1".into()), (3, 2, "r3c2".into())]);
    // The grid stays whole: a paragraph at every position.
    let positions: std::collections::BTreeSet<(u32, u32)> = rt.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| (c.row, c.col))).collect();
    assert_eq!(positions.len(), 12);
}

/// A list continues past a paragraph that is not in it.
///
/// Word counts each list instance (`numId`) on its own: minutes numbered
/// "1", then a paragraph of discussion, then "2" are one list. This model's
/// count ends a list at any plain paragraph, so the second item read as a
/// second "1." (render-real `audit-and-risk-assurance-committee-minutes`).
/// The reader restarts it at Word's number, and leaves alone the items the
/// two counts already agree on.
#[test]
fn a_numbered_list_continues_past_a_plain_paragraph() {
    let mut d = Document::from_plain_text("first\nnotes on it\nsecond\nthird\nnot numbered\nfourth");
    for k in [0, 2, 3, 5] {
        d.paragraphs[k].style.list = ListKind::Numbered;
    }
    let rt = doctor_parts(&d, |parts| {
        // Our writer gives each run of items its own instance; Word gives
        // the list one. Point every item at the first item's instance.
        let body = parts.get_mut("word/document.xml").unwrap();
        let ids: Vec<String> = body
            .split("<w:numId w:val=\"")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap().to_string())
            .collect();
        assert_eq!(ids.len(), 4, "fixture shape changed: {body}");
        for id in &ids[1..] {
            *body = body.replace(&format!("<w:numId w:val=\"{id}\""), &format!("<w:numId w:val=\"{}\"", ids[0]));
        }
    });
    let starts: Vec<Option<u32>> = rt.paragraphs.iter().map(|p| p.style.list_start).collect();
    assert_eq!(starts, vec![None, None, Some(2), None, None, Some(4)]);
    let shown = letters_core::lists::ordinals(rt.paragraphs.iter().map(|p| &p.style));
    assert_eq!(shown, vec![1, 0, 2, 3, 0, 4]);
}

/// Two lists stay two lists through a save.
///
/// The writer put every numbered item on one list instance, which Word
/// counts as one list: "1, 2 / a plain paragraph / 1, 2" opened in Word as
/// "1, 2 / 3, 4", and the reader, which counts as Word does, read it back
/// that way.
#[test]
fn separate_numbered_lists_keep_their_numbers_through_a_save() {
    let mut d = Document::from_plain_text("a\nb\nbetween\nc\nd\n  nested\ne");
    for k in [0, 1, 3, 4, 6] {
        d.paragraphs[k].style.list = ListKind::Numbered;
    }
    d.paragraphs[5].style.list = ListKind::Numbered;
    d.paragraphs[5].style.list_level = 1;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lists.docx");
    docx::write(&d, &path).unwrap();
    let rt = docx::read(path.to_str().unwrap()).unwrap();
    let shown = |d: &Document| letters_core::lists::ordinals(d.paragraphs.iter().map(|p| &p.style));
    assert_eq!(shown(&d), vec![1, 2, 0, 1, 2, 1, 3]);
    assert_eq!(shown(&rt), shown(&d));
    let starts: Vec<Option<u32>> = rt.paragraphs.iter().map(|p| p.style.list_start).collect();
    assert_eq!(starts, vec![None; 7], "a list that starts at 1 needs no restart");
    let labels: Vec<Option<ListLabel>> = rt.paragraphs.iter().map(|p| p.style.list_label.clone()).collect();
    assert_eq!(labels, vec![None; 7], "a nested item is \"1.\" as Letters draws it, not Word's default \"a.\"");
}

fn labelled_lists() -> Document {
    let label = |prefix: &str, format, suffix: &str| Some(ListLabel { prefix: prefix.into(), format, suffix: suffix.into() });
    let mut d = Document::from_plain_text("one\na\nb\ntwo\nthree\nfour\nbetween\n2.1\n2.1 again");
    for (k, level, l) in [
        (0, 0, label("(", NumberFormat::Decimal, ")")),
        (1, 1, label("1.", NumberFormat::LowerLetter, "")),
        (2, 1, label("1.", NumberFormat::LowerLetter, "")),
        (3, 0, label("(", NumberFormat::Decimal, ")")),
        // The label changes within the list, and back.
        (4, 0, label("", NumberFormat::UpperRoman, ".")),
        (5, 0, None),
        (7, 0, label("2.1", NumberFormat::None, "")),
        (8, 0, label("2.1", NumberFormat::None, "")),
    ] {
        d.paragraphs[k].style.list = ListKind::Numbered;
        d.paragraphs[k].style.list_level = level;
        d.paragraphs[k].style.list_label = l;
    }
    d
}

fn markers(d: &Document) -> Vec<String> {
    let ordinals = letters_core::lists::ordinals(d.paragraphs.iter().map(|p| &p.style));
    d.paragraphs.iter().zip(ordinals).map(|(p, n)| letters_core::lists::marker_for(&p.style, n).unwrap_or_default()).collect()
}

/// A numbered item's label survives a save: "(1)", "1.a", "III.", and
/// Word's "2.1" with no number in it.
#[test]
fn list_labels_survive_a_docx_save() {
    let d = labelled_lists();
    assert_eq!(markers(&d), ["(1)", "1.a", "1.b", "(2)", "III.", "4.", "", "2.1", "2.1"]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("labels.docx");
    docx::write(&d, &path).unwrap();
    let rt = docx::read(path.to_str().unwrap()).unwrap();
    assert_eq!(markers(&rt), markers(&d));
    let labels = |d: &Document| d.paragraphs.iter().map(|p| p.style.list_label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(&rt), labels(&d));
}

/// Word's level text: the levels above as they stand ("%1.%2" under
/// item 3 is "3.1"), the item's own format, and text with no number.
#[test]
fn word_level_text_becomes_the_items_label() {
    let mut d = Document::from_plain_text("top\nsub\nsub");
    d.paragraphs[0].style.list = ListKind::Numbered;
    d.paragraphs[0].style.list_start = Some(3);
    for p in &mut d.paragraphs[1..] {
        p.style.list = ListKind::Numbered;
        p.style.list_level = 1;
    }
    let rt = doctor_parts(&d, |parts| {
        let numbering = parts.get_mut("word/numbering.xml").unwrap();
        // Level 2 of every definition: "%1.%2", lower letters.
        let at = numbering.find("<w:lvl w:ilvl=\"1\"").expect("fixture shape changed");
        let end = numbering[at..].find("</w:lvl>").unwrap() + at;
        let level = numbering[at..end]
            .replace("w:val=\"decimal\"", "w:val=\"lowerLetter\"")
            .replace("w:val=\"%2.\"", "w:val=\"%1.%2\"");
        assert!(level.contains("%1.%2") && level.contains("lowerLetter"), "fixture shape changed: {level}");
        numbering.replace_range(at..end, &level);
    });
    assert_eq!(markers(&rt), ["3.", "3.a", "3.b"]);
}

/// White shading is no highlight.
///
/// Forms pasted from the web shade their text white (`w:shd w:fill=
/// "FFFFFF"`) on a white page. The reader took any run shading for a
/// highlight, so that text drew yellow where Word and LibreOffice draw
/// nothing (render-real `tier-2-3-notification`).
#[test]
fn white_or_automatic_shading_is_not_a_highlight() {
    let d = Document::from_plain_text("white\nauto\nyellow\nmarked\nunmarked");
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        for (text, rpr) in [
            ("white", "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"FFFFFF\"/>"),
            ("auto", "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"auto\"/>"),
            ("yellow", "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"FFFF00\"/>"),
            ("marked", "<w:highlight w:val=\"green\"/>"),
            ("unmarked", "<w:highlight w:val=\"none\"/><w:shd w:val=\"clear\" w:fill=\"FFFF00\"/>"),
        ] {
            let run = format!("<w:r><w:t>{text}</w:t></w:r>");
            assert!(body.contains(&run), "fixture shape changed: {body}");
            *body = body.replace(&run, &format!("<w:r><w:rPr>{rpr}</w:rPr><w:t>{text}</w:t></w:r>"));
        }
    });
    let highlighted: Vec<(String, bool)> =
        rt.paragraphs.iter().map(|p| (p.text(), p.runs.iter().any(|r| r.style.highlight))).collect();
    assert_eq!(
        highlighted,
        vec![
            ("white".into(), false),
            ("auto".into(), false),
            ("yellow".into(), true),
            ("marked".into(), true),
            ("unmarked".into(), false),
        ]
    );
}

/// The paragraph mark's formatting is the mark's alone.
///
/// `w:pPr/w:rPr` formats the pilcrow, not the paragraph's text. rdocx's
/// effective run properties merge it into every run, so a form whose
/// marks were bold read every paragraph as bold (render-real
/// `tier-2-3-notification`). A run that turns its style's bold off stays
/// plain.
#[test]
fn the_paragraph_marks_formatting_is_not_the_texts() {
    let d = Document::from_plain_text("plain text\nnot bold");
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let run = "<w:r><w:t>plain text</w:t></w:r>";
        assert!(body.contains(run), "fixture shape changed: {body}");
        *body = body.replacen(
            "<w:p>",
            "<w:p><w:pPr><w:rPr><w:b/><w:i/><w:color w:val=\"FF0000\"/><w:sz w:val=\"40\"/></w:rPr></w:pPr>",
            1,
        );
        let run = "<w:r><w:t>not bold</w:t></w:r>";
        assert!(body.contains(run), "fixture shape changed: {body}");
        *body = body.replace(run, "<w:r><w:rPr><w:rStyle w:val=\"Strong\"/><w:b w:val=\"0\"/></w:rPr><w:t>not bold</w:t></w:r>");
        let styles = parts.get_mut("word/styles.xml").unwrap();
        *styles = styles.replace(
            "</w:styles>",
            "<w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/></w:rPr></w:style></w:styles>",
        );
    });
    assert!(rt.paragraphs[0].text() == "plain text", "{:?}", rt.paragraphs);
    let r = &rt.paragraphs[0].runs[0].style;
    assert!(!r.bold && !r.italic, "{r:?}");
    assert_eq!((r.color.as_deref(), r.font_size_hp), (None, None), "{r:?}");
    assert!(!rt.paragraphs[1].runs[0].style.bold, "a run's w:b=0 turns its style's bold off");
}

/// What a paragraph inherits from its styles is how it looks.
///
/// python-docx's template (and Word's) puts 10pt after every paragraph and
/// 1.15 line spacing in docDefaults, the body font in the Normal style, and
/// contextual spacing on the list styles. Reading only direct properties
/// drew those documents tight, in the wrong font, and paginated them onto
/// too few pages (render-lab `letters/pagination`: 3 pages vs 4).
#[test]
fn style_inherited_spacing_font_and_contextual_spacing_are_read() {
    let mut d = Document::from_plain_text("intro\none\ntwo\nthree\noutro");
    for p in &mut d.paragraphs[1..4] {
        p.style.list = ListKind::Bullet;
    }
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let num_id = body.split("<w:numId w:val=\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        while let Some(start) = body.find("<w:numPr>") {
            let end = body[start..].find("</w:numPr>").unwrap() + start + "</w:numPr>".len();
            body.replace_range(start..end, "<w:pStyle w:val=\"ListBullet\"/>");
        }
        let st = parts.get_mut("word/styles.xml").unwrap();
        let a = st.find("<w:docDefaults").unwrap();
        let b = st.find("</w:docDefaults>").unwrap() + "</w:docDefaults>".len();
        st.replace_range(
            a..b,
            "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme=\"minorHAnsi\"/><w:sz w:val=\"22\"/>\
             </w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"200\" w:line=\"276\" \
             w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
        );
        // The body font lives on Normal, as python-docx's fixtures set it.
        let normal = st.find("w:styleId=\"Normal\"").expect("a Normal style");
        let close = st[normal..].find('>').unwrap() + normal + 1;
        st.insert_str(close, "<w:rPr><w:rFonts w:ascii=\"Liberation Serif\" w:hAnsi=\"Liberation Serif\"/><w:sz w:val=\"24\"/></w:rPr>");
        *st = st.replace(
            "</w:styles>",
            &format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"ListBullet\"><w:name w:val=\"List Bullet\"/>\
                 <w:basedOn w:val=\"Normal\"/><w:pPr><w:numPr><w:numId w:val=\"{num_id}\"/></w:numPr>\
                 <w:contextualSpacing/></w:pPr></w:style></w:styles>"
            ),
        );
    });
    assert_eq!(rt.base_font.family.as_deref(), Some("Liberation Serif"));
    assert_eq!(rt.base_font.size_hp, Some(24));
    let s: Vec<(f64, f64, f32)> =
        rt.paragraphs.iter().map(|p| (p.style.space_before_pt, p.style.space_after_pt, p.style.line_spacing)).collect();
    assert_eq!(s[0], (0.0, 10.0, 1.15), "docDefaults spacing on a Normal paragraph");
    assert_eq!(s[1].1, 0.0, "no space between items of one contextual list style");
    assert_eq!(s[2].1, 0.0);
    assert_eq!(s[3].1, 10.0, "the last item keeps its space before a Normal paragraph");
    assert_eq!(s[4], (0.0, 10.0, 1.15));
    assert!(rt.paragraphs[0].runs.iter().all(|r| r.style.font_family.is_none() && r.style.font_size_hp.is_none()),
        "body runs in the body font carry no font of their own: {:?}", rt.paragraphs[0].runs);
}

/// A document Letters writes looks in Word and LibreOffice as it does in
/// Letters, and the same again when Letters reopens it: no inherited
/// paragraph spacing, the base font, and all six heading styles.
#[test]
fn a_written_docx_carries_letters_own_styles() {
    let mut d = Document::from_plain_text("Title\nbody");
    d.paragraphs[0].style.heading = Some(2);
    d.base_font = BaseFont { family: Some("Carlito".into()), size_hp: Some(22) };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("styles.docx");
    docx::write(&d, &path).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut styles = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/styles.xml").unwrap(), &mut styles).unwrap();
    for n in 1..=6 {
        assert_eq!(styles.matches(&format!("w:styleId=\"Heading{n}\"")).count(), 1, "Heading{n} defined once");
    }
    assert!(styles.contains("w:after=\"0\""), "no inherited space after: {styles}");
    assert!(!styles.contains("Calibri\""), "rdocx's Calibri default is gone");

    let rt = docx::read(path.to_str().unwrap()).unwrap();
    assert_eq!(rt.base_font, d.base_font);
    for p in &rt.paragraphs {
        assert_eq!((p.style.space_before_pt, p.style.space_after_pt, p.style.line_spacing), (0.0, 0.0, 1.0));
    }
    assert_eq!(rt.paragraphs[0].style.heading, Some(2));
    assert!(rt.paragraphs[0].runs.iter().all(|r| r.style.font_size_hp.is_none() && !r.style.bold),
        "a heading's look stays its level, not copied onto its runs");
}

/// A document's heading styles are how its headings look: Word's and
/// python-docx's Heading 1 is 14 pt bold blue in the theme's heading font,
/// not Letters' 1.6x body text. They are read once for the document (not
/// copied onto every heading run) and written back as the styles.
#[test]
fn heading_styles_are_read_and_written_back() {
    let mut d = Document::from_plain_text("Title\nbody");
    d.paragraphs[0].style.heading = Some(1);
    let rt = doctor_parts(&d, |parts| {
        // rdocx 0.14 writes Office's current default theme (Aptos Display /
        // Aptos) into every document. Give its major font a name nothing
        // else in the package uses, so the assertion below can only pass by
        // reading the theme.
        let theme = parts.get_mut("word/theme/theme1.xml").expect("the writer emits a theme part");
        assert!(theme.contains("<a:majorFont><a:latin typeface=\""), "theme has a latin major font");
        let at = theme.find("<a:majorFont><a:latin typeface=\"").unwrap() + "<a:majorFont><a:latin typeface=\"".len();
        let end = theme[at..].find('"').unwrap() + at;
        theme.replace_range(at..end, "Theme Major Face");
        let st = parts.get_mut("word/styles.xml").unwrap();
        let a = st.find("w:styleId=\"Heading1\"").map(|i| st[..i].rfind("<w:style ").unwrap());
        if let Some(a) = a {
            let b = st[a..].find("</w:style>").unwrap() + a + "</w:style>".len();
            st.replace_range(a..b, "");
        }
        *st = st.replace(
            "</w:styles>",
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/>\
             <w:basedOn w:val=\"Normal\"/><w:rPr><w:rFonts w:ascii=\"Carlito\" w:hAnsi=\"Carlito\"/><w:b/>\
             <w:color w:val=\"365F91\"/><w:sz w:val=\"28\"/></w:rPr></w:style>\
             <w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/>\
             <w:basedOn w:val=\"Normal\"/><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/>\
             </w:rPr></w:style></w:styles>",
        );
    });
    // A derived style's theme font beats its base style's named face (the
    // body font, set on docDefaults by our writer).
    assert_eq!(rt.heading_styles[1].font_family.as_deref(), Some("Theme Major Face"), "the theme's major font");
    assert_ne!(rt.base_font.family.as_deref(), Some("Theme Major Face"));
    let h1 = rt.heading_styles.first().expect("heading styles read");
    assert_eq!(
        (h1.bold, h1.font_family.as_deref(), h1.font_size_hp, h1.color.as_deref()),
        (true, Some("Carlito"), Some(28), Some("365F91"))
    );
    assert!(rt.paragraphs[0].runs.iter().all(|r| r.style == RunStyle::default()), "not copied onto the runs");

    // Written back as the document's Heading 1, and read again the same.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.docx");
    docx::write(&rt, &path).unwrap();
    let again = docx::read(path.to_str().unwrap()).unwrap();
    assert_eq!(again.heading_styles[0], *h1);
}

/// A list item's saved indent is where Letters draws it, and reads back as
/// written (#1201). rdocx's numbering levels say 0.5in per level; left to
/// them, LibreOffice drew our bullets twice as deep as the editor did.
#[test]
fn list_items_declare_the_indent_letters_draws_and_read_back_unchanged() {
    let item = |text: &str, level: u8, left: f64| Paragraph {
        style: ParaStyle { list: ListKind::Bullet, list_level: level, left_indent_pt: left, ..Default::default() },
        runs: vec![Run::plain(text)],
    };
    let doc = Document {
        paragraphs: vec![
            item("top", 0, 0.0),
            item("nested", 1, 0.0),
            item("indented", 0, 24.0),
            Paragraph { style: ParaStyle { left_indent_pt: 36.0, ..Default::default() }, runs: vec![Run::plain("body")] },
        ],
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lists.docx");
    docx::write(&doc, path.to_str().unwrap()).unwrap();

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml).unwrap();
    let inds: Vec<&str> = xml.match_indices("<w:ind ").map(|(i, _)| &xml[i..i + xml[i..].find("/>").unwrap()]).collect();
    // 18pt per level plus the paragraph's own indent, hanging 18pt (twips).
    assert!(inds[0].contains(r#"w:left="360""#) && inds[0].contains(r#"w:hanging="360""#), "{inds:?}");
    assert!(inds[1].contains(r#"w:left="720""#), "{inds:?}");
    assert!(inds[2].contains(r#"w:left="840""#), "{inds:?}");

    let back = docx::read(path.to_str().unwrap()).unwrap();
    let got: Vec<(ListKind, u8, f64, f64)> = back.paragraphs.iter()
        .map(|p| (p.style.list, p.style.list_level, p.style.left_indent_pt, p.style.first_line_indent_pt))
        .collect();
    assert_eq!(got, vec![
        (ListKind::Bullet, 0, 0.0, 0.0),
        (ListKind::Bullet, 1, 0.0, 0.0),
        (ListKind::Bullet, 0, 24.0, 0.0),
        (ListKind::None, 0, 36.0, 0.0),
    ]);
}

/// A list whose depth lives only in its numbering level, as Word's toolbar
/// bullets (and our own writer before #1201) save it: 0.5in per level.
/// Word and LibreOffice draw its text 36pt in; the model keeps the 18pt
/// beyond the layout's own list indent, so Letters draws it there too.
#[test]
fn a_numbering_levels_indent_is_kept_beyond_the_list_indent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("toolbar.docx");
    let mut d = rdocx::Document::new();
    d.add_bullet_list_item("from the toolbar", 0);
    d.save(&path).unwrap();
    let back = docx::read(path.to_str().unwrap()).unwrap();
    let p = back.paragraphs.iter().find(|p| p.runs.iter().any(|r| r.text.contains("toolbar"))).unwrap();
    assert_eq!((p.style.list, p.style.left_indent_pt, p.style.first_line_indent_pt), (ListKind::Bullet, 18.0, 0.0));
}

#[test]
fn a_run_coloured_auto_has_no_colour_of_its_own() {
    // w:color w:val="auto" is Word's "automatic" (the default text colour),
    // not a colour. It reached GTK as "#AUTO" ("Don't know color") from a
    // real GOV.UK form in the render lab's corpus (#1200).
    let d = Document::from_plain_text("plain");
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        *body = body.replacen("<w:r>", "<w:r><w:rPr><w:color w:val=\"auto\"/></w:rPr>", 1);
        assert!(body.contains("w:val=\"auto\""), "the doctored run: {body}");
    });
    assert_eq!(rt.paragraphs[0].runs[0].style.color, None, "{:?}", rt.paragraphs[0].runs);
}

/// In a heading, though, "auto" undoes the heading style's colour: a
/// subtitle set as a blue Heading 2 with its runs in automatic colour is
/// black in Word and LibreOffice (the questionnaire in the real corpus,
/// #1211), so the run reads as black, not as no colour.
#[test]
fn a_heading_run_coloured_auto_is_black() {
    let mut d = Document::from_plain_text("subtitle");
    d.paragraphs[0].style.heading = Some(2);
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let at = body.find("<w:r>").expect("a run");
        body.replace_range(at..at + 5, "<w:r><w:rPr><w:color w:val=\"auto\"/></w:rPr>");
    });
    assert_eq!(rt.paragraphs[0].style.heading, Some(2));
    assert_eq!(rt.paragraphs[0].runs[0].style.color.as_deref(), Some("000000"), "{:?}", rt.paragraphs[0].runs);
}

/// A cell paragraph takes its table style's spacing beneath its own
/// paragraph style (Word: document defaults, then table style, then
/// paragraph style). Word's "Table Grid" sets no space after and single
/// lines; without it, every cell took the document's 8pt after and 1.08
/// lines and the table drew a third taller than in Word or LibreOffice.
/// Body text keeps the document defaults.
#[test]
fn a_cell_paragraph_takes_its_table_styles_spacing() {
    let mut d = Document::from_plain_text("before\nafter");
    d.insert_table_at(1, 1, 1);
    for p in d.paragraphs.iter_mut().filter(|p| p.style.table_cell.is_some()) {
        p.runs = vec![letters_core::Run::plain("cell")];
    }
    let rt = doctor_parts(&d, |parts| {
        let st = parts.get_mut("word/styles.xml").unwrap();
        let a = st.find("<w:pPrDefault>").expect("fixture shape changed");
        let b = st.find("</w:pPrDefault>").unwrap() + "</w:pPrDefault>".len();
        st.replace_range(a..b, "<w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"259\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault>");
        *st = st.replace("</w:styles>", "<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/>\
            <w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:style></w:styles>");
        let body = parts.get_mut("word/document.xml").unwrap();
        // Direct spacing our writer gives each paragraph would hide the
        // inheritance under test.
        while let Some(s) = body.find("<w:spacing ") {
            let e = body[s..].find("/>").unwrap() + s + 2;
            body.replace_range(s..e, "");
        }
        let at = body.find("<w:tblPr>").expect("fixture shape changed") + "<w:tblPr>".len();
        body.insert_str(at, "<w:tblStyle w:val=\"TableGrid\"/>");
    });
    let cell = rt.paragraphs.iter().find(|p| p.style.table_cell.is_some()).expect("the cell");
    assert_eq!((cell.style.space_after_pt, cell.style.line_spacing), (0.0, 1.0), "the cell takes Table Grid's spacing");
    let body = rt.paragraphs.iter().find(|p| p.text() == "before").unwrap();
    assert_eq!(body.style.space_after_pt, 8.0, "body text keeps the document default");
    assert!((body.style.line_spacing - 259.0 / 240.0).abs() < 1e-3);
}

/// A table's column widths survive a save and reopen, so a narrow number
/// column stays narrow; a table without any gets none made up.
#[test]
fn table_column_widths_survive() {
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 1, 2);
    d.table_columns.insert(table, vec![36.0, 400.0]);
    let rt = round_trip(&d);
    let widths: Vec<Vec<f64>> = rt.table_columns.values().cloned().collect();
    assert_eq!(widths.len(), 1, "{:?}", rt.table_columns);
    assert!(widths[0].iter().zip([36.0, 400.0]).all(|(a, b)| (a - b).abs() < 0.1), "{widths:?}");
}

/// A cell's empty paragraphs are lines of the cell: a form's answer box
/// is a cell holding a label and a run of empty lines, and drawing only
/// the label shrank every box to one line (the questionnaire, #1211).
/// A table that declares no visible border, itself, through its style or
/// on a cell, is Word's "Normal Table": drawn with no rules (a form's grey
/// answer box in render-lab `real-questionnaire-2`). One with a style
/// that rules it ("Table Grid") is ruled. Each keeps its look on a save.
#[test]
fn a_table_without_borders_is_drawn_without_rules() {
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 1, 1);
    assert!(round_trip(&d).table_unruled.is_empty(), "a table made here is ruled");
    let strip = |parts: &mut std::collections::BTreeMap<String, String>, style: Option<&str>| {
        let body = parts.get_mut("word/document.xml").unwrap();
        while let Some(a) = body.find("<w:tblBorders>") {
            let b = body[a..].find("</w:tblBorders>").unwrap() + a + "</w:tblBorders>".len();
            body.replace_range(a..b, "");
        }
        while let Some(a) = body.find("<w:tblStyle ") {
            let b = body[a..].find("/>").unwrap() + a + 2;
            body.replace_range(a..b, "");
        }
        if let Some(id) = style {
            *body = body.replacen("<w:tblPr>", &format!("<w:tblPr><w:tblStyle w:val=\"{id}\"/>"), 1);
            let st = parts.get_mut("word/styles.xml").unwrap();
            *st = st.replace(
                "</w:styles>",
                &format!(
                    "<w:style w:type=\"table\" w:styleId=\"Base{id}\"><w:tblPr><w:tblBorders><w:insideH w:val=\"single\" w:sz=\"4\"/></w:tblBorders></w:tblPr></w:style>\
                     <w:style w:type=\"table\" w:styleId=\"{id}\"><w:basedOn w:val=\"Base{id}\"/></w:style></w:styles>"
                ),
            );
        }
    };
    let plain = doctor_parts(&d, |parts| strip(parts, None));
    assert_eq!(plain.table_unruled.len(), 1, "no border anywhere: unruled");
    assert_eq!(round_trip(&plain).table_unruled.len(), 1, "and written without rules");
    let styled = doctor_parts(&d, |parts| strip(parts, Some("MyGrid")));
    assert!(styled.table_unruled.is_empty(), "ruled through its style's base");
}

#[test]
fn a_cells_empty_lines_survive() {
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 1, 1);
    let cell = |text: &str| Paragraph {
        style: ParaStyle { table_cell: Some(TableCell { table, row: 0, col: 0 }), ..Default::default() },
        runs: if text.is_empty() { vec![] } else { vec![Run::plain(text)] },
    };
    d.paragraphs.splice(0..1, [cell("Comments:"), cell(""), cell(""), cell("")]);
    let rt = round_trip(&d);
    let lines: Vec<String> = rt.paragraphs.iter().filter(|p| p.style.table_cell.is_some()).map(|p| p.text()).collect();
    assert_eq!(lines, ["Comments:", "", "", ""]);
}

/// The empty paragraph Word keeps between two tables is a line of the
/// document, drawn as one: dropped, the tables met and drew as one
/// (the questionnaire, #1211). Only the separator Letters itself writes
/// between adjacent tables is dropped on reading, so a round trip of
/// either keeps the document (`tables_keep_their_place`).
#[test]
fn a_files_empty_line_between_tables_survives() {
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 1, 1);
    d.insert_table_at(1, 1, 1);
    for p in d.paragraphs.iter_mut().filter(|p| p.style.table_cell.is_some()) {
        p.runs = vec![Run::plain("cell")];
    }
    // As Word writes it: a plain empty paragraph between the tables.
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        assert!(body.contains("LettersTableSeparator"), "adjacent tables get the separator: {body}");
        *body = body.replace("<w:pStyle w:val=\"LettersTableSeparator\"/>", "");
    });
    let shape: Vec<String> = rt.paragraphs.iter().map(|p| match p.style.table_cell {
        Some(tc) => format!("T{}", tc.table),
        None => p.text(),
    }).collect();
    assert_eq!(shape, ["T0", "", "T1", "after"]);
}

/// A table's row heights survive a save and reopen (`w:trHeight`), a
/// minimum as `atLeast` and a fixed height as `exact`.
#[test]
fn table_row_heights_survive() {
    use letters_core::RowHeight;
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 3, 2);
    let want = vec![Some(RowHeight { pt: 40.0, exact: false }), None, Some(RowHeight { pt: 18.5, exact: true })];
    d.table_rows.insert(table, want.clone());
    let rt = round_trip(&d);
    let got: Vec<_> = rt.table_rows.values().cloned().collect();
    assert_eq!(got, [want], "{:?}", rt.table_rows);
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 2, 2);
    assert!(round_trip(&d).table_rows.is_empty());
}

/// A table's shaded cells survive a save and reopen (`w:shd w:fill`).
#[test]
fn table_cell_fills_survive() {
    use letters_core::CellFill;
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 2, 2);
    let want = vec![CellFill { row: 0, col: 0, color: "D9D9D9".into() }, CellFill { row: 1, col: 1, color: "DEEAF6".into() }];
    d.table_fills.insert(table, want.clone());
    let rt = round_trip(&d);
    assert_eq!(rt.table_fills.values().cloned().collect::<Vec<_>>(), [want]);
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 1, 1);
    assert!(round_trip(&d).table_fills.is_empty());
}

/// A heading's run set back to regular weight stays regular through a
/// save and reopen (`<w:b w:val="0"/>`).
#[test]
fn a_regular_run_in_a_heading_survives() {
    use letters_core::RunStyle;
    let mut d = Document::from_plain_text("Towing Operations");
    d.paragraphs[0].style.heading = Some(3);
    d.paragraphs[0].runs[0].style = RunStyle { regular: true, ..Default::default() };
    let rt = round_trip(&d);
    assert!(rt.paragraphs[0].runs[0].style.regular);
    assert!(!rt.paragraphs[0].runs[0].style.bold);
}

/// A row kept on one page stays so through a save and reopen
/// (`w:cantSplit`); the others do not become so.
#[test]
fn a_row_kept_whole_survives() {
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 3, 1);
    d.table_whole_rows.entry(table).or_default().insert(1);
    let rt = round_trip(&d);
    let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
    assert_eq!(rt.table_whole_rows.get(&id).map(|r| r.iter().copied().collect::<Vec<_>>()), Some(vec![1]));
}

/// A heading row (`w:tblHeader`) is read as a row kept whole: Writer
/// never splits one across pages (measured: a six-line heading row at a
/// page's foot moves whole to the next page, where the same row without
/// `w:tblHeader` splits).
#[test]
fn a_heading_row_is_kept_whole() {
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 3, 1);
    let rt = doctor_document_xml(&d, |xml| xml.replacen("<w:tr>", "<w:tr><w:trPr><w:tblHeader/></w:trPr>", 1));
    let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
    assert_eq!(rt.table_whole_rows.get(&id).map(|r| r.iter().copied().collect::<Vec<_>>()), Some(vec![0]));
}

/// A table sized as a share of the text's width stays so through a save
/// and reopen (`w:tblW w:type="pct"`); one with the file's widths does not
/// become one.
#[test]
fn a_table_sized_to_the_text_survives() {
    let mut d = Document::from_plain_text("after");
    let shared = d.insert_table_at(0, 1, 2);
    d.table_columns.insert(shared, vec![200.0, 300.0]);
    d.table_relative.insert(shared);
    let rt = round_trip(&d);
    let table = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
    assert_eq!(rt.table_relative.iter().copied().collect::<Vec<_>>(), [table]);
    d.table_relative.clear();
    assert!(round_trip(&d).table_relative.is_empty());
}

/// Capitals survive a save and reopen with the text in its own case, and
/// a paragraph style's `w:caps` puts its runs in capitals: the crime
/// supervisor form's title, typed "Crime SUPERVISOR STANDARD and".
#[test]
fn capitals_survive_and_come_from_a_style() {
    use letters_core::RunStyle;
    let mut d = Document::from_plain_text("Crime supervisor standard\nplain");
    d.paragraphs[0].runs[0].style = RunStyle { caps: true, ..Default::default() };
    let rt = round_trip(&d);
    assert!(rt.paragraphs[0].runs[0].style.caps);
    assert_eq!(rt.paragraphs[0].text(), "Crime supervisor standard", "the text keeps its case");
    assert!(!rt.paragraphs[1].runs[0].style.caps);
    let styled = doctor_parts(&Document::from_plain_text("Crime supervisor standard"), |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        *body = body.replacen("<w:p>", "<w:p><w:pPr><w:pStyle w:val=\"annexatitle\"/></w:pPr>", 1);
        let styles = parts.get_mut("word/styles.xml").unwrap();
        *styles = styles.replace(
            "</w:styles>",
            "<w:style w:type=\"paragraph\" w:styleId=\"annexatitle\"><w:name w:val=\"annex a title\"/><w:rPr><w:caps/></w:rPr></w:style></w:styles>",
        );
    });
    assert!(styled.paragraphs[0].runs[0].style.caps, "{:?}", styled.paragraphs[0].runs);
}

/// A floating header or footer line survives a save and reopen as a text
/// box set against the page, ahead of the line whose paragraph holds it.
#[test]
fn floating_header_and_footer_lines_survive() {
    use letters_core::{Alignment, HfFloat, HfLine, RunStyle};
    let mut d = Document::from_plain_text("body");
    d.header = Some("OFFICIAL\nThe Crime Contract 2025".into());
    d.header_lines = vec![HfLine { alignment: Alignment::Center, look: RunStyle { font_size_hp: Some(24), ..Default::default() }, float: Some(HfFloat::Top(15.0)), ..Default::default() }, HfLine::default()];
    d.footer = Some("OFFICIAL\n{page}".into());
    d.footer_lines = vec![HfLine { alignment: Alignment::Right, float: Some(HfFloat::Bottom(10.0)), ..Default::default() }, HfLine::default()];
    let rt = round_trip(&d);
    assert_eq!((rt.header.as_deref(), rt.footer.as_deref()), (d.header.as_deref(), d.footer.as_deref()));
    assert_eq!((&rt.header_lines, &rt.footer_lines), (&d.header_lines, &d.footer_lines));
}

/// A footer paragraph's border gaps pad it above and below, an invisible
/// (`none`) border's too, as Writer keeps them: the questionnaire's Normal
/// style pads every side 31pt that way, and its one empty footer line
/// stands 62pt taller, moving the page's text up with it.
#[test]
fn a_footer_paragraphs_border_gaps_pad_it() {
    let mut d = Document::from_plain_text("body");
    d.footer = Some("foot".into());
    let rt = doctor_parts(&d, |parts| {
        let name = parts.keys().find(|k| k.starts_with("word/footer")).expect("a footer part").clone();
        let part = parts.get_mut(&name).unwrap();
        let at = part.find("<w:p>").map(|i| i + "<w:p>".len()).or_else(|| part.find("<w:p ").map(|i| i + part[i..].find('>').unwrap() + 1)).expect("a paragraph");
        let pbdr = "<w:pBdr><w:top w:val=\"none\" w:sz=\"96\" w:space=\"31\" w:color=\"FFFFFF\"/><w:bottom w:val=\"single\" w:sz=\"8\" w:space=\"4\" w:color=\"000000\"/></w:pBdr>";
        if part[at..].starts_with("<w:pPr>") {
            part.insert_str(at + "<w:pPr>".len(), pbdr);
        } else {
            part.insert_str(at, &format!("<w:pPr>{pbdr}</w:pPr>"));
        }
    });
    let line = rt.footer_lines.first().expect("the footer's line");
    assert_eq!((line.pad_above_pt, line.pad_below_pt), (31.0, 5.0), "the invisible gap, and the drawn one with its width");
}

/// An inline picture's room beside it survives a save and reopen: a .docx
/// says it by leaving `distL`/`distR` out, which Writer takes as its own
/// 3.19mm a side; a value the file gives takes no room, as Writer and
/// Word draw it.
#[test]
fn an_inline_pictures_room_beside_it_survives() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("p.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0\x90wS\xde\0\0\0\x0cIDATx\x9cc\xf8\xcf\xc0\0\0\x03\x01\x01\0\xc9\xfe\x92\xef\0\0\0\0IEND\xaeB`\x82").unwrap();
    let mut d = Document::from_plain_text("caption");
    d.paragraphs[0].runs.push(Run {
        text: String::new(),
        style: RunStyle { image: Some(png.to_string_lossy().into()), image_extent_emu: Some((914_400, 914_400)), image_sides_emu: Some((114_840, 114_840)), ..Default::default() },
    });
    let sides = |d: &Document| d.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.style.image.is_some()).and_then(|r| r.style.image_sides_emu);
    assert_eq!(sides(&round_trip(&d)), Some((114_840, 114_840)));
    // Given in the file, whatever its value: none.
    let given = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        *body = body.replacen("<wp:inline", "<wp:inline distL=\"114300\" distR=\"114300\"", 1);
    });
    assert_eq!(sides(&given), None);
}

/// The first page's own header and footer survive a save and reopen as
/// Word's "different first page" (`w:titlePg` and the `first` parts),
/// and the running ones stay the others'.
#[test]
fn the_first_pages_own_header_and_footer_survive() {
    use letters_core::{Alignment, HfLine, HfStory};
    let mut d = Document::from_plain_text("body");
    d.header = Some("Running title".into());
    d.footer = Some("{page}".into());
    d.first_header = Some(HfStory {
        text: "Letterhead\nDepartment".into(),
        alignment: Alignment::Center,
        lines: vec![
            HfLine { alignment: Alignment::Center, space_after_pt: 12.0, line_height: Some(LineHeight::Exact(10.0)), ..Default::default() },
            HfLine { alignment: Alignment::Right, space_before_pt: 3.0, line_height: Some(LineHeight::Minimum(14.0)), ..Default::default() },
        ],
        ..Default::default()
    });
    d.first_footer = Some(HfStory { text: "Page {page}".into(), ..Default::default() });
    let rt = round_trip(&d);
    assert_eq!((rt.header.as_deref(), rt.footer.as_deref()), (Some("Running title"), Some("{page}")));
    let first = rt.first_header.as_ref().expect("the first page's own header");
    assert_eq!((first.text.as_str(), first.alignment), ("Letterhead\nDepartment", Alignment::Center));
    assert_eq!(first.lines.iter().map(|l| (l.alignment, l.space_before_pt, l.space_after_pt, l.line_height)).collect::<Vec<_>>(), d.first_header.as_ref().unwrap().lines.iter().map(|l| (l.alignment, l.space_before_pt, l.space_after_pt, l.line_height)).collect::<Vec<_>>());
    assert_eq!(rt.first_footer.as_ref().map(|s| s.text.as_str()), Some("Page {page}"));
    // Without a first page of its own, a document reads none.
    assert_eq!(round_trip(&Document::from_plain_text("body")).first_header, None);
}

/// A cell's bare sides survive a save and reopen (`w:tcBorders` with
/// `w:val="none"` over the table's grid).
#[test]
fn a_cells_bare_sides_survive() {
    use letters_core::{CellEdges, OpenEdges};
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 2, 2);
    let want = vec![
        CellEdges { row: 0, col: 0, open: OpenEdges { right: true, ..Default::default() } },
        CellEdges { row: 0, col: 1, open: OpenEdges { left: true, top: true, ..Default::default() } },
    ];
    d.table_open_edges.insert(table, want.clone());
    let rt = round_trip(&d);
    assert_eq!(rt.table_open_edges.values().cloned().collect::<Vec<_>>(), [want]);
    assert!(rt.table_unruled.is_empty(), "the rest of the grid stays ruled");
}

/// A cell set lower than the top survives a save and reopen (`w:vAlign`).
#[test]
fn a_cells_vertical_alignment_survives() {
    use letters_core::{CellVAlign, VAlign};
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 1, 2);
    let want = vec![CellVAlign { row: 0, col: 0, align: VAlign::Center }, CellVAlign { row: 0, col: 1, align: VAlign::Bottom }];
    d.table_valign.insert(table, want.clone());
    assert_eq!(round_trip(&d).table_valign.values().cloned().collect::<Vec<_>>(), [want]);
}

/// A table's cell padding survives a save and reopen (`w:tblCellMar`); a
/// table with Word's default has none of its own.
#[test]
fn table_cell_padding_survives() {
    use letters_core::CellPadding;
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 2, 2);
    let want = CellPadding { top_pt: 4.0, bottom_pt: 4.0, left_pt: 4.0, right_pt: 4.0 };
    d.table_padding.insert(table, want);
    let rt = round_trip(&d);
    assert_eq!(rt.table_padding.values().copied().collect::<Vec<_>>(), [want]);
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 1, 1);
    assert!(round_trip(&d).table_padding.is_empty());
}

/// A table stays where the document has it, between the paragraphs around
/// it, and the paragraph OOXML needs after a table that meets another or
/// ends the document comes and goes with the save: a round trip neither
/// adds nor loses an empty line (the reader once put every table at the
/// end, which moved a form's tables pages away from their labels).
#[test]
fn tables_keep_their_place() {
    fn text(s: &str) -> Paragraph {
        Paragraph { style: ParaStyle::default(), runs: vec![Run { text: s.into(), style: RunStyle::default() }] }
    }
    fn cell(table: u32, s: &str) -> Paragraph {
        let mut p = if s.is_empty() { Paragraph { style: ParaStyle::default(), runs: vec![] } } else { text(s) };
        p.style.table_cell = Some(TableCell { table, row: 0, col: 0 });
        p
    }
    let empty = || Paragraph { style: ParaStyle::default(), runs: vec![] };
    let shape = |d: &Document| -> Vec<String> {
        d.paragraphs.iter().map(|p| match p.style.table_cell {
            Some(tc) => format!("T{}:{}", tc.table, p.text()),
            None => p.text(),
        }).collect()
    };
    let cases: Vec<Vec<Paragraph>> = vec![
        vec![text("before"), cell(0, "a"), text("after")],
        vec![cell(0, "a"), text("after")],
        vec![text("before"), cell(0, "a")],
        vec![cell(0, "a"), cell(1, "b")],
        vec![cell(0, "a"), empty(), cell(1, "b")],
        vec![text("x"), cell(0, "a"), empty(), text("y")],
        vec![cell(0, "a"), empty(), empty()],
        vec![text("before"), cell(0, "a"), text("mid"), cell(1, "b"), text("after")],
    ];
    for paragraphs in cases {
        let doc = Document { paragraphs, ..Document::new() };
        let once = round_trip(&doc);
        assert_eq!(shape(&once), shape(&doc));
        assert_eq!(shape(&round_trip(&once)), shape(&doc));
    }
}

/// The text of `name` in the package at `path`.
fn package_part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut xml).unwrap();
    xml
}

/// A report with a landscape, two-column appendix carrying its own header
/// and a closing table section with no footer (LET-005S): each section's
/// page, header and footer survive a save and reopen, the breaks land
/// where they were (one before a table), and the file has one `w:sectPr`
/// per section.
#[test]
fn multi_section_docx_round_trips_geometry_and_headers() {
    let mut d = Document::from_plain_text("Introduction\nAppendix\na1\nb1");
    for (k, col) in [(2usize, 0u32), (3, 1)] {
        d.paragraphs[k].style.table_cell = Some(TableCell { table: 0, row: 0, col });
    }
    d.header = Some("Report".into());
    let portrait = PageGeometry { margin_top_pt: 72.0, margin_bottom_pt: 72.0, margin_left_pt: 72.0, margin_right_pt: 72.0, ..Default::default() };
    let landscape = PageGeometry {
        width_pt: 842.0,
        height_pt: 595.0,
        margin_top_pt: 36.0,
        margin_bottom_pt: 36.0,
        margin_left_pt: 36.0,
        margin_right_pt: 36.0,
        columns: 2,
        column_gap_pt: 24.0,
        header_pt: Some(20.0),
        footer_pt: Some(22.0),
        border: None,
    };
    d.sections = vec![
        DocSection { id: 0, range: ParagraphRange::new(0, 1), geometry: portrait, header: None, footer: None },
        DocSection { id: 1, range: ParagraphRange::new(1, 2), geometry: landscape, header: Some("Appendix {page}".into()), footer: None },
        DocSection { id: 2, range: ParagraphRange::new(2, 4), geometry: portrait, header: None, footer: Some(String::new()) },
    ];
    d.next_section_id = 3;
    d.page = Some(portrait);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sections.docx");
    docx::write(&d, &path).expect("write docx");
    let xml = package_part(&path, "word/document.xml");
    assert_eq!(xml.matches("<w:sectPr").count(), 3, "{xml}");
    assert_eq!(xml.matches("w:orient=\"landscape\"").count(), 1, "{xml}");

    let rt = docx::read(path.to_str().unwrap()).expect("read docx");
    assert_eq!(rt.paragraphs.len(), 4);
    assert_eq!(rt.sections.len(), 3);
    for (read, written) in rt.sections.iter().zip(&d.sections) {
        assert_eq!(read.range, written.range);
        assert!(read.geometry.approx_eq(&written.geometry), "{:?} vs {:?}", read.geometry, written.geometry);
        let distances = |g: &PageGeometry| (g.header_pt.unwrap_or(36.0), g.footer_pt.unwrap_or(36.0));
        assert_eq!(distances(&read.geometry), distances(&written.geometry));
        assert_eq!(read.header, written.header);
        assert_eq!(read.footer, written.footer);
    }
    assert_eq!(rt.header.as_deref(), Some("Report"));
    assert_eq!(rt.footer, None);
    assert_eq!(rt.page, Some(rt.sections[0].geometry));
    assert_eq!(rt.next_section_id, 3);
    assert!(rt.paragraphs[2].style.table_cell.is_some() && rt.paragraphs[1].style.table_cell.is_none());
}

/// A document of one section writes one `w:sectPr` and no section parts,
/// and reads back as one section.
#[test]
fn single_section_docx_writes_one_section_properties() {
    let mut d = Document::from_plain_text("one section\nstill one");
    d.page = Some(PageGeometry { width_pt: 612.0, height_pt: 792.0, ..Default::default() });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("single.docx");
    docx::write(&d, &path).expect("write docx");
    assert_eq!(package_part(&path, "word/document.xml").matches("<w:sectPr").count(), 1);
    let zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert!(!zip.file_names().any(|n| n.unwrap().contains("lettersSection")));
    let rt = docx::read(path.to_str().unwrap()).expect("read docx");
    assert!(rt.sections.is_empty());
    assert!(rt.page.unwrap().approx_eq(&d.page.unwrap()));
}

/// Word's own section breaks: a `w:sectPr` in a paragraph's properties
/// ends its section there, the body's last ends the document. A section
/// that leaves out its margins keeps the one before's; one without a
/// header reference continues the one before's, and the first without
/// one has none.
#[test]
fn word_section_breaks_read_as_sections() {
    let mut d = Document::from_plain_text("one\ntwo\nthree");
    d.header = Some("Second part".into());
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let start = body.find("<w:sectPr").expect("the body's section properties");
        let end = body[start..].find("</w:sectPr>").map(|e| start + e + "</w:sectPr>".len()).expect("closed");
        let sect = body[start..end].to_string();
        let r_start = sect.find("<w:headerReference").expect("rdocx's header reference");
        let r_end = r_start + sect[r_start..].find("/>").unwrap() + 2;
        let reference = sect[r_start..r_end].to_string();
        let letter = r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#;
        body.replace_range(start..end, letter);
        let a4 = r#"<w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720"/></w:sectPr></w:pPr>"#;
        let landscape = format!(r#"<w:pPr><w:sectPr>{reference}<w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/></w:sectPr></w:pPr>"#);
        for (text, props) in [("one", a4.to_string()), ("two", landscape)] {
            let marker = format!("<w:p><w:r><w:t>{text}</w:t>");
            assert!(body.contains(&marker), "{body}");
            *body = body.replacen(&marker, &format!("<w:p>{props}<w:r><w:t>{text}</w:t>"), 1);
        }
    });
    assert_eq!(rt.to_plain_text(), "one\ntwo\nthree");
    assert_eq!(rt.sections.len(), 3);
    let ranges: Vec<(usize, usize)> = rt.sections.iter().map(|s| (s.range.start, s.range.end)).collect();
    assert_eq!(ranges, [(0, 1), (1, 2), (2, 3)]);
    let a4 = PageGeometry { width_pt: 595.3, height_pt: 841.9, margin_top_pt: 36.0, margin_bottom_pt: 36.0, margin_left_pt: 36.0, margin_right_pt: 36.0, ..Default::default() };
    let a4_landscape = PageGeometry { width_pt: 841.9, height_pt: 595.3, ..a4 };
    let letter = PageGeometry { width_pt: 612.0, height_pt: 792.0, ..Default::default() };
    for (read, want) in rt.sections.iter().zip([a4, a4_landscape, letter]) {
        assert!(read.geometry.approx_eq(&want), "{:?} vs {want:?}", read.geometry);
    }
    assert_eq!(rt.page, Some(rt.sections[0].geometry));
    assert_eq!(rt.header, None, "the first section has no header reference");
    assert_eq!(rt.sections[0].header, None);
    assert_eq!(rt.sections[1].header.as_deref(), Some("Second part"));
    assert_eq!(rt.sections[2].header, None, "continues the second section's");
    assert_eq!(rt.next_section_id, 3);
    assert_eq!(letters_core::sections::header(&rt, 2), (Some("Second part"), false));
}

/// A line break within a paragraph is written as Word's `w:br` and read
/// back as one: a raw newline was whitespace, and the two lines ran
/// together.
#[test]
fn a_line_break_within_a_paragraph_survives() {
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![Run::plain("Statistical Release\nOctober 2020")];
    assert_eq!(round_trip(&d).paragraphs[0].runs, d.paragraphs[0].runs);
}

/// A list item's own indent, a zero one too, is kept over its level's: the
/// business appointment rules' bullets set `w:left="0" w:hanging="2"` over
/// a 0.5in level, and stand at the margin. Read as no indent at all, they
/// were drawn at the level's.
#[test]
fn a_list_items_own_zero_indent_is_kept() {
    let mut d = Document::from_plain_text("item");
    d.paragraphs[0].style.list = ListKind::Bullet;
    let doctored = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let at = body.find("</w:numPr>").expect("a list item") + "</w:numPr>".len();
        let tail = &body[at..];
        // Our writer's own indent, replaced by the file's.
        let end = tail.find("/>").map(|e| at + e + 2).filter(|_| tail.starts_with("<w:ind"));
        match end {
            Some(e) => body.replace_range(at..e, "<w:ind w:left=\"0\" w:hanging=\"2\"/>"),
            None => body.insert_str(at, "<w:ind w:left=\"0\" w:hanging=\"2\"/>"),
        }
    });
    let st = &doctored.paragraphs[0].style;
    assert_eq!(st.list, ListKind::Bullet);
    assert!((st.left_indent_pt + letters_core::lists::text_indent_pt(0)).abs() < 0.01, "at the margin: {}", st.left_indent_pt);
    assert_eq!(st.list_hanging_pt, Some(0.1));
    // And as we write it back.
    let rt = round_trip(&doctored);
    assert_eq!((rt.paragraphs[0].style.left_indent_pt, rt.paragraphs[0].style.list_hanging_pt), (st.left_indent_pt, st.list_hanging_pt));
}

/// A table's indent survives a save and reopen, and `w:tblInd` reads by
/// the file's rules: under Word 2013's (compatibility mode 15) it is the
/// table's edge, its text a left padding further right; under older ones
/// it is the text's.
#[test]
fn a_tables_indent_reads_by_the_files_word_rules() {
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 1, 1);
    d.table_indent.insert(table, 30.0);
    let rt = round_trip(&d);
    assert_eq!(rt.table_indent.values().copied().collect::<Vec<_>>(), [30.0]);

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("t.docx");
    docx::write(&d, &src).expect("write docx");
    let with_mode = |mode: &str| -> Vec<f64> {
        let out = dir.path().join(format!("mode{mode}.docx"));
        let mut zin = zip::ZipArchive::new(std::fs::File::open(&src).unwrap()).unwrap();
        let mut zout = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().unwrap().into_owned();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
            if name == "word/document.xml" {
                let xml = String::from_utf8(buf).unwrap();
                let at = xml.find("<w:tblInd ").expect("the writer's w:tblInd");
                let end = at + xml[at..].find("/>").unwrap() + 2;
                buf = format!("{}<w:tblInd w:w=\"400\" w:type=\"dxa\"/>{}", &xml[..at], &xml[end..]).into_bytes();
            }
            if name == "word/settings.xml" {
                let xml = String::from_utf8(buf).unwrap();
                assert!(xml.contains("w:val=\"15\""), "the writer's mode: {xml}");
                buf = xml.replace("w:val=\"15\"", &format!("w:val=\"{mode}\"")).into_bytes();
            }
            zout.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zout, &buf).unwrap();
        }
        zout.finish().unwrap();
        docx::read(out.to_str().unwrap()).expect("read").table_indent.values().copied().collect()
    };
    assert_eq!(with_mode("15"), [20.0 + 5.4], "the edge 20pt in, the text its padding further");
    assert_eq!(with_mode("14"), [20.0], "the text 20pt in");
}

/// A blank paragraph is a line of its mark's font (`w:pPr/w:rPr`), as Word
/// and Writer size it: the screening checklist opens with two blank 22pt
/// lines above its title, which drawn at the body size stood it 32px
/// high. A blank paragraph in the body font has nothing to keep.
#[test]
fn a_blank_paragraph_keeps_its_marks_size() {
    let d = Document::from_plain_text("title\nbody");
    let doctored = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let at = body.find("<w:body>").expect("a body") + "<w:body>".len();
        body.insert_str(at, "<w:p><w:pPr><w:rPr><w:sz w:val=\"44\"/></w:rPr></w:pPr></w:p><w:p/>");
    });
    let blank = &doctored.paragraphs[0];
    assert_eq!(blank.text(), "");
    assert_eq!(blank.runs.iter().map(|r| r.style.font_size_hp).collect::<Vec<_>>(), [Some(44)], "a 22pt blank line");
    assert!(doctored.paragraphs[1].runs.is_empty(), "a blank line in the body font: {:?}", doctored.paragraphs[1].runs);
    assert_eq!(doctored.paragraphs[2].text(), "title");
}

/// A list whose definition only links a numbering style (`w:numStyleLink`)
/// and gives its levels in the instance's own `w:lvlOverride` is numbered
/// as those levels say. Its items were read as bullets: the Ofsted
/// minutes' "6.1." item took a bullet's room, and moved its next page.
#[test]
fn a_list_numbered_by_its_level_overrides_is_numbered() {
    let mut d = Document::from_plain_text("first\nafter");
    d.paragraphs[0].style.list = ListKind::Numbered;
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let at = body.find("<w:numId w:val=\"").expect("fixture shape changed");
        let end = at + body[at..].find("/>").unwrap() + 2;
        body.replace_range(at..end, "<w:numId w:val=\"90\"/>");
        let numbering = parts.get_mut("word/numbering.xml").unwrap();
        let abs_at = numbering.find("<w:abstractNum ").expect("fixture shape changed");
        numbering.insert_str(
            abs_at,
            "<w:abstractNum w:abstractNumId=\"90\"><w:multiLevelType w:val=\"multilevel\"/><w:numStyleLink w:val=\"OutlineNumbered\"/></w:abstractNum>",
        );
        *numbering = numbering.replace(
            "</w:numbering>",
            "<w:num w:numId=\"90\"><w:abstractNumId w:val=\"90\"/><w:lvlOverride w:ilvl=\"0\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/>\
             <w:lvlText w:val=\"6.%1.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"720\" w:hanging=\"720\"/></w:pPr></w:lvl></w:lvlOverride></w:num></w:numbering>",
        );
    });
    assert_eq!(rt.paragraphs[0].style.list, ListKind::Numbered);
    let label = rt.paragraphs[0].style.list_label.as_ref().expect("its level text");
    assert_eq!((label.prefix.as_str(), label.suffix.as_str()), ("6.", "."));
}

/// A list item numbered by its own `w:numPr` takes its level's indent
/// over its paragraph style's, as Word and Writer resolve it: a "List
/// Paragraph" (0.5in) item at a level of 0.25in stands at 0.25in. One
/// whose paragraph gives its own indent keeps that.
#[test]
fn a_directly_numbered_item_takes_its_levels_indent_over_its_styles() {
    let mut d = Document::from_plain_text("first\nsecond\nafter");
    d.paragraphs[0].style.list = ListKind::Numbered;
    d.paragraphs[1].style.list = ListKind::Numbered;
    let rt = doctor_parts(&d, |parts| {
        let numbering = parts.get_mut("word/numbering.xml").unwrap();
        let abs_at = numbering.find("<w:abstractNum ").expect("fixture shape changed");
        numbering.insert_str(
            abs_at,
            "<w:abstractNum w:abstractNumId=\"91\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/>\
             <w:pPr><w:ind w:left=\"360\" w:hanging=\"360\"/></w:pPr></w:lvl></w:abstractNum>",
        );
        *numbering = numbering.replace("</w:numbering>", "<w:num w:numId=\"91\"><w:abstractNumId w:val=\"91\"/></w:num></w:numbering>");
        let st = parts.get_mut("word/styles.xml").unwrap();
        *st = st.replace(
            "</w:styles>",
            "<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:pPr><w:ind w:left=\"720\"/></w:pPr></w:style></w:styles>",
        );
        let body = parts.get_mut("word/document.xml").unwrap();
        // The writer's own indents off, so the style and level decide.
        while let Some(at) = body.find("<w:ind ") {
            let end = at + body[at..].find("/>").unwrap() + 2;
            body.replace_range(at..end, "");
        }
        let mut out = String::new();
        let mut rest = body.as_str();
        let mut n = 0;
        while let Some(at) = rest.find("<w:numPr>") {
            let end = at + rest[at..].find("</w:numPr>").unwrap() + "</w:numPr>".len();
            out.push_str(&rest[..at]);
            let own = if n == 1 { "<w:ind w:left=\"1080\" w:hanging=\"360\"/>" } else { "" };
            out.push_str(&format!("<w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"91\"/></w:numPr>{own}"));
            rest = &rest[end..];
            n += 1;
        }
        out.push_str(rest);
        assert_eq!(n, 2, "fixture shape changed: {body}");
        *body = out;
    });
    let text = letters_core::lists::text_indent_pt(0);
    let at = |i: usize| rt.paragraphs[i].style.left_indent_pt + text;
    assert!((at(0) - 18.0).abs() < 1e-6, "its level's 0.25in, not the style's 0.5in: {}", at(0));
    assert!((at(1) - 54.0).abs() < 1e-6, "its own 0.75in: {}", at(1));
}

/// A table's rules keep their colour and width: Word's "Table Grid Light"
/// style draws its grid light grey (BFBFBF), which was drawn black, and a
/// table whose own borders say a colour and width keeps them through a
/// save.
#[test]
fn a_table_keeps_its_rule_colour() {
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 2, 2);
    d.table_rule_colors.insert(table, "BFBFBF".into());
    d.table_rule_widths.insert(table, 0.75);
    let rt = round_trip(&d);
    let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
    assert_eq!(rt.table_rule_colors.get(&id).map(String::as_str), Some("BFBFBF"), "its own borders' colour");
    assert_eq!(rt.table_rule_widths.get(&id).copied(), Some(0.75), "and width");
    d.table_rule_widths.clear();

    d.table_rule_colors.clear();
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let start = body.find("<w:tblBorders>").expect("fixture shape changed");
        let end = body.find("</w:tblBorders>").unwrap() + "</w:tblBorders>".len();
        body.replace_range(start..end, "");
        *body = body.replacen("<w:tblPr>", "<w:tblPr><w:tblStyle w:val=\"TableGridLight\"/>", 1);
        let st = parts.get_mut("word/styles.xml").unwrap();
        *st = st.replace(
            "</w:styles>",
            "<w:style w:type=\"table\" w:styleId=\"TableGridLight\"><w:name w:val=\"Grid Table Light\"/><w:tblPr><w:tblBorders>\
             <w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\" w:themeColor=\"background1\" w:themeShade=\"BF\"/>\
             <w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/></w:tblBorders></w:tblPr></w:style></w:styles>",
        );
    });
    let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
    assert!(!rt.table_unruled.contains(&id), "the style rules it");
    assert_eq!(rt.table_rule_colors.get(&id).map(String::as_str), Some("BFBFBF"), "in its style's colour");
}

/// A table ruled in double lines (`w:val="double"`) keeps them through a
/// save: the crime supervisor form's tables were drawn single.
#[test]
fn a_table_keeps_its_double_rules() {
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 2, 2);
    d.table_rule_double.insert(table);
    let rt = round_trip(&d);
    let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
    assert!(rt.table_rule_double.contains(&id), "{:?}", rt.table_rule_double);
    d.table_rule_double.clear();
    assert!(round_trip(&d).table_rule_double.is_empty(), "single rules stay single");
}

/// A cell's own borders (`w:tcBorders`) win over its table's: the
/// questionnaire's respondent table rules itself white and each of its
/// cells black, and Word and Writer draw it black, not white.
#[test]
fn a_cells_own_border_colour_wins_over_its_tables() {
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 2, 2);
    let rt = doctor_parts(&d, |parts| {
        let body = parts.get_mut("word/document.xml").unwrap();
        let start = body.find("<w:tblBorders>").expect("fixture shape changed");
        let end = body.find("</w:tblBorders>").unwrap() + "</w:tblBorders>".len();
        body.replace_range(
            start..end,
            "<w:tblBorders><w:top w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"FFFFFF\"/>\
             <w:insideH w:val=\"single\" w:sz=\"8\" w:space=\"0\" w:color=\"FFFFFF\"/></w:tblBorders>",
        );
        let cell = "<w:tcBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"000000\"/></w:tcBorders>";
        *body = match body.find("<w:tcPr>") {
            Some(at) => format!("{}{cell}{}", &body[..at + "<w:tcPr>".len()], &body[at + "<w:tcPr>".len()..]),
            None => body.replacen("<w:tc>", &format!("<w:tc><w:tcPr>{cell}</w:tcPr>"), 1),
        };
    });
    let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
    assert!(!rt.table_unruled.contains(&id), "ruled");
    assert_eq!(rt.table_rule_colors.get(&id), None, "black, as its cells say: {:?}", rt.table_rule_colors);
}
