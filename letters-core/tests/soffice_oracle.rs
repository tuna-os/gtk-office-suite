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
    let has_media = z.file_names().any(|n| n.starts_with("word/media/"));
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
    d.paragraphs[0].style.tab_stops_pt = vec![36.0, 108.0];
    let dir = tempfile::tempdir().unwrap();

    let near = |got: &[f64], want: &[f64], what: &str| {
        assert_eq!(got.len(), want.len(), "{what}: {got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1.0, "{what}: {got:?}");
        }
    };

    // odt -> Writer -> docx
    let a = dir.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    let op = a.join("x.odt");
    letters_core::odt::write(&d, op.to_str().unwrap()).expect("write odt");
    let _ = soffice_convert(bin, &op, "docx:MS Word 2007 XML").ok();
    let dp = a.join("x.docx");
    assert!(dp.exists(), "soffice did not convert the odt");
    let rt = docx::read(dp.to_str().unwrap()).expect("read converted docx");
    near(
        &para_with_text(&rt, "tabbed across formats").style.tab_stops_pt,
        &[36.0, 108.0],
        "odt->docx tab stops",
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
    near(
        &para_with_text(&rt, "tabbed across formats").style.tab_stops_pt,
        &[36.0, 108.0],
        "docx->odt tab stops",
    );
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
fn changes_of(doc: &Document) -> Vec<(RevisionKind, String, String, String)> {
    let mut out: Vec<(RevisionKind, String, String, String)> = Vec::new();
    for c in letters_core::track::changes(doc) {
        let date = c.revision.date.chars().take(19).collect::<String>();
        match out.last_mut() {
            Some(last) if last.0 == c.revision.kind && last.1 == c.revision.author => last.2.push_str(&c.text),
            _ => out.push((c.revision.kind, c.revision.author.clone(), c.text.clone(), date)),
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
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [("docx", "docx"), ("odt", "odt"), ("docx", "odt"), ("odt", "docx")] {
        let work = dir.path().join(format!("{from}-{to}"));
        let out = work.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let staged = out.join(format!("tracked.{from}"));
        match from {
            "docx" => docx::write(&d, &staged).expect("write docx"),
            _ => letters_core::odt::write(&d, &staged).expect("write odt"),
        }
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        let _ = soffice_convert(bin, &staged, filter);
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
        let staged = out.join(format!("commented.{from}"));
        match from {
            "docx" => docx::write(&d, &staged).expect("write docx"),
            _ => letters_core::odt::write(&d, &staged).expect("write odt"),
        }
        let filter = if to == "docx" { "docx:MS Word 2007 XML" } else { "odt" };
        let _ = soffice_convert(bin, &staged, filter);
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
