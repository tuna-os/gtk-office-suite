// soffice_oracle.rs — LibreOffice-headless interop oracle.
//
// The parity strategy (PLAN.md): we do not port LibreOffice code or tests;
// we measure our output against LibreOffice's *behavior*. Every .docx this
// engine writes must (a) open in LibreOffice Writer without error and
// (b) survive soffice's own conversion with identical extracted text.
//
// Locally these tests skip when soffice is absent. In CI the oracle job
// sets REQUIRE_SOFFICE=1, turning a missing oracle into a failure so the
// check can never silently vanish (the lesson of the old GUI workflow).

use std::process::Command;

use letters_core::docx;
use letters_core::model::*;

fn soffice() -> Option<&'static str> {
    for cand in ["soffice", "libreoffice"] {
        if Command::new(cand).arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(match cand { "soffice" => "soffice", _ => "libreoffice" });
        }
    }
    None
}

fn require_or_skip() -> Option<&'static str> {
    match soffice() {
        Some(bin) => Some(bin),
        None => {
            if std::env::var("REQUIRE_SOFFICE").is_ok() {
                panic!("REQUIRE_SOFFICE set but no soffice binary found");
            }
            eprintln!("skipping: soffice not installed");
            None
        }
    }
}

/// Convert a file with soffice; returns the output file's contents.
fn soffice_convert(bin: &str, input: &std::path::Path, to: &str) -> Result<String, String> {
    let dir = input.parent().unwrap();
    // Isolated profile dir: parallel soffice instances clash otherwise.
    let profile = dir.join("lo-profile");
    let out = Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", to, "--outdir"])
        .arg(dir)
        .arg(input)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("soffice failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let converted = input.with_extension(to.split(':').next().unwrap());
    std::fs::read_to_string(&converted)
        .map_err(|e| format!("no converted output {}: {} (stdout: {})",
            converted.display(), e, String::from_utf8_lossy(&out.stdout)))
}

/// Normalize text for comparison: soffice txt export uses \r\n and may add
/// a trailing newline.
fn norm(s: &str) -> String {
    s.trim_start_matches('\u{feff}').replace("\r\n", "\n").trim_end_matches('\n').to_string()
}

/// The paragraph carrying `want`, with every paragraph in the panic.
///
/// A converter is free to pad a document with empty paragraphs, and
/// LibreOffice's do differ between builds, so `paragraphs[0]` is not
/// reliably the paragraph a fixture wrote — an empty pad would answer
/// every style question with its defaults and read as a lost property.
/// Looking the paragraph up by its text asks about the one we authored,
/// and printing the lot makes a real loss diagnosable from a CI log.
fn para_with_text<'a>(doc: &'a Document, want: &str) -> &'a Paragraph {
    doc.paragraphs
        .iter()
        .find(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>().contains(want))
        .unwrap_or_else(|| {
            let seen: Vec<String> = doc
                .paragraphs
                .iter()
                .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect())
                .collect();
            panic!("no paragraph contains {want:?}; paragraphs: {seen:?}")
        })
}

/// The lookup has to skip a pad, which is the whole reason it exists.
#[test]
fn para_with_text_skips_an_empty_pad() {
    let mut d = Document::from_plain_text("");
    d.paragraphs.push(Paragraph {
        style: ParaStyle { left_indent_pt: 36.0, ..Default::default() },
        runs: vec![Run::plain("wanted")],
    });
    assert!(d.paragraphs[0].runs.iter().all(|r| r.text.is_empty()), "fixture needs a leading pad");
    assert!((para_with_text(&d, "wanted").style.left_indent_pt - 36.0).abs() < 0.01);
}

fn oracle_text_round_trip(doc: &Document) {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ours.docx");
    docx::write(doc, path.to_str().unwrap()).expect("write docx");

    let extracted = soffice_convert(bin, &path, "txt:Text (encoded):UTF8")
        .expect("LibreOffice could not open our .docx");
    assert_eq!(
        norm(&extracted),
        norm(&doc.to_plain_text()),
        "LibreOffice reads different text than we wrote"
    );
}

#[test]
fn oracle_reads_plain_paragraphs() {
    oracle_text_round_trip(&Document::from_plain_text(
        "first paragraph\nsecond paragraph\n\nfourth after blank",
    ));
}

#[test]
fn oracle_reads_styled_text() {
    let mut d = Document::from_plain_text("normal bold italic strike");
    d.apply_run_style(7, 11, &StylePatch::set_bold(true));
    d.apply_run_style(12, 18, &StylePatch::set_italic(true));
    d.apply_run_style(19, 25, &StylePatch::set_strikethrough(true));
    oracle_text_round_trip(&d);
}

#[test]
fn oracle_reads_headings() {
    let mut d = Document::from_plain_text("Document Title\nSection One\nbody text here");
    d.set_heading(0, Some(1));
    d.set_heading(1, Some(2));
    oracle_text_round_trip(&d);
}

#[test]
fn oracle_reads_unicode() {
    oracle_text_round_trip(&Document::from_plain_text("héllo — “fancy” 中文 emoji ✨"));
}

/// The reverse direction: a docx LibreOffice writes must open in our engine.
#[test]
fn we_read_soffice_output() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();

    // Have soffice author a .docx from markdown-ish plain text.
    let src = dir.path().join("lo-authored.txt");
    std::fs::write(&src, "alpha\nbeta\ngamma\n").unwrap();
    let _ = soffice_convert(bin, &src, "docx").ok();
    let docx_path = dir.path().join("lo-authored.docx");
    // convert-to docx returns binary; soffice_convert tried read_to_string and
    // may have failed — only require the file to exist.
    assert!(docx_path.exists(), "soffice did not produce a docx");

    let doc = docx::read(docx_path.to_str().unwrap()).expect("our engine failed on LO-authored docx");
    assert_eq!(norm(&doc.to_plain_text()), "alpha\nbeta\ngamma");
}

// ── ODT oracle (PARITY #20) ──────────────────────────────────────────

fn oracle_odt_text_round_trip(doc: &Document) {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ours.odt");
    letters_core::odt::write(doc, path.to_str().unwrap()).expect("write odt");

    let extracted = soffice_convert(bin, &path, "txt:Text (encoded):UTF8")
        .expect("LibreOffice could not open our .odt");
    assert_eq!(
        norm(&extracted),
        norm(&doc.to_plain_text()),
        "LibreOffice reads different text than we wrote (odt)"
    );
}

#[test]
fn odt_oracle_reads_plain_paragraphs() {
    oracle_odt_text_round_trip(&Document::from_plain_text(
        "first paragraph\nsecond paragraph\n\nfourth after blank",
    ));
}

#[test]
fn odt_oracle_reads_styled_text() {
    let mut d = Document::from_plain_text("normal bold italic strike");
    d.apply_run_style(7, 11, &StylePatch::set_bold(true));
    d.apply_run_style(12, 18, &StylePatch::set_italic(true));
    d.apply_run_style(19, 25, &StylePatch::set_strikethrough(true));
    oracle_odt_text_round_trip(&d);
}

#[test]
fn odt_oracle_reads_headings_and_alignment() {
    let mut d = Document::from_plain_text("Document Title\nSection One\nbody text here");
    d.set_heading(0, Some(1));
    d.set_heading(1, Some(2));
    d.paragraphs[2].style.alignment = Alignment::Center;
    oracle_odt_text_round_trip(&d);
}

#[test]
fn odt_oracle_reads_lists() {
    // soffice's text export renders list items as "    • item"; strip the
    // markers — what we assert is that the items and their order survive.
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("intro\napples\npears\noutro");
    d.paragraphs[1].style.list = ListKind::Bullet;
    d.paragraphs[2].style.list = ListKind::Bullet;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lists.odt");
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");
    let extracted = soffice_convert(bin, &path, "txt:Text (encoded):UTF8")
        .expect("LibreOffice could not open our .odt");
    let cleaned: String = norm(&extracted)
        .lines()
        .map(|l| l.trim_start().trim_start_matches("• ").trim_start())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(cleaned, "intro\napples\npears\noutro");
}

#[test]
fn odt_oracle_reads_unicode() {
    oracle_odt_text_round_trip(&Document::from_plain_text("héllo — “fancy” 中文 emoji ✨"));
}

/// The reverse: an .odt LibreOffice writes must open in our engine.
#[test]
fn we_read_soffice_odt_output() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();

    let src = dir.path().join("lo-authored.txt");
    std::fs::write(&src, "alpha\nbeta\ngamma\n").unwrap();
    let _ = soffice_convert(bin, &src, "odt").ok();
    let odt_path = dir.path().join("lo-authored.odt");
    assert!(odt_path.exists(), "soffice did not produce an odt");

    let doc = letters_core::odt::read(odt_path.to_str().unwrap())
        .expect("our engine failed on LO-authored odt");
    assert_eq!(norm(&doc.to_plain_text()), "alpha\nbeta\ngamma");
}

/// Style fidelity through LibreOffice: our odt → soffice converts to docx →
/// our docx reader sees the same runs (bold survives a full LO pass).
#[test]
fn odt_styles_survive_lo_conversion_to_docx() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("styled.odt");
    let mut d = Document::from_plain_text("plain bolded end");
    d.apply_run_style(6, 12, &StylePatch::set_bold(true));
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");

    let _ = soffice_convert(bin, &path, "docx").ok();
    let docx_path = dir.path().join("styled.docx");
    assert!(docx_path.exists(), "soffice did not convert odt to docx");
    let rt = docx::read(docx_path.to_str().unwrap()).expect("read converted docx");
    assert_eq!(norm(&rt.to_plain_text()), "plain bolded end");
    let bold_text: String = rt.paragraphs[0]
        .runs
        .iter()
        .filter(|r| r.style.bold)
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(bold_text.trim(), "bolded", "bold did not survive the LO pass");
}

/// Page geometry through LibreOffice: our odt → LO converts to docx →
/// our docx reader sees the same page size and margins.
#[test]
fn page_geometry_survives_lo_conversion() {
    use letters_core::model::PageGeometry;
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("geom.odt");
    let mut d = Document::from_plain_text("geometry probe");
    let want = PageGeometry {
        width_pt: 612.0, // US Letter — distinct from the A4 default
        height_pt: 792.0,
        margin_top_pt: 36.0,
        margin_bottom_pt: 54.0,
        margin_left_pt: 90.0,
        margin_right_pt: 45.0,
        columns: 1,
        column_gap_pt: 18.0,
        ..Default::default()
    };
    d.page = Some(want);
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");

    let _ = soffice_convert(bin, &path, "docx").ok();
    let docx_path = dir.path().join("geom.docx");
    assert!(docx_path.exists(), "soffice did not convert odt to docx");
    let rt = docx::read(docx_path.to_str().unwrap()).expect("read converted docx");
    let pg = rt.page.expect("LibreOffice dropped the page geometry");
    assert!(pg.approx_eq(&want), "geometry drifted through LO: {pg:?}");
}

// ── Oracle wave 2 (TDD): attribute fidelity through a full LO pass ────
//
// Each test writes our .odt, has LibreOffice convert it, and reads the
// result back through our own readers — asserting the *attribute*
// survives, not just the text. Written red-first; the writer/reader pair
// is fixed until each goes green.

/// Our odt → LO converts to docx → our docx reader. Returns the document.
fn through_lo_to_docx(doc: &Document, stem: &str) -> Option<Document> {
    let bin = require_or_skip()?;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{stem}.odt"));
    letters_core::odt::write(doc, path.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &path, "docx").ok();
    let docx_path = dir.path().join(format!("{stem}.docx"));
    assert!(docx_path.exists(), "soffice did not convert {stem}.odt to docx");
    Some(docx::read(docx_path.to_str().unwrap()).expect("read converted docx"))
}

#[test]
fn hyperlink_survives_lo_pass() {
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![
        Run::plain("visit "),
        Run {
            text: "gnome".into(),
            style: RunStyle { link: Some("https://gnome.org/".into()), ..Default::default() },
        },
        Run::plain(" today"),
    ];
    let Some(rt) = through_lo_to_docx(&d, "link") else { return };
    let linked: Vec<(&str, Option<&str>)> = rt.paragraphs[0]
        .runs
        .iter()
        .filter(|r| r.style.link.is_some())
        .map(|r| (r.text.as_str(), r.style.link.as_deref()))
        .collect();
    assert_eq!(
        linked,
        vec![("gnome", Some("https://gnome.org/"))],
        "hyperlink did not survive the LO pass: {:?}",
        rt.paragraphs[0].runs
    );
}

#[test]
fn font_family_size_color_survive_lo_pass() {
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![Run {
        text: "styled".into(),
        style: RunStyle {
            font_family: Some("Liberation Serif".into()),
            font_size_hp: Some(36), // 18pt
            color: Some("cc0000".into()),
            ..Default::default()
        },
    }];
    let Some(rt) = through_lo_to_docx(&d, "fontattrs") else { return };
    let run = &rt.paragraphs[0].runs[0];
    assert_eq!(run.style.font_family.as_deref(), Some("Liberation Serif"), "family lost");
    assert_eq!(run.style.font_size_hp, Some(36), "size lost");
    assert_eq!(
        run.style.color.as_deref().map(str::to_lowercase),
        Some("cc0000".into()),
        "color lost"
    );
}

#[test]
fn highlight_survives_lo_pass() {
    let mut d = Document::from_plain_text("plain marked end");
    d.apply_run_style(6, 12, &StylePatch::set_highlight(true));
    let Some(rt) = through_lo_to_docx(&d, "highlight") else { return };
    let marked: String = rt.paragraphs[0]
        .runs
        .iter()
        .filter(|r| r.style.highlight)
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(marked.trim(), "marked", "highlight lost: {:?}", rt.paragraphs[0].runs);
}

#[test]
fn header_footer_survive_lo_pass() {
    let mut d = Document::from_plain_text("body text");
    d.header = Some("Company Report".into());
    d.footer = Some("Confidential".into());
    let Some(rt) = through_lo_to_docx(&d, "hf") else { return };
    assert_eq!(rt.header.as_deref(), Some("Company Report"), "header lost");
    assert_eq!(rt.footer.as_deref(), Some("Confidential"), "footer lost");
}

#[test]
fn line_spacing_survives_lo_odt_pass() {
    // odt → LO → odt (docx read-side for spacing is blocked upstream).
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spacing.odt");
    let mut d = Document::from_plain_text("single spaced\ndouble spaced");
    d.paragraphs[1].style.line_spacing = 2.0;
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");
    let outdir = dir.path().join("out");
    std::fs::create_dir(&outdir).unwrap();
    let profile = dir.path().join("lo-profile2");
    let out = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "odt", "--outdir"])
        .arg(&outdir)
        .arg(&path)
        .output()
        .expect("soffice runs");
    assert!(out.status.success());
    let converted = outdir.join("spacing.odt");
    assert!(converted.exists(), "no LO-rewritten odt");
    let rt = letters_core::odt::read(converted.to_str().unwrap()).expect("read LO odt");
    assert!(
        (rt.paragraphs[1].style.line_spacing - 2.0).abs() < 0.05,
        "line spacing lost: {:?}",
        rt.paragraphs[1].style.line_spacing
    );
    assert!((rt.paragraphs[0].style.line_spacing - 1.0).abs() < 0.05);
}

