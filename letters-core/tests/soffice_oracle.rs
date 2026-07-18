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