#[test]
fn superscript_survives_lo_pass() {
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![
        Run::plain("E = mc"),
        Run {
            text: "2".into(),
            style: RunStyle { vert_align: Some(VertAlign::Superscript), ..Default::default() },
        },
    ];
    let Some(rt) = through_lo_to_docx(&d, "super") else { return };
    let sup: String = rt.paragraphs[0]
        .runs
        .iter()
        .filter(|r| r.style.vert_align == Some(VertAlign::Superscript))
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(sup, "2", "superscript lost: {:?}", rt.paragraphs[0].runs);
}

#[test]
fn list_structure_survives_lo_pass() {
    let mut d = Document::from_plain_text("intro\nalpha\nbeta\noutro");
    d.paragraphs[1].style.list = ListKind::Bullet;
    d.paragraphs[2].style.list = ListKind::Bullet;
    let Some(rt) = through_lo_to_docx(&d, "listkind") else { return };
    let kinds: Vec<ListKind> = rt.paragraphs.iter().map(|p| p.style.list).collect();
    assert_eq!(
        kinds,
        vec![ListKind::None, ListKind::Bullet, ListKind::Bullet, ListKind::None],
        "list structure lost: texts {:?}",
        rt.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>()
    );
}

// ── Oracle wave 3 (red-first): odt gaps ──────────────────────────────

#[test]
fn code_span_survives_lo_odt_pass() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.odt");
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![
        Run::plain("run "),
        Run { text: "cargo test".into(), style: RunStyle { code: true, ..Default::default() } },
    ];
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");
    let outdir = dir.path().join("out");
    std::fs::create_dir(&outdir).unwrap();
    let profile = dir.path().join("p");
    let out = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "odt", "--outdir"])
        .arg(&outdir)
        .arg(&path)
        .output()
        .expect("soffice runs");
    assert!(out.status.success());
    let rt = letters_core::odt::read(outdir.join("code.odt").to_str().unwrap()).expect("read");
    let code_text: String = rt.paragraphs[0]
        .runs
        .iter()
        .filter(|r| r.style.code)
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(code_text, "cargo test", "code span lost: {:?}", rt.paragraphs[0].runs);
}

#[test]
fn block_quote_survives_odt_round_trip() {
    // Local round-trip first (red until the odt writer knows quotes).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quote.odt");
    let mut d = Document::from_plain_text("intro\nwise words\noutro");
    d.paragraphs[1].style.block_quote = true;
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");
    let rt = letters_core::odt::read(path.to_str().unwrap()).expect("read odt");
    assert!(rt.paragraphs[1].style.block_quote, "block quote lost");
    assert!(!rt.paragraphs[0].style.block_quote);
}

#[test]
fn title_subtitle_survive_lo_odt_pass() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("named.odt");
    let mut d = Document::from_plain_text("My Document\nA modest subtitle\nbody");
    d.paragraphs[0].style.named_style = Some("Title".into());
    d.paragraphs[1].style.named_style = Some("Subtitle".into());
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");
    let outdir = dir.path().join("out");
    std::fs::create_dir(&outdir).unwrap();
    let profile = dir.path().join("p");
    let out = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "odt", "--outdir"])
        .arg(&outdir)
        .arg(&path)
        .output()
        .expect("soffice runs");
    assert!(out.status.success());
    let rt = letters_core::odt::read(outdir.join("named.odt").to_str().unwrap()).expect("read");
    assert_eq!(rt.paragraphs[0].style.named_style.as_deref(), Some("Title"), "Title lost");
    assert_eq!(
        rt.paragraphs[1].style.named_style.as_deref(),
        Some("Subtitle"),
        "Subtitle lost"
    );
}

/// A document table survives docx → LO rewrite → our reader with its
/// grid coordinates intact.
#[test]
fn table_grid_survives_lo_docx_pass() {
    use letters_core::model::TableCell;
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("table.docx");
    let mut d = Document::from_plain_text("before");
    for (r, c, text) in [(0u32, 0u32, "a1"), (0, 1, "b1"), (1, 0, "a2"), (1, 1, "b2")] {
        d.paragraphs.push(Paragraph {
            style: ParaStyle {
                table_cell: Some(TableCell { table: 0, row: r, col: c }),
                ..Default::default()
            },
            runs: vec![Run::plain(text)],
        });
    }
    docx::write(&d, path.to_str().unwrap()).expect("write docx");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let profile = dir.path().join("p");
    let st = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "docx", "--outdir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let rt = docx::read(out_dir.join("table.docx").to_str().unwrap()).expect("read");
    let mut found = std::collections::HashMap::new();
    for p in &rt.paragraphs {
        if let Some(tc) = p.style.table_cell {
            found.insert((tc.row, tc.col), p.text());
        }
    }
    assert_eq!(found.get(&(0, 0)).map(String::as_str), Some("a1"), "{found:?}");
    assert_eq!(found.get(&(1, 1)).map(String::as_str), Some("b2"), "{found:?}");
}

/// An inline image survives our docx → LO rewrite (structurally: the
/// rewritten package still embeds a media part and our reader sees an
/// image run).
#[test]
fn inline_image_survives_lo_docx_pass() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("dot.png");
    let png: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a,
        0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0,
        0xfd, 0xd4, 0x9a, 0x73,
        0, 0, 0, 21, b'I', b'D', b'A', b'T', 0x78, 0x9c, 0x62, 0xfa, 0xcf, 0xc0, 0xc0,
        0xf0, 0x1f, 0x88, 0xff, 0x33, 0x30, 0x30, 0x00, 0x00, 0x00, 0xff, 0xff,
        0x03, 0x00, 0x2b, 0x11, 0x04, 0xf9,
        0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&png_path, png).unwrap();
    let path = dir.path().join("img.docx");
    let mut d = Document::from_plain_text("caption text");
    d.paragraphs.push(Paragraph {
        style: ParaStyle::default(),
        runs: vec![Run {
            text: "alt text".into(),
            style: RunStyle {
                image: Some(png_path.to_string_lossy().to_string()),
                ..Default::default()
            },
        }],
    });
    docx::write(&d, path.to_str().unwrap()).expect("write docx");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let profile = dir.path().join("p");
    let st = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "docx", "--outdir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let rewritten = out_dir.join("img.docx");
    // Structural: the media part survived the LO rewrite.
    let f = std::fs::File::open(&rewritten).unwrap();
    let z = zip::ZipArchive::new(f).unwrap();
    let has_media = z.file_names().any(|n| n.unwrap().starts_with("word/media/"));
    assert!(has_media, "embedded image lost through LO");
    // And our reader sees an image run.
    let rt = docx::read(rewritten.to_str().unwrap()).expect("read");
    let has_image_run = rt
        .paragraphs
        .iter()
        .any(|p| p.runs.iter().any(|r| r.style.image.is_some()));
    assert!(has_image_run, "our reader lost the image run");
}

/// Line spacing through a full LO docx rewrite (read side unblocked by
/// rdocx's line_spacing_multiple getter).
#[test]
fn line_spacing_survives_lo_docx_pass() {
    let mut d = Document::from_plain_text("single spaced\ndouble spaced");
    d.paragraphs[1].style.line_spacing = 2.0;
    let Some(rt) = through_lo_to_docx(&d, "docxspacing") else { return };
    assert!(
        (rt.paragraphs[1].style.line_spacing - 2.0).abs() < 0.05,
        "line spacing lost through LO: {}",
        rt.paragraphs[1].style.line_spacing
    );
    assert!((rt.paragraphs[0].style.line_spacing - 1.0).abs() < 0.05);
}

// ── Footnotes (ADR 0003 §2) ──────────────────────────────────────────

/// Our docx footnote must survive a Writer rewrite: content intact and
/// still referenced from the body.
#[test]
fn footnote_survives_writer_rewrite() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("Body text");
    d.paragraphs[0].runs.push(Run {
        text: String::new(),
        style: RunStyle { footnote: Some(0), ..Default::default() },
    });
    d.footnotes = vec!["A note from the oracle.".into()];

    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("in");
    std::fs::create_dir(&src_dir).unwrap();
    let path = src_dir.join("fnote.docx");
    docx::write(&d, path.to_str().unwrap()).expect("write docx");
    // Distinct outdir so the rewrite cannot alias the input file.
    let profile = dir.path().join("prof");
    let st = Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "docx", "--outdir"])
        .arg(dir.path())
        .arg(&path)
        .output()
        .expect("soffice runs");
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let rewritten = dir.path().join("fnote.docx");
    assert!(rewritten.exists(), "Writer produced no docx");
    let rt = docx::read(rewritten.to_str().unwrap()).expect("read rewritten docx");
    assert!(
        rt.footnotes.iter().any(|f| f.contains("A note from the oracle")),
        "footnote text lost: {:?}",
        rt.footnotes
    );
    assert!(
        rt.paragraphs.iter().any(|p| p.runs.iter().any(|r| r.style.footnote.is_some())),
        "footnote reference lost after Writer rewrite"
    );
}

/// Paragraph spacing has to be written where ODF says it lives.
///
/// The writer emitted `fo:space-before`/`fo:space-after`, which the reader
/// here understood and LibreOffice does not — ODF spells paragraph
/// spacing `fo:margin-top`/`fo:margin-bottom`, the same XSL-FO properties
/// the page geometry already used. So the self round trip passed on an
/// attribute nothing else reads, and the spacing was gone the moment the
/// file reached Writer.
///
/// This asserts the bytes because it is a claim about the package rather
/// than about our own reader; the conversion below is what makes it a
/// claim about a real consumer.
#[test]
fn odt_paragraph_spacing_uses_the_attribute_odf_defines() {
    let mut d = Document::from_plain_text("");
    d.paragraphs[0] = Paragraph {
        style: ParaStyle { space_before_pt: 12.0, space_after_pt: 18.0, ..Default::default() },
        runs: vec![Run::plain("spaced")],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spacing.odt");
    letters_core::odt::write(&d, path.to_str().unwrap()).expect("write odt");

    let f = std::fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(f).unwrap();
    let mut xml = String::new();
    {
        use std::io::Read;
        zip.by_name("content.xml").unwrap().read_to_string(&mut xml).unwrap();
    }
    assert!(xml.contains("fo:margin-top=\"12.00pt\""), "spacing before: {xml}");
    assert!(xml.contains("fo:margin-bottom=\"18.00pt\""), "spacing after");
    assert!(
        !xml.contains("fo:space-before"),
        "still writing the attribute only this reader understands"
    );
}

/// Indents and spacing across a format boundary, in both directions.
///
/// The existing oracle tests here cover styling, lists, headings, page
/// geometry and more — but not paragraph indents or spacing, so neither
/// the docx writer's silence nor the odt writer's wrong attribute had
/// anything asking about them.
#[test]
fn indents_and_spacing_survive_a_conversion_between_the_two_formats() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("");
    d.paragraphs[0] = Paragraph {
        style: ParaStyle {
            left_indent_pt: 36.0,
            space_before_pt: 12.0,
            space_after_pt: 18.0,
            ..Default::default()
        },
        runs: vec![Run::plain("indented and spaced")],
    };
    let dir = tempfile::tempdir().unwrap();

    // odt -> Writer -> docx, through each docx filter by name.
    //
    // A bare `--convert-to docx` lets LibreOffice choose between its two
    // docx exporters, and the choice differs between installations: the
    // transitional filter writes `w:ind w:left`, the strict one
    // `w:ind w:start`. Naming both makes this test ask the same question
    // everywhere instead of whichever spelling the local build prefers.
    for (i, filter) in ["docx:MS Word 2007 XML", "docx:Office Open XML Text"].iter().enumerate() {
        let sub = dir.path().join(format!("f{i}"));
        std::fs::create_dir_all(&sub).unwrap();
        let op = sub.join("x.odt");
        letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
        let _ = soffice_convert(bin, &op, filter).ok();
        let dp = sub.join("x.docx");
        assert!(dp.exists(), "soffice did not convert the odt with {filter}");
        let rt = docx::read(dp.to_str().unwrap()).expect("read converted docx");
        let s = &para_with_text(&rt, "indented and spaced").style;
        assert!((s.left_indent_pt - 36.0).abs() < 1.0, "{filter} left indent: {}", s.left_indent_pt);
        assert!((s.space_before_pt - 12.0).abs() < 1.0, "{filter} space before: {}", s.space_before_pt);
        assert!((s.space_after_pt - 18.0).abs() < 1.0, "{filter} space after: {}", s.space_after_pt);
    }

    // docx -> Writer -> odt
    let dp2 = dir.path().join("y.docx");
    docx::write(&d, &dp2).expect("write docx");
    let _ = soffice_convert(bin, &dp2, "odt").ok();
    let op2 = dir.path().join("y.odt");
    assert!(op2.exists(), "soffice did not convert the docx");
    let rt = letters_core::odt::read(op2.to_str().unwrap()).expect("read converted odt");
    let s = &para_with_text(&rt, "indented and spaced").style;
    assert!((s.left_indent_pt - 36.0).abs() < 1.0, "docx->odt left indent: {}", s.left_indent_pt);
    assert!((s.space_before_pt - 12.0).abs() < 1.0, "docx->odt space before: {}", s.space_before_pt);
    assert!((s.space_after_pt - 18.0).abs() < 1.0, "docx->odt space after: {}", s.space_after_pt);
}

/// Footnotes across the format boundary, both ways.
///
/// `footnote_survives_writer_rewrite` above only rewrites a docx as a
/// docx, so it could not see that the odt writer and reader had no
/// `text:note` support at all: a footnote was silently dropped on every
/// odt save. These cross the boundary, which is the only way to learn
/// whether Writer accepts the note we emit and whether we read the one
/// it emits.
#[test]
fn footnotes_survive_a_conversion_between_the_two_formats() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("Body text");
    d.paragraphs[0].runs.push(Run {
        text: String::new(),
        style: RunStyle { footnote: Some(0), ..Default::default() },
    });
    d.footnotes = vec!["A note that has to cross formats.".into()];
    let dir = tempfile::tempdir().unwrap();

    // odt -> Writer -> docx
    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let op = a.join("x.odt");
    letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &op, "docx:MS Word 2007 XML").ok();
    let dp = a.join("x.docx");
    assert!(dp.exists(), "soffice did not convert the odt");
    let rt = docx::read(dp.to_str().unwrap()).expect("read converted docx");
    assert!(
        rt.footnotes.iter().any(|f| f.contains("has to cross formats")),
        "odt->docx lost the footnote: {:?}",
        rt.footnotes
    );
    assert!(
        rt.paragraphs.iter().any(|p| p.runs.iter().any(|r| r.style.footnote.is_some())),
        "odt->docx lost the reference"
    );

    // docx -> Writer -> odt
    let b = dir.path().join("b");
    std::fs::create_dir_all(&b).unwrap();
    let dp2 = b.join("y.docx");
    docx::write(&d, &dp2).expect("write docx");
    let _ = soffice_convert(bin, &dp2, "odt").ok();
    let op2 = b.join("y.odt");
    assert!(op2.exists(), "soffice did not convert the docx");
    let rt = letters_core::odt::read(op2.to_str().unwrap()).expect("read converted odt");
    assert!(
        rt.footnotes.iter().any(|f| f.contains("has to cross formats")),
        "docx->odt lost the footnote: {:?}",
        rt.footnotes
    );
    assert!(
        rt.paragraphs.iter().any(|p| p.runs.iter().any(|r| r.style.footnote.is_some())),
        "docx->odt lost the reference"
    );
    assert_eq!(
        para_with_text(&rt, "Body text").runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "Body text",
        "the note's text leaked into the body paragraph"
    );
}

/// Column layout across the format boundary, both ways.
///
/// `page_geometry_survives_lo_conversion` above asks about the page size
/// and margins but not the column count, which is how the docx writer's
/// missing `w:cols` went unnoticed: the odt side carried it, so a self
/// round trip in either format looked fine.
#[test]
fn columns_survive_a_conversion_between_the_two_formats() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("two columns crossing formats");
    d.page = Some(letters_core::model::PageGeometry {
        columns: 2,
        column_gap_pt: 24.0,
        ..Default::default()
    });
    let dir = tempfile::tempdir().unwrap();

    // odt -> Writer -> docx
    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let op = a.join("x.odt");
    letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &op, "docx:MS Word 2007 XML").ok();
    let dp = a.join("x.docx");
    assert!(dp.exists(), "soffice did not convert the odt");
    let pg = docx::read(dp.to_str().unwrap()).expect("read converted docx").page.expect("geometry");
    assert_eq!(pg.columns, 2, "odt->docx column count");

    // docx -> Writer -> odt
    let b = dir.path().join("b");
    std::fs::create_dir_all(&b).unwrap();
    let dp2 = b.join("y.docx");
    docx::write(&d, &dp2).expect("write docx");
    let _ = soffice_convert(bin, &dp2, "odt").ok();
    let op2 = b.join("y.odt");
    assert!(op2.exists(), "soffice did not convert the docx");
    let pg = letters_core::odt::read(op2.to_str().unwrap())
        .expect("read converted odt")
        .page
        .expect("geometry");
    assert_eq!(pg.columns, 2, "docx->odt column count");
}

/// Tab stops across the format boundary, both ways.
///
/// Neither writer persisted these, so a self round trip in either format
/// agreed with itself about nothing being there. These ask whether the
/// stops we now emit reach a real Writer, and whether we read the ones
/// Writer emits.
#[test]
fn tab_stops_survive_a_conversion_between_the_two_formats() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("tabbed across formats");
    d.paragraphs[0].style.tab_stops_pt = vec![
        TabStop::left(36.0),
        TabStop { pos_pt: 108.0, align: TabAlign::Right, leader: TabLeader::Dot },
    ];
    let dir = tempfile::tempdir().unwrap();

    let near = |got: &[TabStop], want: &[TabStop], what: &str| {
        assert_eq!(got.len(), want.len(), "{what}: {got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g.pos_pt - w.pos_pt).abs() < 1.0, "{what}: {got:?}");
            assert_eq!(g.align, w.align, "{what} alignment: {got:?}");
            assert_eq!(g.leader, w.leader, "{what} leader: {got:?}");
        }
    };
    let want = &d.paragraphs[0].style.tab_stops_pt;

    // odt -> Writer -> docx
    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let op = a.join("x.odt");
    letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &op, "docx:MS Word 2007 XML").ok();
    let dp = a.join("x.docx");
    assert!(dp.exists(), "soffice did not convert the odt");
    let rt = docx::read(dp.to_str().unwrap()).expect("read converted docx");
    near(&para_with_text(&rt, "tabbed across formats").style.tab_stops_pt, want, "odt->docx tab stops");

    // docx -> Writer -> odt
    let b = dir.path().join("b");
    std::fs::create_dir_all(&b).unwrap();
    let dp2 = b.join("y.docx");
    docx::write(&d, &dp2).expect("write docx");
    let _ = soffice_convert(bin, &dp2, "odt").ok();
    let op2 = b.join("y.odt");
    assert!(op2.exists(), "soffice did not convert the docx");
    let rt = letters_core::odt::read(op2.to_str().unwrap()).expect("read converted odt");
    near(&para_with_text(&rt, "tabbed across formats").style.tab_stops_pt, want, "docx->odt tab stops");
}

/// A page break across the format boundary, both ways.
///
/// This was the last field the Letters audit left open: the break
/// survived every direction except `odt -> Writer -> docx`. Nothing was
/// losing it. OOXML spells a page break two ways, and LibreOffice
/// converts ODF's `fo:break-before="page"` into the run-level one —
/// `<w:br w:type="page"/>` as the last run of the paragraph *before* the
/// break — where this reader only understood `w:pageBreakBefore`.
#[test]
fn a_page_break_survives_a_conversion_between_the_two_formats() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("first page\nsecond page");
    d.paragraphs[1].style.page_break_before = true;
    let dir = tempfile::tempdir().unwrap();

    // odt -> Writer -> docx
    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let op = a.join("x.odt");
    letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &op, "docx:MS Word 2007 XML").ok();
    let dp = a.join("x.docx");
    assert!(dp.exists(), "soffice did not convert the odt");
    let rt = docx::read(dp.to_str().unwrap()).expect("read converted docx");
    assert!(
        para_with_text(&rt, "second page").style.page_break_before,
        "odt->docx lost the page break"
    );
    assert!(
        !para_with_text(&rt, "first page").style.page_break_before,
        "odt->docx put the break on the wrong paragraph"
    );
    // The break is not content: a paragraph never holds a newline.
    assert_eq!(
        para_with_text(&rt, "first page").runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "first page",
        "the break leaked into the paragraph's text"
    );

    // docx -> Writer -> odt
    let b = dir.path().join("b");
    std::fs::create_dir_all(&b).unwrap();
    let dp2 = b.join("y.docx");
    docx::write(&d, &dp2).expect("write docx");
    let _ = soffice_convert(bin, &dp2, "odt").ok();
    let op2 = b.join("y.odt");
    assert!(op2.exists(), "soffice did not convert the docx");
    let rt = letters_core::odt::read(op2.to_str().unwrap()).expect("read converted odt");
    assert!(
        para_with_text(&rt, "second page").style.page_break_before,
        "docx->odt lost the page break"
    );
}

/// A new Letters document opens in LibreOffice in Letters' default font,
/// not a theme font: after LibreOffice saves it as .docx it reopens in
/// Liberation Serif, and its .odt default style names Liberation Serif.
#[test]
fn a_new_document_keeps_its_font_through_lo() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    for to in ["docx", "odt"] {
        let out = dir.path().join(to);
        std::fs::create_dir_all(&out).unwrap();
        let input = out.join("new.docx");
        docx::write(&Document::from_plain_text("hello"), &input).unwrap();
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        let _ = soffice_convert(bin, &input, filter);
        let converted = out.join(format!("new.{to}"));
        assert!(converted.exists(), "soffice did not convert to {to}");
        if to == "docx" {
            let rt = docx::read(converted.to_str().unwrap()).unwrap();
            let font = rt.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.style.font_family.clone()).or(rt.base_font.family);
            assert_eq!(font.as_deref(), Some("Liberation Serif"), "LibreOffice's .docx reopens in another font");
        } else {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&converted).unwrap()).unwrap();
            let mut styles = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("styles.xml").unwrap(), &mut styles).unwrap();
            assert!(styles.contains("style:font-name=\"Liberation Serif\""), "LibreOffice's default font is not ours");
            assert!(!styles.contains("Aptos"), "a theme font leaked into LibreOffice's styles");
        }
    }
}

/// `doc`'s tracked changes as (kind, author, text, date to the second),
/// adjacent changes of one kind and author taken as one (a format without
/// nesting joins a deletion of an insertion to the deletion next to it).
/// A tracked change as compared through Writer: kind, author, text, date,
/// and the pending change it was made to (a deletion of someone else's
/// insertion, `Revision::under`: #1297), as kind and author.
type Change = (RevisionKind, String, String, String, Option<(RevisionKind, String)>);

fn changes_of(doc: &Document) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    for c in letters_core::track::changes(doc) {
        let date = c.revision.date.chars().take(19).collect::<String>();
        let under = c.revision.under.as_ref().map(|u| (u.kind, u.author.clone()));
        match out.last_mut() {
            Some(last) if last.0 == c.revision.kind && last.1 == c.revision.author && last.4 == under => last.2.push_str(&c.text),
            _ => out.push((c.revision.kind, c.revision.author.clone(), c.text.clone(), date, under)),
        }
    }
    out
}

/// Tracked changes survive LibreOffice: our .docx and .odt, opened and
/// saved by Writer in either format, reopen with the same insertions and
/// deletions (their text, author and date).
#[test]
fn tracked_changes_survive_lo_passes() {
    let Some(bin) = require_or_skip() else { return };
    let d = letters_core::track::sample_document();
    let want = changes_of(&d);
    assert!(want.iter().any(|c| c.4.is_some()), "the sample needs a deletion of a pending insertion");
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [("docx", "docx"), ("odt", "odt"), ("docx", "odt"), ("odt", "docx")] {
        let work = dir.path().join(format!("{from}-{to}"));
        let out = work.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let staged = work.join(format!("tracked.{from}"));
        match from {
            "docx" => docx::write(&d, &staged).expect("write docx"),
            _ => letters_core::odt::write(&d, &staged).expect("write odt"),
        }
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        // Into a directory of its own: converting onto the input itself,
        // Writer writes nothing, and the same-format passes read our own
        // file back.
        let st = Command::new(bin)
            .arg("--headless")
            .arg(format!("-env:UserInstallation=file://{}", work.join("lo").display()))
            .args(["--convert-to", filter, "--outdir"])
            .arg(&out)
            .arg(&staged)
            .output()
            .expect("soffice");
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        let converted = out.join(format!("tracked.{to}"));
        assert!(converted.exists(), "soffice did not convert {from} to {to}");
        let rt = match to {
            "docx" => docx::read(converted.to_str().unwrap()).expect("read converted docx"),
            _ => letters_core::odt::read(converted.to_str().unwrap()).expect("read converted odt"),
        };
        assert_eq!(changes_of(&rt), want, "{from} -> LibreOffice -> {to} changed the tracked changes: {:?}", rt.paragraphs[0].runs);
    }
}

/// Comments survive LibreOffice: our .docx and .odt, opened and saved by
/// Writer in either format, reopen with the same threads (anchor text,
/// comments, replies, resolved) and the same marks on the text. Ids are
/// the reader's own, so threads compare without them.
#[test]
fn comments_survive_lo_passes() {
    let Some(bin) = require_or_skip() else { return };
    let d = letters_core::comments::sample_document();
    let want = threads_of(&d);
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [("docx", "docx"), ("odt", "odt"), ("docx", "odt"), ("odt", "docx")] {
        let work = dir.path().join(format!("{from}-{to}"));
        let out = work.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let staged = work.join(format!("commented.{from}"));
        match from {
            "docx" => docx::write(&d, &staged).expect("write docx"),
            _ => letters_core::odt::write(&d, &staged).expect("write odt"),
        }
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        // Into a directory of its own, as above: onto the input itself
        // Writer writes nothing.
        let st = Command::new(bin)
            .arg("--headless")
            .arg(format!("-env:UserInstallation=file://{}", work.join("lo").display()))
            .args(["--convert-to", filter, "--outdir"])
            .arg(&out)
            .arg(&staged)
            .output()
            .expect("soffice");
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        let converted = out.join(format!("commented.{to}"));
        assert!(converted.exists(), "soffice did not convert {from} to {to}");
        let rt = match to {
            "docx" => docx::read(converted.to_str().unwrap()).expect("read converted docx"),
            _ => letters_core::odt::read(converted.to_str().unwrap()).expect("read converted odt"),
        };
        // LibreOffice 24.2 keeps a reply's thread from .odt (odt -> odt)
        // but does not write it to .docx for a document it read from .odt
        // (no w15:paraIdParent): there the reply is a comment of its own on
        // the same text. docx -> docx keeps it.
        let want = if (from, to) == ("odt", "docx") { flattened(&want) } else { want.clone() };
        let mut got = threads_of(&rt);
        if (from, to) == ("odt", "docx") {
            got.sort();
        }
        assert_eq!(got, want, "{from} -> LibreOffice -> {to} changed the comments: {:?}", rt.comments);
    }
}

/// `threads` with every reply a thread of its own on its parent's text.
fn flattened(threads: &[ThreadView]) -> Vec<ThreadView> {
    let mut out: Vec<ThreadView> = threads
        .iter()
        .flat_map(|(anchor, c, replies)| {
            std::iter::once((anchor.clone(), c.clone(), Vec::new())).chain(replies.iter().map(|(a, d, t)| (anchor.clone(), (a.clone(), d.clone(), t.clone(), false), Vec::new())))
        })
        .collect();
    out.sort();
    out
}

/// A table of contents survives LibreOffice: our .docx (a TOC field) and
/// .odt (an index), opened and saved by Writer in either format, reopen
/// with the same entries: level, title and page number.
#[test]
fn a_table_of_contents_survives_lo_passes() {
    let Some(bin) = require_or_skip() else { return };
    let d = letters_core::toc::sample_document();
    let entries = |d: &Document| -> Vec<(u8, String)> {
        letters_core::toc::blocks(d).into_iter().flat_map(|b| d.paragraphs[b].to_vec()).map(|p| (p.style.toc.unwrap(), p.text())).collect()
    };
    let want = entries(&d);
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [("docx", "docx"), ("odt", "odt"), ("docx", "odt"), ("odt", "docx")] {
        let work = dir.path().join(format!("{from}-{to}"));
        let out = work.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let staged = out.join(format!("contents.{from}"));
        match from {
            "docx" => docx::write(&d, &staged).expect("write docx"),
            _ => letters_core::odt::write(&d, &staged).expect("write odt"),
        }
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        let _ = soffice_convert(bin, &staged, filter);
        let converted = out.join(format!("contents.{to}"));
        assert!(converted.exists(), "soffice did not convert {from} to {to}");
        let rt = match to {
            "docx" => docx::read(converted.to_str().unwrap()).expect("read converted docx"),
            _ => letters_core::odt::read(converted.to_str().unwrap()).expect("read converted odt"),
        };
        assert_eq!(entries(&rt), want, "{from} -> LibreOffice -> {to} changed the table of contents: {:?}", rt.paragraphs.iter().map(|p| (p.style.toc, p.text())).collect::<Vec<_>>());
    }
}

/// Comment threads as (anchor, (author, date, text, resolved), replies).
type ThreadView = (Option<(usize, usize, String)>, (String, String, String, bool), Vec<(String, String, String)>);

fn threads_of(doc: &Document) -> Vec<ThreadView> {
    letters_core::comments::threads(doc)
        .into_iter()
        .map(|t| {
            (
                t.anchor.map(|a| (a.start, a.end, a.text)),
                (t.comment.author, t.comment.date, t.comment.text, t.comment.resolved),
                t.replies.into_iter().map(|r| (r.author, r.date, r.text)).collect(),
            )
        })
        .collect()
}

/// The chips in `doc`, as (kind, value, label).
fn chips_of(doc: &Document) -> Vec<(letters_core::chips::ChipKind, String, String)> {
    doc.paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.style.chip.as_ref().map(|c| (c.kind, c.value.clone(), r.text.clone())))
        .collect()
}

/// Smart chips survive LibreOffice: our .docx and .odt, opened and saved by
/// Writer in either format, reopen with every chip (a date control / field,
/// the named links and bookmark) intact.
#[test]
fn smart_chips_survive_lo_passes() {
    let Some(bin) = require_or_skip() else { return };
    let d = letters_core::chips::sample_document();
    let want = chips_of(&d);
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [("docx", "docx"), ("odt", "odt"), ("docx", "odt"), ("odt", "docx")] {
        let work = dir.path().join(format!("{from}-{to}"));
        let out = work.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let input = work.join(format!("chips.{from}"));
        match from {
            "docx" => docx::write(&d, &input).expect("write docx"),
            _ => letters_core::odt::write(&d, &input).expect("write odt"),
        }
        // Convert into its own directory: a same-format pass would otherwise
        // overwrite the input.
        let staged = out.join(format!("chips.{from}"));
        std::fs::copy(&input, &staged).unwrap();
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        let _ = soffice_convert(bin, &staged, filter);
        let converted = out.join(format!("chips.{to}"));
        assert!(converted.exists(), "soffice did not convert {from} to {to}");
        let rt = match to {
            "docx" => docx::read(converted.to_str().unwrap()).expect("read converted docx"),
            _ => letters_core::odt::read(converted.to_str().unwrap()).expect("read converted odt"),
        };
        assert_eq!(chips_of(&rt), want, "{from} -> LibreOffice -> {to} lost a chip: {:?}", rt.paragraphs[0].runs);
    }
}

/// The paragraph and run attributes no other oracle test crossed the format
/// boundary with (docs/LETTERS-FIELD-MAP.md, #1205): each set on its own
/// paragraph, carried odt -> Writer -> docx and docx -> Writer -> odt, and
/// read back by our reader of the other format. A failure lists every
/// attribute lost in that direction, not just the first.
#[test]
fn paragraph_and_run_attributes_survive_a_conversion_between_the_two_formats() {
    let Some(bin) = require_or_skip() else { return };
    let run = |text: &str, style: RunStyle| Run { text: text.into(), style };
    let para = |text: &str, style: ParaStyle, run_style: RunStyle| Paragraph { style, runs: vec![run(text, run_style)] };
    let mut d = Document::from_plain_text("");
    d.paragraphs = vec![
        para("italic words", ParaStyle::default(), RunStyle { italic: true, ..Default::default() }),
        para("underlined words", ParaStyle::default(), RunStyle { underline: true, ..Default::default() }),
        para("struck words", ParaStyle::default(), RunStyle { strikethrough: true, ..Default::default() }),
        para("a second-level heading", ParaStyle { heading: Some(2), ..Default::default() }, RunStyle::default()),
        para("centred text", ParaStyle { alignment: Alignment::Center, ..Default::default() }, RunStyle::default()),
        para("right-aligned text", ParaStyle { alignment: Alignment::Right, ..Default::default() }, RunStyle::default()),
        para("justified text", ParaStyle { alignment: Alignment::Justify, ..Default::default() }, RunStyle::default()),
        para("kept with the next", ParaStyle { keep_with_next: true, ..Default::default() }, RunStyle::default()),
        para("first-line indented", ParaStyle { first_line_indent_pt: 18.0, ..Default::default() }, RunStyle::default()),
        para("right indented", ParaStyle { right_indent_pt: 36.0, ..Default::default() }, RunStyle::default()),
        para("numbered from five", ParaStyle { list: ListKind::Numbered, list_start: Some(5), ..Default::default() }, RunStyle::default()),
        para("nested bullet", ParaStyle { list: ListKind::Bullet, list_level: 1, ..Default::default() }, RunStyle::default()),
        para("let x = 1;", ParaStyle { code_block: Some(String::new()), ..Default::default() }, RunStyle::default()),
        para("a quoted passage", ParaStyle { block_quote: true, ..Default::default() }, RunStyle::default()),
        para("The Title", ParaStyle { named_style: Some("Title".into()), ..Default::default() }, RunStyle::default()),
        para("inline_code()", ParaStyle::default(), RunStyle { code: true, ..Default::default() }),
    ];
    let check = |rt: &Document, what: &str| {
        let mut lost: Vec<String> = Vec::new();
        let p = |t: &str| para_with_text(rt, t);
        let any_run = |t: &str, f: &dyn Fn(&RunStyle) -> bool| p(t).runs.iter().any(|r| !r.text.trim().is_empty() && f(&r.style));
        if !any_run("italic words", &|s| s.italic) { lost.push("italic".into()) }
        if !any_run("underlined words", &|s| s.underline) { lost.push("underline".into()) }
        if !any_run("struck words", &|s| s.strikethrough) { lost.push("strikethrough".into()) }
        if p("a second-level heading").style.heading != Some(2) { lost.push(format!("heading: {:?}", p("a second-level heading").style.heading)) }
        for (t, a) in [("centred text", Alignment::Center), ("right-aligned text", Alignment::Right), ("justified text", Alignment::Justify)] {
            if p(t).style.alignment != a { lost.push(format!("alignment {a:?}: {:?}", p(t).style.alignment)) }
        }
        if !p("kept with the next").style.keep_with_next { lost.push("keep_with_next".into()) }
        let s = &p("first-line indented").style;
        if (s.first_line_indent_pt - 18.0).abs() > 1.0 { lost.push(format!("first_line_indent_pt: {}", s.first_line_indent_pt)) }
        let s = &p("right indented").style;
        if (s.right_indent_pt - 36.0).abs() > 1.0 { lost.push(format!("right_indent_pt: {}", s.right_indent_pt)) }
        let s = &p("numbered from five").style;
        if s.list != ListKind::Numbered || s.list_start.unwrap_or(1) != 5 { lost.push(format!("list_start: {:?} {:?}", s.list, s.list_start)) }
        let s = &p("nested bullet").style;
        if s.list != ListKind::Bullet || s.list_level != 1 { lost.push(format!("list_level: {:?} {}", s.list, s.list_level)) }
        if p("let x = 1;").style.code_block.is_none() { lost.push("code_block".into()) }
        if !p("a quoted passage").style.block_quote { lost.push(format!("block_quote: {:?}", p("a quoted passage").style)) }
        if p("The Title").style.named_style.as_deref() != Some("Title") { lost.push(format!("named_style: {:?}", p("The Title").style.named_style)) }
        if !any_run("inline_code()", &|s| s.code) { lost.push("code (run)".into()) }
        assert!(lost.is_empty(), "{what} lost: {lost:#?}");
    };
    let dir = tempfile::tempdir().unwrap();

    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let op = a.join("x.odt");
    letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &op, "docx:MS Word 2007 XML").ok();
    let dp = a.join("x.docx");
    assert!(dp.exists(), "soffice did not convert the odt");
    let from_odt = docx::read(dp.to_str().unwrap()).expect("read converted docx");

    let b = dir.path().join("b");
    std::fs::create_dir_all(&b).unwrap();
    let dp2 = b.join("y.docx");
    docx::write(&d, &dp2).expect("write docx");
    let _ = soffice_convert(bin, &dp2, "odt").ok();
    let op2 = b.join("y.odt");
    assert!(op2.exists(), "soffice did not convert the docx");
    let from_docx = letters_core::odt::read(op2.to_str().unwrap()).expect("read converted odt");

    let first = std::panic::catch_unwind(|| check(&from_odt, "odt -> Writer -> docx"));
    check(&from_docx, "docx -> Writer -> odt");
    if let Err(e) = first {
        std::panic::resume_unwind(e);
    }
}

/// The document's heading look and a picture's displayed size through a
/// docx Writer itself rewrites (docs/LETTERS-FIELD-MAP.md, #1205). Both
/// live in docx only: the heading look in styles.xml's Heading styles,
/// the size in the drawing's extent. Writer re-derives both when it saves.
#[test]
fn heading_styles_and_picture_size_survive_writer_rewriting_a_docx() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("dot.png");
    let png: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a,
        0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0,
        0xfd, 0xd4, 0x9a, 0x73,
        0, 0, 0, 21, b'I', b'D', b'A', b'T', 0x78, 0x9c, 0x62, 0xfa, 0xcf, 0xc0, 0xc0,
        0xf0, 0x1f, 0x88, 0xff, 0x33, 0x30, 0x30, 0x00, 0x00, 0x00, 0xff, 0xff,
        0x03, 0x00, 0x2b, 0x11, 0x04, 0xf9,
        0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&png_path, png).unwrap();
    let mut d = Document::from_plain_text("A heading\nbody");
    d.paragraphs[0].style.heading = Some(1);
    d.heading_styles = vec![RunStyle { color: Some("c00000".into()), font_size_hp: Some(40), bold: true, ..Default::default() }];
    let extent = (1_828_800, 914_400); // 2 x 1 inches
    d.paragraphs.push(Paragraph {
        style: ParaStyle::default(),
        runs: vec![Run {
            text: "a picture".into(),
            style: RunStyle { image: Some(png_path.to_string_lossy().into()), image_extent_emu: Some(extent), ..Default::default() },
        }],
    });
    let src = dir.path().join("in").join("h.docx");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    docx::write(&d, &src).expect("write docx");
    // Into a directory of its own: Writer will not convert a file onto
    // itself, so reading the input's name back would read our own bytes.
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let st = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", dir.path().join("p").display()))
        .args(["--convert-to", "docx:MS Word 2007 XML", "--outdir"])
        .arg(&out_dir)
        .arg(&src)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let rewritten = out_dir.join("h.docx");
    assert_ne!(std::fs::read(&rewritten).unwrap(), std::fs::read(&src).unwrap(), "Writer did not rewrite the docx");
    let rt = docx::read(rewritten.to_str().unwrap()).expect("read Writer's docx");
    let h1 = rt.heading_styles.first().cloned().unwrap_or_default();
    assert_eq!(h1.color.as_deref().map(str::to_lowercase).as_deref(), Some("c00000"), "heading colour: {:?}", rt.heading_styles);
    assert_eq!(h1.font_size_hp, Some(40), "heading size: {:?}", rt.heading_styles);
    let pic = rt.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.style.image.is_some()).expect("the picture");
    let (w, h) = pic.style.image_extent_emu.expect("the picture's size");
    assert!(w.abs_diff(extent.0) < 20_000 && h.abs_diff(extent.1) < 20_000, "size came back as {w} x {h} EMU");
}

/// Right-to-left text through Writer in both formats (#1281). Paragraph
/// direction is not a Letters feature yet (docs/LETTERS-REVIEW-WORKFLOWS.md
/// lists it as unavailable): no control sets it and neither writer writes
/// one, and the screen detects it per paragraph. What is promised is the
/// text: Hebrew, Arabic and mixed lines come back exactly, in logical order,
/// after Writer converts our odt to docx and our docx to odt.
#[test]
fn right_to_left_text_survives_a_conversion_between_the_two_formats() {
    let Some(bin) = require_or_skip() else { return };
    let lines = ["שלום עולם", "مرحبا بالعالم", "Mixed: שלום and مرحبا 123"];
    let d = Document::from_plain_text(&lines.join("\n"));
    let texts = |doc: &Document| doc.paragraphs.iter().map(|p| p.text()).filter(|t| !t.is_empty()).collect::<Vec<_>>();
    let dir = tempfile::tempdir().unwrap();

    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let op = a.join("x.odt");
    letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &op, "docx:MS Word 2007 XML").ok();
    let rt = docx::read(a.join("x.docx").to_str().unwrap()).expect("read Writer's docx");
    assert_eq!(texts(&rt), lines, "odt -> Writer -> docx");

    let b = dir.path().join("b");
    std::fs::create_dir_all(&b).unwrap();
    let dp = b.join("y.docx");
    docx::write(&d, &dp).expect("write docx");
    let _ = soffice_convert(bin, &dp, "odt").ok();
    let rt = letters_core::odt::read(b.join("y.odt").to_str().unwrap()).expect("read Writer's odt");
    assert_eq!(texts(&rt), lines, "docx -> Writer -> odt");
}

// ── Interop evidence gaps (docs/INTEROP-EVIDENCE.md, #1276) ───────────

/// Hyperlinks in our docx, through Writer, into the odt it writes.
///
/// `hyperlink_survives_lo_pass` crosses the other way (our odt → Writer's
/// docx), so neither our docx writer's links nor our odt reader's reading
/// of Writer's `text:a` had been through LibreOffice.
#[test]
fn hyperlinks_in_our_docx_reach_writers_odt() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![
        Run::plain("visit "),
        Run {
            text: "gnome".into(),
            style: RunStyle { link: Some("https://gnome.org/".into()), ..Default::default() },
        },
        Run::plain(" today"),
    ];
    let dir = tempfile::tempdir().unwrap();
    let dp = dir.path().join("link.docx");
    docx::write(&d, &dp).expect("write docx");
    let _ = soffice_convert(bin, &dp, "odt").ok();
    let op = dir.path().join("link.odt");
    assert!(op.exists(), "soffice did not convert the docx");
    let rt = letters_core::odt::read(op.to_str().unwrap()).expect("read converted odt");
    let linked: Vec<(String, Option<String>)> = rt
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .filter(|r| r.style.link.is_some())
        .map(|r| (r.text.clone(), r.style.link.clone()))
        .collect();
    assert_eq!(
        linked,
        vec![("gnome".to_string(), Some("https://gnome.org/".to_string()))],
        "the link did not survive docx -> Writer -> odt: {:?}",
        rt.paragraphs
    );
}

/// A picture Writer itself places, read from the docx Writer writes.
///
/// `inline_image_survives_lo_docx_pass` re-saves our own docx, so the
/// drawing markup it reads back is still shaped by our writer. Here the
/// source is a flat ODT whose `draw:frame` carries the picture inline, so
/// every byte of the docx's drawing comes from Writer.
#[test]
fn we_read_a_picture_writer_places_in_a_docx() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("pic.fodt");
    std::fs::write(&src, r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
  xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"
  xmlns:xlink="http://www.w3.org/1999/xlink"
  office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.text">
 <office:body><office:text>
  <text:p>before the picture</text:p>
  <text:p><draw:frame draw:name="dot" text:anchor-type="as-char" svg:width="1cm" svg:height="1cm"><draw:image><office:binary-data>iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFUlEQVR4nGL6z8DA8B+I/zMwMAAAAP//AwArEQT5AAAAAElFTkSuQmCC</office:binary-data></draw:image></draw:frame></text:p>
  <text:p>after the picture</text:p>
 </office:text></office:body>
</office:document>
"#).unwrap();
    let _ = soffice_convert(bin, &src, "docx").ok();
    let dp = dir.path().join("pic.docx");
    assert!(dp.exists(), "soffice did not convert the flat odt");
    let rt = docx::read(dp.to_str().unwrap()).expect("read Writer's docx");
    let text = norm(&rt.to_plain_text());
    assert!(text.contains("before the picture") && text.contains("after the picture"), "{text:?}");
    let pictures = rt.paragraphs.iter().flat_map(|p| p.runs.iter()).filter(|r| r.style.image.is_some()).count();
    assert_eq!(pictures, 1, "the picture Writer placed was not read: {:?}", rt.paragraphs);
}

/// Pictures in our odt, through Writer, back into our reader (#1292).
///
/// Writer rewrites the odt (into a directory of its own: it will not
/// convert a file onto itself), so the frames we read back are Writer's.
/// The picture keeps its bytes, its place between the words around it,
/// its size and its alt text.
#[test]
fn pictures_in_our_odt_survive_writer_rewriting_it() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    // A valid 3x2 PNG (Writer re-encodes what it can't parse).
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xf1,
        0x4d, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x3c, 0x21, 0x27, 0xc7,
        0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc4, 0x00, 0x03, 0x00, 0x13, 0x2e, 0x01, 0x08, 0x6a, 0xc0,
        0x65, 0x61, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let png_path = dir.path().join("dot.png");
    std::fs::write(&png_path, png).unwrap();
    let extent = (1_828_800, 914_400); // 2 x 1 inches
    let mut d = Document::from_plain_text("");
    d.paragraphs = vec![Paragraph {
        style: ParaStyle::default(),
        runs: vec![
            Run::plain("before "),
            Run {
                text: "a dot".into(),
                style: RunStyle { image: Some(png_path.to_string_lossy().into()), image_extent_emu: Some(extent), ..Default::default() },
            },
            Run::plain(" after"),
        ],
    }];
    let src = dir.path().join("in").join("p.odt");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    letters_core::odt::write(&d, &src).expect("write odt");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let st = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", dir.path().join("lo").display()))
        .args(["--convert-to", "odt", "--outdir"])
        .arg(&out_dir)
        .arg(&src)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let rewritten = out_dir.join("p.odt");
    assert_ne!(std::fs::read(&rewritten).unwrap(), std::fs::read(&src).unwrap(), "Writer did not rewrite the odt");
    let rt = letters_core::odt::read(rewritten.to_str().unwrap()).expect("read Writer's odt");
    let runs: Vec<&Run> = rt.paragraphs.iter().flat_map(|p| &p.runs).collect();
    let pics: Vec<&&Run> = runs.iter().filter(|r| r.style.image.is_some()).collect();
    assert_eq!(pics.len(), 1, "the picture did not come back: {:?}", rt.paragraphs);
    let pic = pics[0];
    assert_eq!(std::fs::read(pic.style.image.as_ref().unwrap()).unwrap(), png, "the picture's bytes");
    let (w, h) = pic.style.image_extent_emu.expect("a size");
    // Writer stores lengths in hundredths of a millimetre: within 0.01mm.
    assert!(w.abs_diff(extent.0) <= 360 && h.abs_diff(extent.1) <= 360, "size {:?}, wrote {extent:?}", (w, h));
    assert_eq!(pic.text, "a dot", "alt text");
    let text: String = rt.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("before") && text.contains("after"), "{text:?}");
    let para = rt.paragraphs.iter().find(|p| p.runs.iter().any(|r| r.style.image.is_some())).unwrap();
    let at = para.runs.iter().position(|r| r.style.image.is_some()).unwrap();
    assert!(para.runs[..at].iter().any(|r| r.text.contains("before")) && para.runs[at + 1..].iter().any(|r| r.text.contains("after")),
        "the picture moved out of its line: {:?}", para.runs);
}

/// A picture Writer itself places, read from the odt Writer writes (#1292).
///
/// The source is a flat ODT, which Writer converts to a packaged odt, so
/// the frame, the Pictures/ member and the manifest are all Writer's.
#[test]
fn we_read_a_picture_writer_places_in_an_odt() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("pic.fodt");
    std::fs::write(&src, r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
  xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
  xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
  xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"
  xmlns:xlink="http://www.w3.org/1999/xlink"
  office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.text">
 <office:body><office:text>
  <text:p>before the picture</text:p>
  <text:p><draw:frame draw:name="dot" text:anchor-type="as-char" svg:width="2cm" svg:height="1cm"><draw:image><office:binary-data>iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFUlEQVR4nGL6z8DA8B+I/zMwMAAAAP//AwArEQT5AAAAAElFTkSuQmCC</office:binary-data></draw:image><svg:title>a red dot</svg:title></draw:frame></text:p>
  <text:p>after the picture</text:p>
 </office:text></office:body>
</office:document>
"#).unwrap();
    let _ = soffice_convert(bin, &src, "odt").ok();
    let op = dir.path().join("pic.odt");
    assert!(op.exists(), "soffice did not convert the flat odt");
    let zip = zip::ZipArchive::new(std::fs::File::open(&op).unwrap()).unwrap();
    assert!(zip.file_names().any(|n| n.unwrap().starts_with("Pictures/")), "Writer stored no picture");
    let rt = letters_core::odt::read(op.to_str().unwrap()).expect("read Writer's odt");
    let text = norm(&rt.to_plain_text());
    assert!(text.contains("before the picture") && text.contains("after the picture"), "{text:?}");
    let pics: Vec<&Run> = rt.paragraphs.iter().flat_map(|p| &p.runs).filter(|r| r.style.image.is_some()).collect();
    assert_eq!(pics.len(), 1, "the picture Writer placed was not read: {:?}", rt.paragraphs);
    assert!(std::fs::read(pics[0].style.image.as_ref().unwrap()).unwrap().starts_with(b"\x89PNG"), "not the picture's bytes");
    let (w, h) = pics[0].style.image_extent_emu.expect("a size");
    assert!(w.abs_diff(720_000) <= 360 && h.abs_diff(360_000) <= 360, "2cm x 1cm read as {:?} EMU", (w, h));
    assert_eq!(pics[0].text, "a red dot", "alt text");
}

/// A table's paragraphs as (table, row, col, text), body paragraphs as None.
fn table_shape(doc: &Document) -> Vec<(Option<(u32, u32)>, String)> {
    doc.paragraphs
        .iter()
        .filter(|p| p.style.table_cell.is_some() || !p.text().trim().is_empty())
        .map(|p| (p.style.table_cell.map(|c| (c.row, c.col)), p.text().trim().to_string()))
        .collect()
}

fn table_document() -> Document {
    let cell = |row, col, text: &str| Paragraph {
        style: ParaStyle { table_cell: Some(TableCell { table: 0, row, col }), ..Default::default() },
        runs: if text.is_empty() { Vec::new() } else { vec![Run::plain(text)] },
    };
    let mut d = Document::from_plain_text("before the table");
    d.paragraphs.extend([
        cell(0, 0, "Item"), cell(0, 1, "Q1"), cell(0, 2, "Q2"),
        cell(1, 0, "Rent"), cell(1, 1, ""), cell(1, 2, "1250"),
    ]);
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("after the table")] });
    d
}

/// A table in our odt, rewritten by Writer, read back cell by cell
/// (#1296): every cell in its place, the empty one included, and the text
/// around the table outside it.
#[test]
fn tables_in_our_odt_survive_writer_rewriting_it() {
    let Some(bin) = require_or_skip() else { return };
    let d = table_document();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("in").join("t.odt");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    letters_core::odt::write(&d, &src).expect("write odt");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let st = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", dir.path().join("lo").display()))
        .args(["--convert-to", "odt", "--outdir"])
        .arg(&out_dir)
        .arg(&src)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let rewritten = out_dir.join("t.odt");
    assert_ne!(std::fs::read(&rewritten).unwrap(), std::fs::read(&src).unwrap(), "Writer did not rewrite the odt");
    let rt = letters_core::odt::read(rewritten.to_str().unwrap()).expect("read Writer's odt");
    assert_eq!(table_shape(&rt), table_shape(&d));
}

/// A table Writer writes into an odt, converting our docx (#1296): the
/// table markup is all Writer's.
#[test]
fn we_read_the_table_writer_writes_into_an_odt() {
    let Some(bin) = require_or_skip() else { return };
    let d = table_document();
    let dir = tempfile::tempdir().unwrap();
    let dp = dir.path().join("t.docx");
    docx::write(&d, &dp).expect("write docx");
    let _ = soffice_convert(bin, &dp, "odt").ok();
    let rt = letters_core::odt::read(dir.path().join("t.odt").to_str().unwrap()).expect("read Writer's odt");
    assert_eq!(table_shape(&rt), table_shape(&d));
}

/// How headings look, through Writer, in odt (#1297): Writer rewrites our
/// odt, and converts it to docx; both keep heading 1's colour, size, weight
/// and face, which the odt writer used not to write at all.
#[test]
fn heading_looks_survive_writer_in_odt() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("A heading\nbody");
    d.paragraphs[0].style.heading = Some(1);
    d.heading_styles = vec![RunStyle { color: Some("c00000".into()), font_size_hp: Some(40), bold: true, font_family: Some("DejaVu Serif".into()), ..Default::default() }];
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("in").join("h.odt");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    letters_core::odt::write(&d, &src).expect("write odt");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    for to in ["odt", "docx:MS Word 2007 XML"] {
        let st = std::process::Command::new(bin)
            .arg("--headless")
            .arg(format!("-env:UserInstallation=file://{}", dir.path().join("lo").display()))
            .args(["--convert-to", to, "--outdir"])
            .arg(&out_dir)
            .arg(&src)
            .output()
            .expect("soffice");
        assert!(st.status.success());
    }
    let from_odt = letters_core::odt::read(out_dir.join("h.odt").to_str().unwrap()).expect("read Writer's odt");
    let from_docx = docx::read(out_dir.join("h.docx").to_str().unwrap()).expect("read Writer's docx");
    for (what, rt) in [("odt -> Writer -> odt", &from_odt), ("odt -> Writer -> docx", &from_docx)] {
        let h1 = rt.heading_styles.first().cloned().unwrap_or_default();
        assert_eq!(h1.color.as_deref().map(str::to_lowercase).as_deref(), Some("c00000"), "{what}: colour {:?}", rt.heading_styles);
        assert_eq!(h1.font_size_hp, Some(40), "{what}: size");
        assert!(h1.bold, "{what}: weight");
        assert_eq!(h1.font_family.as_deref(), Some("DejaVu Serif"), "{what}: face");
        assert_eq!(rt.paragraphs[0].style.heading, Some(1), "{what}: still a heading");
    }
}

/// A table nested in a cell, as LibreOffice writes it to docx: its text is
/// read into the outer cell, and a save is told the nesting will be
/// flattened (#1419). The two inner cells used to be dropped without a word.
#[test]
fn a_nested_table_lo_wrote_keeps_its_text() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("nested.html");
    std::fs::write(
        &html,
        "<html><body><p>before</p><table><tr><td>outer A<table><tr><td>inner one</td><td>inner two</td></tr></table></td><td>outer B</td></tr></table><p>after</p></body></html>",
    )
    .unwrap();
    let profile = dir.path().join("p");
    let out = Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--infilter=HTML (StarWriter)", "--convert-to", "docx:MS Word 2007 XML", "--outdir"])
        .arg(dir.path())
        .arg(&html)
        .output()
        .expect("soffice");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let docx_path = dir.path().join("nested.docx");
    let doc = docx::read(docx_path.to_str().unwrap()).expect("read");
    let outer_a = para_with_text(&doc, "outer A").style.table_cell.expect("outer A is in a cell");
    for inner in ["inner one", "inner two"] {
        let cell = para_with_text(&doc, inner).style.table_cell;
        assert_eq!(cell, Some(outer_a), "{inner} should be in outer A's cell");
    }
    let report = letters_core::loss::content_a_save_drops(&docx_path);
    let ids: Vec<String> = report.destructive_features().iter().map(|f| f.id.clone()).collect();
    assert!(ids.contains(&"nested-tables".to_string()), "{ids:?}");
}

/// Table column widths through LibreOffice, both ways: our docx → Writer
/// converts it to odt → our odt reader sees the widths, and our odt →
/// Writer converts it to docx → our docx reader sees them. A narrow
/// number column beside a wide one is how agendas and forms are laid out;
/// without widths every table came out in equal columns.
#[test]
fn table_column_widths_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("after the table");
    let table = d.insert_table_at(0, 2, 2);
    for p in d.paragraphs.iter_mut().filter(|p| p.style.table_cell.is_some()) {
        p.runs = vec![Run::plain("cell")];
    }
    let want = [36.0, 400.0];
    d.table_columns.insert(table, want.to_vec());
    let close = |got: &Document, how: &str| {
        let widths: Vec<&Vec<f64>> = got.table_columns.values().collect();
        assert_eq!(widths.len(), 1, "{how}: {:?}", got.table_columns);
        assert!(widths[0].iter().zip(want).all(|(a, b)| (a - b).abs() < 1.0), "{how}: {widths:?}");
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("widths.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("widths.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "widths") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Table row heights through LibreOffice, both ways, as for column
/// widths: a minimum height stays a minimum and a fixed one stays fixed.
/// A form's tall answer rows were drawn one line high without them.
#[test]
fn table_row_heights_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("after the table");
    let table = d.insert_table_at(0, 3, 2);
    for p in d.paragraphs.iter_mut().filter(|p| p.style.table_cell.is_some()) {
        p.runs = vec![Run::plain("cell")];
    }
    let want = vec![Some(letters_core::RowHeight { pt: 40.0, exact: false }), None, Some(letters_core::RowHeight { pt: 30.0, exact: true })];
    d.table_rows.insert(table, want.clone());
    let close = |got: &Document, how: &str| {
        let rows: Vec<&Vec<Option<letters_core::RowHeight>>> = got.table_rows.values().collect();
        assert_eq!(rows.len(), 1, "{how}: {:?}", got.table_rows);
        let same = |a: &Option<letters_core::RowHeight>, b: &Option<letters_core::RowHeight>| match (a, b) {
            (Some(a), Some(b)) => a.exact == b.exact && (a.pt - b.pt).abs() < 1.0,
            (a, b) => a.is_none() && b.is_none(),
        };
        let padded = |v: &Vec<Option<letters_core::RowHeight>>| (0..3).map(|i| v.get(i).copied().flatten()).collect::<Vec<_>>();
        assert!(padded(rows[0]).iter().zip(&want).all(|(a, b)| same(a, b)), "{how}: {rows:?}");
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("heights.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("heights.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "heights") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Merged cells through LibreOffice, both ways: a heading across the
/// table and a cell down two rows keep their spans, and the text stays in
/// its cell.
#[test]
fn merged_cells_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("after the table");
    let table = d.insert_table_at(0, 3, 3);
    for p in d.paragraphs.iter_mut() {
        let Some(c) = p.style.table_cell else { continue };
        if ((c.row, c.col) == (0, 0) || c.row > 0) && (c.row, c.col) != (2, 0) {
            p.runs = vec![Run::plain(format!("r{}c{}", c.row, c.col))];
        }
    }
    let spans = vec![
        letters_core::CellSpan { row: 0, col: 0, rows: 1, cols: 3 },
        letters_core::CellSpan { row: 1, col: 0, rows: 2, cols: 1 },
    ];
    d.table_spans.insert(table, spans.clone());
    let texts = |d: &Document| -> Vec<(u32, u32, String)> {
        d.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| (c.row, c.col, p.text()))).filter(|(_, _, t)| !t.is_empty()).collect()
    };
    let close = |got: &Document, how: &str| {
        let mut got_spans: Vec<_> = got.table_spans.values().flatten().copied().collect();
        got_spans.sort_by_key(|s| (s.row, s.col));
        assert_eq!(got_spans, spans, "{how}");
        assert_eq!(texts(got), texts(&d), "{how}");
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("merged.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("merged.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "merged") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A numbered item's label through LibreOffice, both ways: "(1)", "1.a",
/// a roman "IV." after a change of label within the list, and Word's
/// "2.1" with no number in it. Each item shows what it showed before.
#[test]
fn list_labels_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let label = |prefix: &str, format, suffix: &str| Some(ListLabel { prefix: prefix.into(), format, suffix: suffix.into() });
    let mut d = Document::from_plain_text("one\nsub\ntwo\nthree\nfour\nbetween\nliteral");
    for (k, level, l) in [
        (0, 0, label("(", NumberFormat::Decimal, ")")),
        (1, 1, label("1.", NumberFormat::LowerLetter, "")),
        (2, 0, label("(", NumberFormat::Decimal, ")")),
        (3, 0, None),
        (4, 0, label("", NumberFormat::UpperRoman, ".")),
        (6, 0, label("2.1", NumberFormat::None, "")),
    ] {
        d.paragraphs[k].style.list = ListKind::Numbered;
        d.paragraphs[k].style.list_level = level;
        d.paragraphs[k].style.list_label = l;
    }
    let markers = |d: &Document| {
        let ordinals = letters_core::lists::ordinals(d.paragraphs.iter().map(|p| &p.style));
        d.paragraphs
            .iter()
            .zip(ordinals)
            .filter(|(p, _)| !p.text().is_empty())
            .map(|(p, n)| (p.text(), letters_core::lists::marker_for(&p.style, n).unwrap_or_default()))
            .collect::<Vec<_>>()
    };
    let want = markers(&d);
    assert_eq!(
        want.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>(),
        ["(1)", "1.a", "(2)", "3.", "IV.", "", "2.1"]
    );

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("labels.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("labels.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    let got = letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt");
    assert_eq!(markers(&got), want, "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "labels") else { return };
    assert_eq!(markers(&rt), want, "odt → Writer → docx");
}

/// Shaded table cells through LibreOffice, both ways: a form's grey
/// header cells keep their colour.
#[test]
fn table_cell_fills_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("after the table");
    let table = d.insert_table_at(0, 2, 2);
    for p in d.paragraphs.iter_mut().filter(|p| p.style.table_cell.is_some()) {
        p.runs = vec![Run::plain("cell")];
    }
    let want = vec![letters_core::CellFill { row: 0, col: 0, color: "D9D9D9".into() }, letters_core::CellFill { row: 1, col: 1, color: "DEEAF6".into() }];
    d.table_fills.insert(table, want.clone());
    let close = |got: &Document, how: &str| {
        let fills: Vec<&Vec<letters_core::CellFill>> = got.table_fills.values().collect();
        assert_eq!(fills, [&want], "{how}");
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("fills.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("fills.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "fills") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A floating picture keeps its placement through Writer both ways: our
/// docx converted to odt, and our odt converted to docx. The Ofsted logo
/// of the audit-committee minutes (right-aligned in the margins, raised
/// above its paragraph) and a picture behind the text, an inch in from the
/// page's edge and centred down it.
#[test]
fn floating_pictures_keep_their_placement_through_writer_both_ways() {
    use letters_core::model::{AnchorAlign, AnchorFrame, ImageAnchor, Wrap};
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xf1,
        0x4d, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x3c, 0x21, 0x27, 0xc7,
        0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc4, 0x00, 0x03, 0x00, 0x13, 0x2e, 0x01, 0x08, 0x6a, 0xc0,
        0x65, 0x61, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let png_path = dir.path().join("logo.png");
    std::fs::write(&png_path, png).unwrap();
    let want = [
        // Text beside it, text above and below it, and text over it.
        ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, y_emu: -660_400, wrap: Wrap::Parallel, ..Default::default() },
        ImageAnchor { h_from: AnchorFrame::Margin, x_emu: 457_200, v_from: AnchorFrame::Text, y_emu: 127_000, wrap: Wrap::TopBottom, ..Default::default() },
        ImageAnchor { h_from: AnchorFrame::Page, x_emu: 914_400, v_from: AnchorFrame::Page, v_align: Some(AnchorAlign::Center), behind: true, ..Default::default() },
    ];
    let mut d = Document::from_plain_text("");
    d.paragraphs = want
        .iter()
        .map(|a| Paragraph {
            style: ParaStyle::default(),
            runs: vec![Run::plain("Minutes "), Run {
                text: "logo".into(),
                style: RunStyle { image: Some(png_path.to_string_lossy().into()), image_extent_emu: Some((914_400, 457_200)), image_anchor: Some(*a), ..Default::default() },
            }],
        })
        .collect();
    let close = |got: &Document, how: &str| {
        let read: Vec<ImageAnchor> = got.paragraphs.iter().flat_map(|p| &p.runs).filter_map(|r| r.style.image_anchor).collect();
        assert_eq!(read.len(), want.len(), "{how}: {:?}", got.paragraphs);
        for (r, w) in read.iter().zip(&want) {
            // Writer stores lengths in hundredths of a millimetre.
            assert!(r.x_emu.abs_diff(w.x_emu) <= 360 && r.y_emu.abs_diff(w.y_emu) <= 360, "{how}: {r:?} for {w:?}");
            assert_eq!(ImageAnchor { x_emu: w.x_emu, y_emu: w.y_emu, ..*r }, *w, "{how}");
        }
    };

    let docx_path = dir.path().join("float.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("float.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "float") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A letterhead's logo and a footer's picture survive Writer both ways:
/// our docx converted to odt, and our odt converted to docx. Each keeps its
/// place (the header or the footer), its bytes, its size and its alt text.
#[test]
fn header_and_footer_pictures_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xf1,
        0x4d, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x3c, 0x21, 0x27, 0xc7,
        0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc4, 0x00, 0x03, 0x00, 0x13, 0x2e, 0x01, 0x08, 0x6a, 0xc0,
        0x65, 0x61, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let png_path = dir.path().join("logo.png");
    std::fs::write(&png_path, png).unwrap();
    let picture = |alt: &str| Run {
        text: alt.into(),
        style: RunStyle { image: Some(png_path.to_string_lossy().into()), image_extent_emu: Some((1_304_925, 542_925)), ..Default::default() },
    };
    let mut d = Document::from_plain_text("body text");
    d.header = Some("Office of Financial Sanctions".into());
    d.header_pictures = vec![picture("HM Treasury")];
    d.footer = Some("October 2025".into());
    d.footer_pictures = vec![picture("seal")];
    let close = |got: &Document, how: &str| {
        for (pics, alt) in [(&got.header_pictures, "HM Treasury"), (&got.footer_pictures, "seal")] {
            assert_eq!(pics.len(), 1, "{how}: {alt}: header {:?} footer {:?}", got.header_pictures, got.footer_pictures);
            let p = &pics[0];
            assert_eq!(p.text, alt, "{how}: alt text");
            let (w, h) = p.style.image_extent_emu.expect("a size");
            // Writer keeps lengths in hundredths of a millimetre.
            assert!(w.abs_diff(1_304_925) <= 360 && h.abs_diff(542_925) <= 360, "{how}: {alt} {:?}", (w, h));
            assert!(std::fs::read(p.style.image.as_ref().unwrap()).unwrap().starts_with(b"\x89PNG"), "{how}: {alt}'s bytes");
        }
        assert!(got.header.as_deref().is_some_and(|h| h.contains("Office of Financial Sanctions")), "{how}: {:?}", got.header);
        assert!(got.paragraphs.iter().all(|p| p.runs.iter().all(|r| r.style.image.is_none())), "{how}: a picture moved into the body");
    };

    let docx_path = dir.path().join("hf.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("hf.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "hf") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A header at the right and a footer centred survive Writer both ways:
/// our docx converted to odt, and our odt converted to docx.
#[test]
fn header_and_footer_alignment_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("body text");
    d.header = Some("FINANCE BILL".into());
    d.header_alignment = Alignment::Right;
    d.footer = Some("OFFICIAL".into());
    d.footer_alignment = Alignment::Center;
    let close = |got: &Document, how: &str| {
        assert_eq!((got.header.as_deref(), got.header_alignment), (Some("FINANCE BILL"), Alignment::Right), "{how}");
        assert_eq!((got.footer.as_deref(), got.footer_alignment), (Some("OFFICIAL"), Alignment::Center), "{how}");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("align.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("align.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "align") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Each line of a header and footer keeps its own alignment, look and
/// tab stops through Writer both ways: the crime contract's 16pt title
/// centred over a ship's name with its ID at a right tab, and a footer's
/// page number at the right over its date at the left.
#[test]
fn header_and_footer_lines_survive_writer_both_ways() {
    use letters_core::{HfLine, RunStyle, TabAlign, TabStop};
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("body text");
    let sans = |hp: u16| RunStyle { font_family: Some("Liberation Sans".into()), font_size_hp: Some(hp), ..Default::default() };
    d.header = Some("The Crime Contract 2025\nName of ship:\tID:".into());
    d.header_lines = vec![
        HfLine { alignment: Alignment::Center, look: RunStyle { bold: true, ..sans(32) }, ..Default::default() },
        HfLine { look: sans(18), tab_stops: vec![TabStop { pos_pt: 450.0, align: TabAlign::Right, leader: letters_core::TabLeader::None }], ..Default::default() },
    ];
    d.footer = Some("{page}\nOctober 2025".into());
    d.footer_lines = vec![
        HfLine { alignment: Alignment::Right, ..Default::default() },
        HfLine { look: RunStyle { color: Some("1f4e79".into()), ..sans(22) }, ..Default::default() },
    ];
    let close = |got: &Document, how: &str| {
        assert_eq!(got.header.as_deref(), d.header.as_deref(), "{how}: header text");
        assert_eq!(got.footer.as_deref(), d.footer.as_deref(), "{how}: footer text");
        for (name, text, want, got) in [("header", &d.header, &d.header_lines, &got.header_lines), ("footer", &d.footer, &d.footer_lines, &got.footer_lines)] {
            assert_eq!(got.len(), want.len(), "{how}: {name} lines {got:?}");
            let texts: Vec<&str> = text.as_deref().unwrap_or("").split('\n').collect();
            for (k, (w, g)) in want.iter().zip(got).enumerate() {
                assert_eq!(g.alignment, w.alignment, "{how}: {name} line {k} alignment");
                // What the line sets comes back; Writer spells out its own
                // defaults (12pt) for what it leaves to the body.
                assert_eq!(g.look.bold, w.look.bold, "{how}: {name} line {k} weight");
                if w.look.font_size_hp.is_some() {
                    assert_eq!(g.look.font_size_hp, w.look.font_size_hp, "{how}: {name} line {k} size");
                }
                if w.look.color.is_some() {
                    assert_eq!(g.look.color, w.look.color, "{how}: {name} line {k} colour");
                }
                if w.look.font_family.is_some() {
                    assert_eq!(g.look.font_family, w.look.font_family, "{how}: {name} line {k} font");
                }
                // A line's tab goes to its stop; Writer may add its Header
                // style's own stops, which a line without a tab has no use
                // for, and ones past it.
                if texts[k].contains('\t') {
                    let stops: Vec<(i64, TabAlign)> = g.tab_stops.iter().map(|t| (t.pos_pt.round() as i64, t.align)).collect();
                    for t in &w.tab_stops {
                        assert!(stops.contains(&(t.pos_pt.round() as i64, t.align)), "{how}: {name} line {k} tab stops {stops:?}");
                    }
                    let first = g.tab_stops.iter().map(|t| t.pos_pt).fold(f64::MAX, f64::min);
                    assert!((first - w.tab_stops[0].pos_pt).abs() < 1.0, "{how}: {name} line {k}'s tab reaches its own stop first: {stops:?}");
                }
            }
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("lines.docx");
    docx::write(&d, &docx_path).expect("write docx");
    close(&docx::read(docx_path.to_str().unwrap()).expect("read our docx"), "our docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("lines.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "lines") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A table row kept on one page stays so through Writer both ways (Word's
/// `w:cantSplit`, ODF's row `fo:keep-together="always"`), and a row not
/// kept so is not made so.
#[test]
fn a_row_kept_whole_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 3, 1);
    d.table_whole_rows.entry(table).or_default().insert(1);
    let close = |got: &Document, how: &str| {
        let id = got.paragraphs.iter().find_map(|p| p.style.table_cell).expect("a table").table;
        let rows: Vec<u32> = got.table_whole_rows.get(&id).map(|r| r.iter().copied().collect()).unwrap_or_default();
        assert_eq!(rows, [1], "{how}: only the middle row is kept whole");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("whole.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("whole.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "whole") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Through Writer both ways, a table sized as a share of the text's width
/// stays one (the transparency return's 100% table), and a table with its
/// own widths, wider than the text, keeps them (the crime supervisor
/// form's, which runs on into the right margin).
#[test]
fn a_tables_width_kind_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("between\nafter");
    let wide = d.insert_table_at(0, 1, 2);
    d.table_columns.insert(wide, vec![108.0, 374.0]);
    let at = d.paragraphs.iter().position(|p| p.text() == "between").unwrap();
    let shared = d.insert_table_at(at + 1, 1, 2);
    d.table_columns.insert(shared, vec![150.0, 270.0]);
    d.table_relative.insert(shared);
    let close = |got: &Document, how: &str| {
        let ids: Vec<u32> = {
            let mut seen = Vec::new();
            for c in got.paragraphs.iter().filter_map(|p| p.style.table_cell) {
                if !seen.contains(&c.table) {
                    seen.push(c.table);
                }
            }
            seen
        };
        assert_eq!(ids.len(), 2, "{how}: two tables");
        assert!(!got.table_relative.contains(&ids[0]), "{how}: the wide table keeps its own widths");
        assert!(got.table_relative.contains(&ids[1]), "{how}: the shared table stays a share of the text");
        let total: f64 = got.table_columns.get(&ids[0]).map_or(0.0, |w| w.iter().sum());
        assert!((total - 482.0).abs() < 1.0, "{how}: wider than the text, as the file has it: {total}");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("widths.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("widths.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "widths") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A classification label floating at the top and foot of the page, out
/// of the header's and footer's flow, through Writer both ways: the crime
/// supervisor form's "OFFICIAL" text boxes, 15pt in from the page's edge.
#[test]
fn floating_header_and_footer_lines_survive_writer_both_ways() {
    use letters_core::{HfFloat, HfLine, RunStyle};
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("body text");
    let label = |float| HfLine { alignment: Alignment::Center, look: RunStyle { font_size_hp: Some(24), ..Default::default() }, float: Some(float), ..Default::default() };
    d.header = Some("OFFICIAL\nThe Crime Contract 2025".into());
    d.header_lines = vec![label(HfFloat::Top(15.0)), HfLine { alignment: Alignment::Center, ..Default::default() }];
    d.footer = Some("OFFICIAL\nOctober 2025".into());
    d.footer_lines = vec![label(HfFloat::Bottom(15.0)), HfLine::default()];
    let close = |got: &Document, how: &str| {
        for (name, text, lines, want_text, want_lines) in [
            ("header", &got.header, &got.header_lines, &d.header, &d.header_lines),
            ("footer", &got.footer, &got.footer_lines, &d.footer, &d.footer_lines),
        ] {
            assert_eq!(text.as_deref().map(str::trim_end), want_text.as_deref(), "{how}: {name} text");
            let floats: Vec<Option<HfFloat>> = lines.iter().map(|l| l.float).collect();
            assert_eq!(floats.len().min(2), 2, "{how}: {name} lines {lines:?}");
            match (floats[0], want_lines[0].float) {
                (Some(HfFloat::Top(a)), Some(HfFloat::Top(b))) | (Some(HfFloat::Bottom(a)), Some(HfFloat::Bottom(b))) => assert!((a - b).abs() < 0.5, "{how}: {name} inset {a}"),
                other => panic!("{how}: {name} label floats {other:?}"),
            }
            assert_eq!(lines[0].alignment, Alignment::Center, "{how}: {name} label across the page");
            assert!(floats[1..].iter().all(Option::is_none), "{how}: {name} only the label floats");
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("float.docx");
    docx::write(&d, &docx_path).expect("write docx");
    close(&docx::read(docx_path.to_str().unwrap()).expect("read our docx"), "our docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("float.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "float") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Keep-lines and widow control across the format boundary, both ways:
/// our docx converted to odt, and our odt converted to docx.
///
/// Writer inverts widow control through an odt-to-docx pass: a paragraph
/// we mark uncontrolled comes back controlled, and an unmarked one comes
/// back uncontrolled (diagnosed from both sides' XML: our `fo:widows="1"`
/// reads back as `<w:widowControl/>`, our unmarked paragraph as
/// `w:val="0"`). Keep-lines survives both ways, and widow control
/// survives everywhere else, including the docx-to-odt pass here — so
/// that leg asserts the values, and the odt-to-docx leg asserts
/// keep-lines only.
#[test]
fn keep_lines_and_widow_control_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("kept together\nmay strand\nordinary");
    d.paragraphs[0].style.keep_lines = true;
    d.paragraphs[1].style.widow_control = Some(false);
    let keep = |got: &Document, how: &str| {
        assert!(para_with_text(got, "kept together").style.keep_lines, "{how}: keep_lines lost");
        assert!(!para_with_text(got, "may strand").style.keep_lines, "{how}: keep_lines spread");
        assert!(!para_with_text(got, "ordinary").style.keep_lines, "{how}: keep_lines spread");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("keep.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("keep.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    let rt = letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt");
    keep(&rt, "docx → Writer → odt");
    let free = &para_with_text(&rt, "may strand").style;
    let plain = &para_with_text(&rt, "ordinary").style;
    assert_eq!(free.widow_control, Some(false), "docx → Writer → odt: widow control lost");
    // Writer materializes inherited control as explicit 2-and-2, which
    // reads as Some(true): behaviorally the default, so the plain
    // paragraph must only not allow widows.
    assert!(plain.widow_control != Some(false), "docx → Writer → odt: plain paragraph gained control: {plain:?}");
    let Some(rt) = through_lo_to_docx(&d, "keep") else { return };
    keep(&rt, "odt → Writer → docx");
}

/// Fixed and minimum line heights across the format boundary, both
/// ways: exact points stay exact, minima stay minima.
#[test]
fn exact_and_minimum_line_heights_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("fixed height\nminimum height\nproportional");
    d.paragraphs[0].style.line_height = Some(LineHeight::Exact(24.0));
    d.paragraphs[1].style.line_height = Some(LineHeight::Minimum(30.0));
    let close = |got: &Document, how: &str| {
        let pt = |p: &Paragraph| match p.style.line_height {
            Some(LineHeight::Exact(v)) | Some(LineHeight::Minimum(v)) => v,
            None => panic!("{how}: line height lost on {:?}", p.text()),
        };
        let fixed = para_with_text(got, "fixed height");
        let minimum = para_with_text(got, "minimum height");
        assert!(matches!(fixed.style.line_height, Some(LineHeight::Exact(_))), "{how}: exact became {:?}", fixed.style.line_height);
        assert!((pt(fixed) - 24.0).abs() < 1.0, "{how}: exact moved: {:?}", fixed.style.line_height);
        assert!(matches!(minimum.style.line_height, Some(LineHeight::Minimum(_))), "{how}: minimum became {:?}", minimum.style.line_height);
        assert!((pt(minimum) - 30.0).abs() < 1.0, "{how}: minimum moved: {:?}", minimum.style.line_height);
        assert_eq!(para_with_text(got, "proportional").style.line_height, None, "{how}: plain paragraph gained a height");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("height.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("height.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "height") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Paragraph shading and borders across the format boundary, both
/// ways: the fill and every edge survive a Writer rewrite.
#[test]
fn paragraph_shading_and_borders_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("shaded and boxed\nordinary");
    d.paragraphs[0].style.shading = Some("D9D9D9".into());
    d.paragraphs[0].style.borders = ParaBorders {
        top: Some(ParaBorder { style: BorderStyle::Single, color: "FF0000".into(), width_pt: 0.75, space_pt: 0.0 }),
        bottom: Some(ParaBorder { style: BorderStyle::Dashed, color: "0000FF".into(), width_pt: 1.0, space_pt: 0.0 }),
        ..Default::default()
    };
    let close = |got: &Document, how: &str| {
        let boxed = &para_with_text(got, "shaded and boxed").style;
        assert_eq!(boxed.shading.as_deref(), Some("D9D9D9"), "{how}: shading lost");
        assert!(boxed.borders.top.is_some(), "{how}: top border lost: {:?}", boxed.borders);
        assert!(boxed.borders.bottom.is_some(), "{how}: bottom border lost: {:?}", boxed.borders);
        let top = boxed.borders.top.as_ref().unwrap();
        assert_eq!((top.style, top.color.as_str()), (BorderStyle::Single, "FF0000"), "{how}: top border changed");
        assert!((top.width_pt - 0.75).abs() < 0.25, "{how}: top width moved: {}", top.width_pt);
        let bottom = boxed.borders.bottom.as_ref().unwrap();
        assert_eq!((bottom.style, bottom.color.as_str()), (BorderStyle::Dashed, "0000FF"), "{how}: bottom border changed");
        assert!(boxed.borders.left.is_none() && boxed.borders.right.is_none(), "{how}: phantom side borders: {:?}", boxed.borders);
        let plain = &para_with_text(got, "ordinary").style;
        assert!(plain.shading.is_none() && plain.borders.is_empty(), "{how}: plain paragraph gained decoration");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("box.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("box.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "box") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A table's cell padding survives Writer both ways: our docx converted to
/// odt (`w:tblCellMar` to `fo:padding`), and our odt converted to docx.
/// The questionnaire's cells are 4pt in from every edge; the expression of
/// interest form's 0.0784in (5.65pt) all round.
#[test]
fn table_cell_padding_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("after the table");
    let table = d.insert_table_at(0, 2, 2);
    for p in d.paragraphs.iter_mut().filter(|p| p.style.table_cell.is_some()) {
        p.runs = vec![Run::plain("cell")];
    }
    let want = letters_core::CellPadding { top_pt: 4.0, bottom_pt: 4.0, left_pt: 5.65, right_pt: 5.65 };
    d.table_padding.insert(table, want);
    let close = |got: &Document, how: &str| {
        let pads: Vec<&letters_core::CellPadding> = got.table_padding.values().collect();
        assert_eq!(pads.len(), 1, "{how}: {:?}", got.table_padding);
        let p = pads[0];
        // Writer keeps lengths in hundredths of a millimetre, Word in twips.
        for (g, w) in [(p.top_pt, want.top_pt), (p.bottom_pt, want.bottom_pt), (p.left_pt, want.left_pt), (p.right_pt, want.right_pt)] {
            assert!((g - w).abs() < 0.1, "{how}: {p:?} for {want:?}");
        }
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("pad.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("pad.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "pad") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A two-section document through Writer both ways (LET-005S): a portrait
/// part with the document's header, then a landscape part in two columns
/// with narrower margins and a header of its own. Each section keeps its
/// page, columns, margins and header, starts where it did, and the pages
/// lay out as the sections say: one portrait page, one landscape.
#[test]
fn multi_section_geometry_survives_writer_both_ways() {
    use letters_core::layout::{layout, LayoutOptions, MonoShaper};
    use letters_core::model::{DocSection, PageGeometry, ParagraphRange};
    let Some(bin) = require_or_skip() else { return };
    let portrait = PageGeometry::default();
    let landscape = PageGeometry {
        width_pt: 841.9,
        height_pt: 595.3,
        margin_top_pt: 54.0,
        margin_bottom_pt: 54.0,
        margin_left_pt: 36.0,
        margin_right_pt: 36.0,
        columns: 2,
        column_gap_pt: 24.0,
        ..Default::default()
    };
    let mut d = Document::from_plain_text("Portrait part.\nLandscape part, first column.\nMore landscape text.");
    d.header = Some("Report".into());
    d.sections = vec![
        DocSection { id: 0, range: ParagraphRange::new(0, 1), geometry: portrait, header: None, footer: None },
        DocSection { id: 1, range: ParagraphRange::new(1, 3), geometry: landscape, header: Some("Appendix".into()), footer: None },
    ];
    d.page = Some(portrait);
    d.next_section_id = 2;
    let check = |got: &Document, how: &str| {
        assert_eq!(got.sections.len(), 2, "{how}: {:?}", got.sections);
        let second = got.paragraphs.iter().position(|p| p.text().starts_with("Landscape part")).expect("the landscape text");
        assert_eq!(got.sections[1].range.start, second, "{how}: the second section starts at its first paragraph");
        assert!(got.sections[0].geometry.approx_eq(&portrait), "{how}: {:?}", got.sections[0].geometry);
        assert!(got.sections[1].geometry.approx_eq(&landscape), "{how}: {:?}", got.sections[1].geometry);
        assert_eq!(got.page, Some(got.sections[0].geometry), "{how}: the page is the first section's");
        assert_eq!(got.next_section_id, 2, "{how}: a file's sections are numbered from 0");
        let header = |k| letters_core::sections::header(got, k).0.map(str::trim).map(str::to_string);
        assert_eq!(header(0).as_deref(), Some("Report"), "{how}");
        assert_eq!(header(1).as_deref(), Some("Appendix"), "{how}");
        let tree = layout(got, &LayoutOptions::default(), &mut MonoShaper);
        let sizes: Vec<(i64, i64)> = tree.pages.iter().map(|p| (p.width_pt.round() as i64, p.height_pt.round() as i64)).collect();
        assert_eq!(sizes, vec![(595, 842), (842, 595)], "{how}: one portrait page, one landscape");
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("sections.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("sections.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    check(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "sections") else { return };
    check(&rt, "odt → Writer → docx");
}

/// A heading's run set back to regular weight stays regular through
/// Writer, both ways: the vessel declaration's Heading 3 cells.
#[test]
fn a_regular_run_in_a_heading_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("Towing Operations\nafter");
    d.paragraphs[0].style.heading = Some(3);
    d.paragraphs[0].runs[0].style.regular = true;
    let close = |got: &Document, how: &str| {
        let p = para_with_text(got, "Towing Operations");
        assert!(p.style.heading.is_some(), "{how}: still a heading");
        assert!(p.runs.iter().all(|r| r.style.regular && !r.style.bold), "{how}: {:?}", p.runs);
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("regular.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("regular.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "regular") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A heading's run set back upright stays upright through Writer, both
/// ways: the W1 notes' "For alcohol", a Heading 4 italic by its style.
#[test]
fn an_upright_run_in_a_heading_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("For alcohol\nafter");
    d.paragraphs[0].style.heading = Some(4);
    d.heading_styles = (1..=6).map(|n| RunStyle { italic: n == 4, ..Default::default() }).collect();
    d.paragraphs[0].runs[0].style.upright = true;
    let close = |got: &Document, how: &str| {
        let p = para_with_text(got, "For alcohol");
        assert!(p.style.heading.is_some(), "{how}: still a heading");
        assert!(p.runs.iter().all(|r| r.style.upright && !r.style.italic), "{how}: {:?}", p.runs);
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("upright.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("upright.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "upright") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Pair kerning through Writer both ways, on the run that asks for it
/// only: Writer and Word draw everything else unkerned, which sets where
/// a line breaks (the audit-committee minutes).
#[test]
fn kerning_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("AVATAR kerned\nplain after");
    d.paragraphs[0].runs[0].style.kerned = true;
    let close = |got: &Document, how: &str| {
        let p = para_with_text(got, "AVATAR kerned");
        assert!(p.runs.iter().all(|r| r.style.kerned), "{how}: {:?}", p.runs);
        assert!(para_with_text(got, "plain after").runs.iter().all(|r| !r.style.kerned), "{how}: only the first");
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("kerned.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("kerned.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "kerned") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Capitals through Writer both ways, the text in its own case: the crime
/// supervisor form's title.
#[test]
fn capitals_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("Crime supervisor standard\nplain after");
    d.paragraphs[0].runs[0].style.caps = true;
    let close = |got: &Document, how: &str| {
        let p = para_with_text(got, "Crime supervisor standard");
        assert!(p.runs.iter().all(|r| r.style.caps), "{how}: {:?}", p.runs);
        assert!(para_with_text(got, "plain after").runs.iter().all(|r| !r.style.caps), "{how}: only the title");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("caps.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("caps.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "caps") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A table cell's bare sides and its place in the row through Writer,
/// both ways: the expression-of-interest form's two-cell header band, and
/// the vessel declaration's labels set in the middle of their rows.
#[test]
fn table_cell_edges_and_valign_survive_writer_both_ways() {
    use letters_core::{CellEdges, CellVAlign, OpenEdges, VAlign};
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("after the table");
    let table = d.insert_table_at(0, 2, 2);
    for p in d.paragraphs.iter_mut().filter(|p| p.style.table_cell.is_some()) {
        p.runs = vec![Run::plain("cell")];
    }
    let edges = vec![
        CellEdges { row: 0, col: 0, open: OpenEdges { right: true, ..Default::default() } },
        CellEdges { row: 0, col: 1, open: OpenEdges { left: true, ..Default::default() } },
    ];
    let valign = vec![CellVAlign { row: 1, col: 0, align: VAlign::Center }, CellVAlign { row: 1, col: 1, align: VAlign::Bottom }];
    d.table_open_edges.insert(table, edges.clone());
    d.table_valign.insert(table, valign.clone());
    let close = |got: &Document, how: &str| {
        assert_eq!(got.table_open_edges.values().collect::<Vec<_>>(), [&edges], "{how}");
        assert_eq!(got.table_valign.values().collect::<Vec<_>>(), [&valign], "{how}");
    };

    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("cells.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("cells.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");

    let Some(rt) = through_lo_to_docx(&d, "cells") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Our docx rewritten by Writer as docx, into a directory of its own (it
/// will not convert a file onto itself), read back.
fn writer_rewrites_docx(bin: &str, d: &Document, stem: &str) -> Document {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("in").join(format!("{stem}.docx"));
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    docx::write(d, &src).expect("write docx");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let st = std::process::Command::new(bin)
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", dir.path().join("p").display()))
        .args(["--convert-to", "docx:MS Word 2007 XML", "--outdir"])
        .arg(&out_dir)
        .arg(&src)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let rewritten = out_dir.join(format!("{stem}.docx"));
    assert_ne!(std::fs::read(&rewritten).unwrap(), std::fs::read(&src).unwrap(), "Writer did not rewrite the docx");
    docx::read(rewritten.to_str().unwrap()).expect("read Writer's docx")
}

/// A numbered paragraph's marker hangs as far as the file says through a
/// Writer rewrite: the audit-committee minutes' "1.1" items hang their
/// number 0.5in before the text, not Letters' own 18pt.
#[test]
fn a_list_markers_hanging_survives_writer() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("Linda Farrant opened the meeting\nMembers accepted the minutes");
    for p in &mut d.paragraphs {
        p.style.list = letters_core::ListKind::Numbered;
        p.style.list_hanging_pt = Some(36.0);
    }
    let rt = writer_rewrites_docx(bin, &d, "hanging");
    for p in &rt.paragraphs {
        let h = p.style.list_hanging_pt.expect("the hanging comes back");
        assert!((h - 36.0).abs() < 0.5, "hanging {h}");
    }
}

/// An inline picture's room beside it, through Writer both ways: a .docx
/// picture that leaves out `distL`/`distR` gets Writer's own frame
/// margins in an odt. Writer's .docx of a frame with margins writes
/// `distL="0"` and draws it at the margin (measured), so none comes back
/// that way: the format cannot carry it out of Writer.
#[test]
fn an_inline_pictures_room_beside_it_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xf1,
        0x4d, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x3c, 0x21, 0x27, 0xc7,
        0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc4, 0x00, 0x03, 0x00, 0x13, 0x2e, 0x01, 0x08, 0x6a, 0xc0,
        0x65, 0x61, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let png_path = dir.path().join("p.png");
    std::fs::write(&png_path, png).unwrap();
    let mut d = Document::from_plain_text("caption");
    d.paragraphs.push(Paragraph {
        runs: vec![Run {
            text: String::new(),
            style: RunStyle { image: Some(png_path.to_string_lossy().into()), image_extent_emu: Some((914_400, 914_400)), image_sides_emu: Some((114_840, 114_840)), ..Default::default() },
        }],
        ..Default::default()
    });
    let close = |got: &Document, how: &str| {
        let sides = got.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.style.image.is_some()).and_then(|r| r.style.image_sides_emu);
        let (l, r) = sides.unwrap_or_else(|| panic!("{how}: no room beside the picture"));
        // Writer keeps lengths in hundredths of a millimetre.
        assert!(l.abs_diff(114_840) <= 360 && r.abs_diff(114_840) <= 360, "{how}: {l} {r}");
    };
    let docx_path = dir.path().join("sides.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("sides.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "sides") else { return };
    let sides = rt.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.style.image.is_some()).map(|r| r.style.image_sides_emu);
    assert_eq!(sides, Some(None), "odt → Writer → docx: Writer's own .docx keeps no room");
}

/// Paragraph spacing adds up in an ODF document and takes the larger in a
/// Word one, through Writer both ways: Writer spaces 30pt after and 40pt
/// before 70pt apart in an odt, whatever its settings, and 40pt in a
/// docx.
#[test]
fn paragraph_spacing_follows_the_format_through_writer() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("First\nSecond");
    d.paragraphs[0].style.space_after_pt = 30.0;
    d.paragraphs[1].style.space_before_pt = 40.0;
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("spacing.docx");
    docx::write(&d, &docx_path).expect("write docx");
    assert!(!docx::read(docx_path.to_str().unwrap()).expect("read our docx").space_adds, "a Word file takes the larger");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("spacing.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    let odt = letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt");
    assert!(odt.space_adds, "docx → Writer → odt adds up");
    let (after, before) = (odt.paragraphs[0].style.space_after_pt, odt.paragraphs[1].style.space_before_pt);
    assert!((after - 30.0).abs() < 0.1 && (before - 40.0).abs() < 0.1, "the spacing itself: {after} {before}");
    let Some(rt) = through_lo_to_docx(&d, "spacing") else { return };
    assert!(!rt.space_adds, "odt → Writer → docx takes the larger");
}

/// The first page's own header and footer, and the spacing of a header's
/// lines, through Writer both ways: the screening committee form's
/// letterhead over blank lines with Normal's space after, on its first
/// page only, and a running title on the others.
#[test]
fn the_first_pages_own_header_survives_writer_both_ways() {
    use letters_core::{HfLine, HfStory};
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xf1,
        0x4d, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x3c, 0x21, 0x27, 0xc7,
        0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc4, 0x00, 0x03, 0x00, 0x13, 0x2e, 0x01, 0x08, 0x6a, 0xc0,
        0x65, 0x61, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let png_path = dir.path().join("logo.png");
    std::fs::write(&png_path, png).unwrap();
    let logo = Run { text: "Committee".into(), style: RunStyle { image: Some(png_path.to_string_lossy().into()), image_extent_emu: Some((1_304_925, 542_925)), ..Default::default() } };
    let mut d = Document::from_plain_text("body text");
    d.header = Some("Running title".into());
    d.footer = Some("{page}".into());
    let spaced = HfLine { space_after_pt: 12.0, line_height: Some(LineHeight::Exact(10.0)), ..Default::default() };
    d.first_header = Some(HfStory { text: "Letterhead\n\n".into(), pictures: vec![logo], alignment: Alignment::Center, lines: vec![HfLine { alignment: Alignment::Center, ..spaced.clone() }, spaced.clone(), spaced] });
    d.first_footer = Some(HfStory { text: "Page {page}".into(), ..Default::default() });
    let close = |got: &Document, how: &str| {
        assert_eq!(got.header.as_deref().map(str::trim_end), Some("Running title"), "{how}: running header");
        let first = got.first_header.as_ref().unwrap_or_else(|| panic!("{how}: no first page header"));
        assert_eq!(first.text.trim_end(), "Letterhead", "{how}: first page header text");
        assert_eq!((first.alignment, first.lines.first().map(|l| l.alignment)), (Alignment::Center, Some(Alignment::Center)), "{how}: centred");
        assert_eq!(first.pictures.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["Committee"], "{how}: its logo");
        assert!(got.header_pictures.is_empty(), "{how}: the logo is the first page's alone");
        assert!(first.lines.len() >= 3, "{how}: its blank lines {:?}", first.lines);
        for l in &first.lines[..3] {
            assert!((l.space_after_pt - 12.0).abs() < 0.1, "{how}: space after {l:?}");
            assert!(matches!(l.line_height, Some(LineHeight::Exact(h)) if (h - 10.0).abs() < 0.1), "{how}: exact line {l:?}");
        }
        assert_eq!(got.first_footer.as_ref().map(|s| s.text.trim_end()), Some("Page {page}"), "{how}: first page footer");
    };
    let docx_path = dir.path().join("first.docx");
    docx::write(&d, &docx_path).expect("write docx");
    close(&docx::read(docx_path.to_str().unwrap()).expect("read our docx"), "our docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("first.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "first") else { return };
    close(&rt, "odt → Writer → docx");
}

/// A line break within a paragraph, and a span's face, through Writer
/// both ways: the statistical release's two-line Verdana title. Writer
/// keeps the break as `text:line-break` in an odt and `w:br` in a docx,
/// and names a span's face by its font declaration in an odt.
#[test]
fn a_line_break_and_a_spans_face_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![Run { text: "Statistical Release\nOctober 2020".into(), style: RunStyle { font_family: Some("Verdana".into()), ..Default::default() } }];
    let close = |got: &Document, how: &str| {
        let text: String = got.paragraphs.iter().flat_map(|p| &p.runs).map(|r| r.text.as_str()).collect();
        assert_eq!(text.trim_end(), "Statistical Release\nOctober 2020", "{how}: the break");
        let run = got.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.text.contains("October")).expect("the title");
        assert_eq!(run.style.font_family.as_deref(), Some("Verdana"), "{how}: the face");
    };
    let dir = tempfile::tempdir().unwrap();
    let docx_path = dir.path().join("brk.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("brk.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    let Some(rt) = through_lo_to_docx(&d, "brk") else { return };
    close(&rt, "odt → Writer → docx");
}

/// Text raised or lowered at its own size (`w:position`) survives Writer
/// rewriting the .docx, and is not read as a superscript or subscript:
/// the business appointment rules' Normal lowers every run half a point.
#[test]
fn a_runs_raised_or_lowered_position_survives_writer() {
    let Some(bin) = require_or_skip() else { return };
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![
        Run::plain("level "),
        Run { text: "lowered".into(), style: RunStyle { position_hp: Some(-2), ..Default::default() } },
        Run { text: " raised".into(), style: RunStyle { position_hp: Some(6), ..Default::default() } },
    ];
    let rt = writer_rewrites_docx(bin, &d, "position");
    let of = |t: &str| rt.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.text.contains(t)).map(|r| (r.style.position_hp, r.style.vert_align));
    assert_eq!(of("lowered"), Some((Some(-2), None)));
    assert_eq!(of("raised"), Some((Some(6), None)));
    assert_eq!(of("level"), Some((None, None)));
}

/// A page's own header and footer distances (`w:pgMar w:header` and
/// `w:footer`) cross Writer both ways: in odt they are the page margins
/// the header and footer areas grow from.
#[test]
fn a_pages_header_and_footer_distances_survive_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut d = Document::from_plain_text("body text");
    d.header = Some("Head".into());
    d.footer = Some("Foot".into());
    d.page = Some(PageGeometry { margin_top_pt: 72.0, margin_bottom_pt: 72.0, header_pt: Some(20.0), footer_pt: Some(25.0), ..PageGeometry::default() });
    let close = |got: &Document, how: &str| {
        let page = got.page.unwrap_or_else(|| panic!("{how}: no page"));
        let near = |v: Option<f64>, want: f64| v.is_some_and(|v| (v - want).abs() < 0.5);
        assert!(near(page.header_pt, 20.0) && near(page.footer_pt, 25.0), "{how}: {page:?}");
        assert!((page.margin_top_pt - 72.0).abs() < 0.5 && (page.margin_bottom_pt - 72.0).abs() < 0.5, "{how}: the body's margins {page:?}");
    };
    let docx_path = dir.path().join("distance.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("distance.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    close(&writer_rewrites_docx(bin, &d, "distance"), "docx → Writer → docx");
}

/// A table's indent crosses Writer both ways: Word's `w:tblInd` under
/// the 2013 rules our writer names, ODF's table `fo:margin-left`, each
/// with the left padding between the table's edge and its text.
#[test]
fn a_tables_indent_survives_writer_both_ways() {
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut d = Document::from_plain_text("after");
    let table = d.insert_table_at(0, 1, 2);
    d.paragraphs[0].runs = vec![Run::plain("indented")];
    d.table_indent.insert(table, 30.0);
    let close = |got: &Document, want: f64, how: &str| {
        let indent: Vec<f64> = got.table_indent.values().copied().collect();
        assert!(indent.len() == 1 && (indent[0] - want).abs() < 0.5, "{how}: {indent:?}");
    };
    let docx_path = dir.path().join("indent.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("indent.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    close(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), 30.0, "docx → Writer → odt");
    // Writer saves a docx as compatibility mode 12 but writes the table's
    // edge as `w:tblInd`, which that mode reads as its text's: Writer's own
    // layout of its file has the table a padding further left, and so
    // does ours.
    close(&writer_rewrites_docx(bin, &d, "indent"), 30.0 - 5.4, "docx → Writer → docx");
}

/// A page border crosses Writer both ways, the text staying where the
/// margins put it: Word's `w:pgBorders` and ODF's page `fo:border`, each
/// with the padding between the rule and the text.
#[test]
fn a_page_border_survives_writer_both_ways() {
    use letters_core::model::{PageBorder, PageGeometry};
    let Some(bin) = require_or_skip() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut d = Document::from_plain_text("bordered");
    let want = PageGeometry {
        border: Some(PageBorder { width_pt: 0.5, color: 0x00_00_80, padding_pt: [2.0, 4.0, 2.0, 4.0] }),
        ..PageGeometry::default()
    };
    d.page = Some(want);
    let check = |got: &Document, how: &str| {
        let pg = got.page.unwrap_or_else(|| panic!("{how}: the page geometry is lost"));
        let b = pg.border.unwrap_or_else(|| panic!("{how}: the page border is lost: {pg:?}"));
        let w = want.border.unwrap();
        let close = |a: f64, b: f64| (a - b).abs() < 0.6;
        assert!(close(b.width_pt, w.width_pt) && b.color == w.color, "{how}: {b:?}");
        assert!(b.padding_pt.iter().zip(w.padding_pt).all(|(a, b)| close(*a, b)), "{how}: {b:?}");
        assert!(pg.approx_eq(&want), "{how}: the text moved: {pg:?}");
    };
    let docx_path = dir.path().join("border.docx");
    docx::write(&d, &docx_path).expect("write docx");
    let _ = soffice_convert(bin, &docx_path, "odt").ok();
    let odt_path = dir.path().join("border.odt");
    assert!(odt_path.exists(), "soffice did not convert docx to odt");
    check(&letters_core::odt::read(odt_path.to_str().unwrap()).expect("read converted odt"), "docx → Writer → odt");
    check(&through_lo_to_docx(&d, "border").expect("Writer"), "odt → Writer → docx");
}

/// A text box survives Writer every way: our .odt or .docx, rewritten by
/// Writer as either, reopens with the box at its paragraph, the same size,
/// place, rule, fill, padding and text, and its text kept out of the body.
#[test]
fn a_text_box_survives_writer_both_ways() {
    use letters_core::model::{AnchorAlign, AnchorFrame, BoxRule, ImageAnchor, TextBox};
    let Some(bin) = require_or_skip() else { return };
    let anchor = ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, y_emu: 91_440, ..Default::default() };
    let tb = TextBox {
        extent_emu: (1_828_800, 457_200),
        border: Some(BoxRule { width_emu: 12_700, color: "1F3864".into() }),
        fill: Some("DEEAF6".into()),
        padding_emu: [45_720, 91_440, 45_720, 91_440],
        text: "Name & date\nsigned".into(),
    };
    let mut d = Document::from_plain_text("after");
    d.paragraphs.insert(0, Paragraph {
        style: Default::default(),
        runs: vec![Run::plain("Sign here"), Run { text: String::new(), style: RunStyle { text_box: Some(Box::new(tb.clone())), image_anchor: Some(anchor), ..Default::default() } }],
    });
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [("odt", "odt"), ("docx", "docx"), ("docx", "odt"), ("odt", "docx")] {
        let how = format!("{from} -> Writer -> {to}");
        let work = dir.path().join(format!("{from}-{to}"));
        let out = work.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let staged = work.join(format!("box.{from}"));
        match from {
            "docx" => docx::write(&d, &staged).expect("write docx"),
            _ => letters_core::odt::write(&d, &staged).expect("write odt"),
        }
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        // Into a directory of its own: onto the input itself Writer writes
        // nothing.
        let st = Command::new(bin)
            .arg("--headless")
            .arg(format!("-env:UserInstallation=file://{}", work.join("lo").display()))
            .args(["--convert-to", filter, "--outdir"])
            .arg(&out)
            .arg(&staged)
            .output()
            .expect("soffice");
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
        let converted = out.join(format!("box.{to}"));
        let rt = match to {
            "docx" => docx::read(converted.to_str().unwrap()).expect("read Writer's docx"),
            _ => letters_core::odt::read(converted.to_str().unwrap()).expect("read Writer's odt"),
        };
        let texts: Vec<String> = rt.paragraphs.iter().map(|p| p.text()).collect();
        assert_eq!(texts, ["Sign here", "after"], "{how}: the box's text stays in the box");
        let got = rt.paragraphs[0].runs.iter().find(|r| r.style.text_box.is_some()).unwrap_or_else(|| panic!("{how}: the box is lost"));
        let g = got.style.text_box.as_deref().unwrap();
        // Writer keeps lengths in hundredths of a millimetre (360 EMU) and
        // rounds them again in inches: within two of them.
        let near = |a: u64, b: u64| a.abs_diff(b) <= 720;
        assert!(near(g.extent_emu.0, tb.extent_emu.0) && near(g.extent_emu.1, tb.extent_emu.1), "{how}: size {:?}", g.extent_emu);
        assert!(g.padding_emu.iter().zip(tb.padding_emu).all(|(a, b)| near(*a as u64, b as u64)), "{how}: padding {:?}", g.padding_emu);
        let rule = g.border.as_ref().unwrap_or_else(|| panic!("{how}: its rule"));
        assert!(near(rule.width_emu as u64, 12_700) && rule.color == "1F3864", "{how}: rule {rule:?}");
        assert_eq!((g.fill.as_deref(), g.text.as_str()), (Some("DEEAF6"), "Name & date\nsigned"), "{how}");
        let a = got.style.image_anchor.unwrap_or_else(|| panic!("{how}: its place"));
        assert_eq!((a.h_from, a.h_align, a.v_from), (AnchorFrame::Margin, Some(AnchorAlign::End), AnchorFrame::Text), "{how}: {a:?}");
        assert!(near(a.y_emu.unsigned_abs(), 91_440), "{how}: {a:?}");
    }
}
