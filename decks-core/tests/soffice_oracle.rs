// LibreOffice Impress interop oracle for Decks (see PLAN.md: oracle, not
// port). Gate: every .pptx we write must be importable by Impress —
// conversion to ODP must succeed and produce a file. Skips without
// soffice unless REQUIRE_SOFFICE=1 (CI sets it).

use std::process::Command;

use decks_core::engine::{write_pptx, Deck, SlideObject};

fn soffice_available() -> bool {
    Command::new("soffice").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn require_or_skip() -> bool {
    if soffice_available() { return true; }
    if std::env::var("REQUIRE_SOFFICE").is_ok() {
        panic!("REQUIRE_SOFFICE set but no soffice binary found");
    }
    eprintln!("skipping: soffice not installed");
    false
}

fn convert(input: &std::path::Path, to: &str) -> Result<std::path::PathBuf, String> {
    let dir = input.parent().unwrap();
    let profile = dir.join("lo-profile");
    // Convert into a subdirectory, never alongside the input. A same-format
    // rewrite (pptx -> pptx, used by the master-slide test) would otherwise
    // ask LibreOffice to write its output over the file it is reading, which
    // some versions tolerate and others refuse outright with
    // "SfxBaseModel::impl_store ... Error Area:Sfx Class:Write". That made the
    // suite pass or fail depending on the installed LibreOffice rather than on
    // this project's code.
    let out_dir = dir.join(format!("converted-{to}"));
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let out = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", to, "--outdir"])
        .arg(&out_dir)
        .arg(input)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("soffice failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let converted = out_dir
        .join(input.file_name().ok_or("input has no file name")?)
        .with_extension(to);
    if converted.exists() {
        Ok(converted)
    } else {
        Err(format!(
            "no output produced (stdout: {} stderr: {})",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

fn sample_deck() -> Deck {
    let mut deck = Deck::new();
    deck.slides[0].title = "Oracle Slide".into();
    deck.slides[0].objects.push(SlideObject::TextBox {
        text: "Impress must read this".into(),
        x: 100.0, y: 100.0, w: 400.0, h: 60.0,
        rotation: 0.0,
        runs: vec![],
        body: Default::default(),
    });
    deck.slides[0].objects.push(SlideObject::Rect { x: 50.0, y: 250.0, w: 200.0, h: 90.0, rotation: 0.0 });
    deck.slides[0].notes = "speaker notes body".into();
    deck
}

#[test]
fn impress_imports_our_pptx() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ours.pptx");
    write_pptx(path.to_str().unwrap(), &sample_deck()).expect("write pptx");

    let odp = convert(&path, "odp").expect("Impress could not import our .pptx");
    let meta = std::fs::metadata(&odp).unwrap();
    assert!(meta.len() > 0, "empty odp produced");
}

#[test]
fn impress_survives_multi_slide_deck() {
    if !require_or_skip() { return; }
    let mut deck = sample_deck();
    for i in 2..=5 {
        deck.slides.push(decks_core::engine::Slide {
            title: format!("Slide {}", i),
            background: "#ffffff".into(),
            objects: vec![SlideObject::TextBox {
                text: format!("content {}", i),
                x: 80.0, y: 120.0, w: 300.0, h: 50.0,
                rotation: 0.0,
                runs: vec![],
                body: Default::default(),
            }],
            notes: String::new(),
            master_idx: Some(0),
            transition: Default::default(),
            builds: Vec::new(),
            ids: Default::default(),
            layout: None,
        });
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multi.pptx");
    write_pptx(path.to_str().unwrap(), &deck).expect("write pptx");
    convert(&path, "odp").expect("Impress could not import multi-slide pptx");
}

// ── Oracle wave 2: fidelity through an Impress rewrite (TDD) ─────────

use decks_core::engine::{read_pptx, Slide};

/// Our pptx → Impress rewrites it as pptx → our reader. None = skipped.
fn through_impress(deck: &Deck, stem: &str) -> Option<Deck> {
    if !require_or_skip() { return None; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{stem}.pptx"));
    write_pptx(path.to_str().unwrap(), deck).expect("write pptx");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let profile = dir.path().join("prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "pptx", "--outdir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .expect("soffice runs");
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let rewritten = out_dir.join(format!("{stem}.pptx"));
    assert!(rewritten.exists(), "Impress produced no pptx");
    Some(read_pptx(rewritten.to_str().unwrap()).expect("we failed to read Impress pptx"))
}

fn text_slide(title: &str, text: &str, notes: &str) -> Slide {
    Slide {
        title: title.into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::TextBox {
            text: text.into(),
            x: 100.0, y: 100.0, w: 500.0, h: 60.0,
            runs: vec![],
            rotation: 0.0,
            body: Default::default(),
        }],
        notes: notes.into(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }
}

fn all_text(slide: &Slide) -> String {
    slide
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::TextBox { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn text_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "hello from decks", "")];
    let Some(rt) = through_impress(&deck, "text") else { return };
    assert!(
        all_text(&rt.slides[0]).contains("hello from decks"),
        "text lost: {:?}",
        rt.slides[0].objects
    );
}

#[test]
fn unicode_text_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "héllo — 中文 ✨", "")];
    let Some(rt) = through_impress(&deck, "uni") else { return };
    // Impress inserts soft line breaks on rewrite; compare content only.
    let text = all_text(&rt.slides[0]).replace('\n', " ");
    assert!(text.contains("héllo — 中文 ✨"), "unicode lost: {text:?}");
}

#[test]
fn notes_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "body", "remember the demo")];
    let Some(rt) = through_impress(&deck, "notes") else { return };
    assert!(
        rt.slides[0].notes.contains("remember the demo"),
        "notes lost: {:?}",
        rt.slides[0].notes
    );
}

#[test]
fn notes_unicode_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "body", "café — 東京")];
    let Some(rt) = through_impress(&deck, "notesuni") else { return };
    let notes = rt.slides[0].notes.replace('\n', " ");
    assert!(notes.contains("café — 東京"), "{notes:?}");
}

#[test]
fn slide_count_and_order_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = (1..=4)
        .map(|i| text_slide(&format!("S{i}"), &format!("content {i}"), ""))
        .collect();
    let Some(rt) = through_impress(&deck, "order") else { return };
    assert_eq!(rt.slides.len(), 4, "slide count changed");
    for (i, slide) in rt.slides.iter().enumerate() {
        assert!(
            all_text(slide).contains(&format!("content {}", i + 1)),
            "slide {} out of order: {:?}",
            i,
            all_text(slide)
        );
    }
}

/// A rectangle as either model form: the editor's older `Rect`, or the
/// `Shape` a file reads back as.
fn is_rect(o: &SlideObject) -> bool {
    matches!(o, SlideObject::Rect { .. } | SlideObject::Shape { kind: decks_core::engine::shape::ShapeKind::Rect, .. })
}

/// An ellipse as either model form (`Circle`, or an ellipse `Shape`).
fn is_ellipse(o: &SlideObject) -> bool {
    matches!(o, SlideObject::Circle { .. } | SlideObject::Shape { kind: decks_core::engine::shape::ShapeKind::Ellipse, .. })
}

#[test]
fn shape_kinds_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "shapes".into(),
        background: "#ffffff".into(),
        objects: vec![
            SlideObject::Rect { x: 100.0, y: 100.0, w: 200.0, h: 100.0, rotation: 0.0 },
            SlideObject::Circle { x: 500.0, y: 300.0, r: 80.0, rotation: 0.0 },
        ],
        notes: String::new(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let Some(rt) = through_impress(&deck, "shapes") else { return };
    let rects = rt.slides[0].objects.iter().filter(|o| is_rect(o)).count();
    let circles = rt.slides[0].objects.iter().filter(|o| is_ellipse(o)).count();
    assert!(rects >= 1, "rect lost: {:?}", rt.slides[0].objects);
    assert!(circles >= 1, "circle lost: {:?}", rt.slides[0].objects);
}

#[test]
fn positions_approx_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "pos".into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::Rect { x: 240.0, y: 180.0, w: 320.0, h: 120.0, rotation: 0.0 }],
        notes: String::new(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let Some(rt) = through_impress(&deck, "pos") else { return };
    let Some((x, y, w, h)) = rt.slides[0]
        .objects
        .iter()
        .find(|o| is_rect(o))
        .map(decks_core::undo::obj_bounds)
    else {
        panic!("rect lost: {:?}", rt.slides[0].objects)
    };
    let (x, y, w, h) = (&x, &y, &w, &h);
    // EMU rounding through two converters: half-a-percent tolerance.
    let close = |a: f64, b: f64| (a - b).abs() < 6.0;
    assert!(close(*x, 240.0) && close(*y, 180.0), "position drifted: {x},{y}");
    assert!(close(*w, 320.0) && close(*h, 120.0), "size drifted: {w}x{h}");
}

#[test]
fn multiline_text_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "first line\nsecond line", "")];
    let Some(rt) = through_impress(&deck, "multiline") else { return };
    let text = all_text(&rt.slides[0]);
    assert!(
        text.contains("first line") && text.contains("second line"),
        "lines lost: {text:?}"
    );
}

#[test]
fn empty_slide_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![
        text_slide("one", "content", ""),
        Slide {
            title: "empty".into(),
            background: "#ffffff".into(),
            objects: vec![],
            notes: String::new(),
            master_idx: Some(0),
            transition: Default::default(),
            builds: Vec::new(),
            ids: Default::default(),
            layout: None,
        },
        text_slide("three", "more", ""),
    ];
    let Some(rt) = through_impress(&deck, "empty") else { return };
    assert_eq!(rt.slides.len(), 3, "empty slide dropped");
}

#[test]
fn bold_run_survives_impress_rewrite() {
    use letters_core::model::{Run, RunStyle};
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "styled".into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::TextBox {
            text: "plain bolded".into(),
            x: 100.0, y: 100.0, w: 500.0, h: 60.0,
            runs: vec![
                Run { text: "plain ".into(), style: RunStyle::default() },
                Run {
                    text: "bolded".into(),
                    style: RunStyle { bold: true, ..Default::default() },
                },
            ],
            rotation: 0.0,
            body: Default::default(),
        }],
        notes: String::new(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let Some(rt) = through_impress(&deck, "boldrun") else { return };
    let bold_text: String = rt.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::TextBox { runs, .. } => Some(
                runs.iter()
                    .filter(|r| r.style.bold)
                    .map(|r| r.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(bold_text.trim(), "bolded", "bold run lost: {:?}", rt.slides[0].objects);
}

#[test]
fn large_deck_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = (0..20).map(|i| text_slide(&format!("S{i}"), &format!("t{i}"), "")).collect();
    let Some(rt) = through_impress(&deck, "large") else { return };
    assert_eq!(rt.slides.len(), 20, "slides lost");
}

#[test]
fn deck_converts_to_pdf_with_page_structure() {
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    deck.slides = (0..8).map(|i| text_slide(&format!("S{i}"), &format!("t{i}"), "")).collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pdf.pptx");
    write_pptx(path.to_str().unwrap(), &deck).expect("write pptx");
    let pdf = convert(&path, "pdf").expect("Impress could not render to pdf");
    let bytes = std::fs::read(&pdf).unwrap();
    let pages = bytes.windows(6).filter(|w| w == b"/Page\x20" || w == b"/Page/").count();
    assert!(bytes.len() > 1000, "suspiciously small pdf");
    // Count the page objects; layouts differ, so just require >= slides.
    let count_marker = bytes.windows(7).filter(|w| w == b"/Count " ).count();
    assert!(pages > 0 || count_marker > 0, "no page structure in pdf");
}

#[test]
fn background_color_survives_impress_rewrite() {
    let mut deck = Deck::new();
    let mut s = text_slide("bg", "colored", "");
    s.background = "#e8f0fe".into();
    deck.slides = vec![s];
    let Some(rt) = through_impress(&deck, "bg") else { return };
    assert_eq!(
        rt.slides[0].background.to_lowercase(),
        "#e8f0fe",
        "background lost: {:?}",
        rt.slides[0].background
    );
}

// ── Oracle wave 3: run styling, images, notes mapping ────────────────

use letters_core::model::{Run, RunStyle};

fn styled_run_slide(runs: Vec<Run>) -> Slide {
    let text = runs.iter().map(|r| r.text.as_str()).collect::<String>();
    Slide {
        title: "styled".into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::TextBox {
            text,
            x: 100.0, y: 100.0, w: 600.0, h: 80.0,
            runs,
            rotation: 0.0,
            body: Default::default(),
        }],
        notes: String::new(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }
}

fn runs_of(slide: &Slide) -> Vec<Run> {
    slide
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::TextBox { runs, .. } => Some(runs.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn italic_underline_runs_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![styled_run_slide(vec![
        Run { text: "it".into(), style: RunStyle { italic: true, ..Default::default() } },
        Run { text: " and ".into(), style: RunStyle::default() },
        Run { text: "un".into(), style: RunStyle { underline: true, ..Default::default() } },
    ])];
    let Some(rt) = through_impress(&deck, "itun") else { return };
    let runs = runs_of(&rt.slides[0]);
    let italic: String =
        runs.iter().filter(|r| r.style.italic).map(|r| r.text.as_str()).collect();
    let underline: String =
        runs.iter().filter(|r| r.style.underline).map(|r| r.text.as_str()).collect();
    assert_eq!(italic.trim(), "it", "italic lost: {runs:?}");
    assert_eq!(underline.trim(), "un", "underline lost: {runs:?}");
}

#[test]
fn run_font_size_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![styled_run_slide(vec![
        Run { text: "small".into(), style: RunStyle::default() },
        Run {
            text: "BIG".into(),
            style: RunStyle { font_size_hp: Some(64), ..Default::default() }, // 32pt
        },
    ])];
    let Some(rt) = through_impress(&deck, "fontsize") else { return };
    let runs = runs_of(&rt.slides[0]);
    let big = runs.iter().find(|r| r.text.contains("BIG")).expect("BIG run lost");
    assert_eq!(big.style.font_size_hp, Some(64), "font size lost: {runs:?}");
}

#[test]
fn run_color_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![styled_run_slide(vec![Run {
        text: "red text".into(),
        style: RunStyle { color: Some("cc0000".into()), ..Default::default() },
    }])];
    let Some(rt) = through_impress(&deck, "runcolor") else { return };
    let runs = runs_of(&rt.slides[0]);
    let red = runs.iter().find(|r| r.text.contains("red")).expect("run lost");
    assert_eq!(
        red.style.color.as_deref().map(str::to_lowercase),
        Some("cc0000".into()),
        "color lost: {runs:?}"
    );
}

#[test]
fn image_object_survives_impress_rewrite() {
    if !require_or_skip() { return; }
    // A 2x2 red PNG, generated inline so no fixture file is needed.
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

    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "img".into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::Image {
            path: png_path.to_string_lossy().to_string(),
            x: 100.0, y: 100.0, w: 200.0, h: 150.0,
            rotation: 0.0, crop: Default::default()
        }],
        notes: String::new(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let Some(rt) = through_impress(&deck, "image") else { return };
    let images = rt.slides[0]
        .objects
        .iter()
        .filter(|o| matches!(o, SlideObject::Image { .. }))
        .count();
    assert!(images >= 1, "image object lost: {:?}", rt.slides[0].objects);
}

#[test]
fn notes_map_to_their_slides_through_impress() {
    let mut deck = Deck::new();
    deck.slides = (1..=3)
        .map(|i| text_slide(&format!("S{i}"), &format!("body {i}"), &format!("note {i}")))
        .collect();
    let Some(rt) = through_impress(&deck, "notesmap") else { return };
    for (i, slide) in rt.slides.iter().enumerate() {
        assert!(
            slide.notes.contains(&format!("note {}", i + 1)),
            "slide {} has wrong notes: {:?}",
            i + 1,
            slide.notes
        );
    }
}

// ── ODP oracle (roadmap item 7 — the LO-native format) ───────────────

use decks_core::odp;

/// Our odp → Impress rewrites as odp → our reader.
fn odp_through_impress(deck: &Deck, stem: &str) -> Option<Deck> {
    if !require_or_skip() { return None; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{stem}.odp"));
    odp::write(deck, path.to_str().unwrap()).expect("write odp");
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let profile = dir.path().join("prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "odp", "--outdir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .expect("soffice runs");
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let rewritten = out_dir.join(format!("{stem}.odp"));
    assert!(rewritten.exists(), "Impress produced no odp");
    Some(odp::read(rewritten.to_str().unwrap()).expect("we failed to read Impress odp"))
}

#[test]
fn odp_text_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "hello from odp", "")];
    let Some(rt) = odp_through_impress(&deck, "text") else { return };
    assert!(
        all_text(&rt.slides[0]).replace('\n', " ").contains("hello from odp"),
        "text lost: {:?}",
        rt.slides[0].objects
    );
}

#[test]
fn odp_notes_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "body", "the odp note")];
    let Some(rt) = odp_through_impress(&deck, "notes") else { return };
    assert!(
        rt.slides[0].notes.replace('\n', " ").contains("the odp note"),
        "notes lost: {:?}",
        rt.slides[0].notes
    );
}

#[test]
fn odp_geometry_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "g".into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::Rect { x: 240.0, y: 180.0, w: 320.0, h: 120.0, rotation: 0.0 }],
        notes: String::new(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let Some(rt) = odp_through_impress(&deck, "geom") else { return };
    // Impress gives the rect its default graphic style, so it reads back
    // as a painted Shape.
    let Some(SlideObject::Rect { x, y, w, h, .. } | SlideObject::Shape { x, y, w, h, .. }) =
        rt.slides[0].objects.iter().find(|o| is_rect(o))
    else {
        panic!("rect lost: {:?}", rt.slides[0].objects)
    };
    let close = |a: f64, b: f64| (a - b).abs() < 6.0;
    assert!(close(*x, 240.0) && close(*y, 180.0), "position drifted: {x},{y}");
    assert!(close(*w, 320.0) && close(*h, 120.0), "size drifted: {w}x{h}");
}

#[test]
fn odp_background_survives_impress_rewrite() {
    let mut deck = Deck::new();
    let mut s = text_slide("bg", "colored", "");
    s.background = "#e8f0fe".into();
    deck.slides = vec![s];
    let Some(rt) = odp_through_impress(&deck, "bg") else { return };
    assert_eq!(rt.slides[0].background.to_lowercase(), "#e8f0fe",
        "background lost: {}", rt.slides[0].background);
}

#[test]
fn odp_slide_order_survives_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = (1..=3)
        .map(|i| text_slide(&format!("S{i}"), &format!("content {i}"), ""))
        .collect();
    let Some(rt) = odp_through_impress(&deck, "order") else { return };
    assert_eq!(rt.slides.len(), 3);
    for (i, slide) in rt.slides.iter().enumerate() {
        assert!(
            all_text(slide).contains(&format!("content {}", i + 1)),
            "slide {} out of order",
            i
        );
    }
}

#[test]
fn odp_bold_run_survives_impress_rewrite() {
    use letters_core::model::{Run as LRun, RunStyle as LRunStyle};
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "styled".into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::TextBox {
            text: "plain bolded".into(),
            x: 100.0, y: 100.0, w: 500.0, h: 60.0,
            runs: vec![
                LRun { text: "plain ".into(), style: LRunStyle::default() },
                LRun {
                    text: "bolded".into(),
                    style: LRunStyle { bold: true, ..Default::default() },
                },
            ],
            rotation: 0.0,
            body: Default::default(),
        }],
        notes: String::new(),
        master_idx: Some(0),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let Some(rt) = odp_through_impress(&deck, "boldrun") else { return };
    let bold: String = rt.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::TextBox { runs, .. } => Some(
                runs.iter().filter(|r| r.style.bold).map(|r| r.text.as_str()).collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(bold.trim(), "bolded", "bold lost: {:?}", rt.slides[0].objects);
}

/// The reverse: an odp Impress writes (from our pptx) must open in our
/// odp reader with the text intact.
#[test]
fn we_read_impress_authored_odp() {
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "authored elsewhere", "")];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("src.pptx");
    write_pptx(path.to_str().unwrap(), &deck).expect("write pptx");
    let odp_path = convert(&path, "odp").expect("Impress could not convert to odp");
    let rt = odp::read(odp_path.to_str().unwrap()).expect("our odp reader failed");
    assert!(
        rt.slides.iter().any(|s| all_text(s).contains("authored elsewhere")),
        "text lost reading Impress-authored odp"
    );
}

// ── Master slides (ADR 0003 §5) ──────────────────────────────────────

/// An Impress-rewritten pptx has real slideMaster/slideLayout parts:
/// we must map slides to masters and must NOT ingest the master's
/// placeholder prompts ("Click to edit …") as decoration content.
#[test]
fn masters_read_from_impress_pptx_without_placeholder_leakage() {
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "master mapping test", "")];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("src.pptx");
    write_pptx(path.to_str().unwrap(), &deck).expect("write pptx");
    let rewritten = convert(&path, "pptx").expect("Impress rewrite");
    let rt = decks_core::engine::read_pptx(rewritten.to_str().unwrap()).expect("read");
    assert!(!rt.masters.is_empty(), "no masters read");
    for s in &rt.slides {
        let mi = s.master_idx.expect("slide unmapped");
        assert!(mi < rt.masters.len(), "master_idx out of range");
    }
    for m in &rt.masters {
        for obj in &m.shapes {
            if let SlideObject::TextBox { text, .. } = obj {
                assert!(
                    !text.to_lowercase().contains("click to edit"),
                    "placeholder leaked into master decorations: {text:?}"
                );
            }
        }
    }
}

// ── The ODF rotation sign (#322, recovery.md rotation row) ───────────

/// ODF has no rotation attribute: it spells rotation as a
/// `draw:transform` list, whose angle is radians *counter-clockwise* where
/// OOXML's `a:xfrm/@rot` is sixtieth-thousandths of a degree *clockwise*.
/// Our odp writer negates, and that negation is the one thing a round trip
/// through our own reader cannot check — read our own file back and a
/// mirrored convention cancels itself out, twice, and passes.
///
/// So this crosses formats. Our odp goes through Impress to pptx, and the
/// pptx reader — which is checked against OOXML's own units — says what
/// angle Impress thought the file asked for. Get the sign wrong and a 30
/// degree clockwise rect comes back as 330.
#[test]
fn impress_reads_our_odp_rotation_with_the_sign_we_wrote() {
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Rect {
        x: 100.0, y: 200.0, w: 300.0, h: 100.0, rotation: 30.0,
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rotated.odp");
    odp::write(&deck, path.to_str().unwrap()).expect("write odp");

    let as_pptx = convert(&path, "pptx").expect("Impress could not convert our odp to pptx");
    let rt = read_pptx(as_pptx.to_str().unwrap()).expect("our pptx reader failed");
    let rotations: Vec<f64> = rt.slides[0]
        .objects
        .iter()
        .map(|o| match o {
            SlideObject::Rect { rotation, .. }
            | SlideObject::TextBox { rotation, .. }
            | SlideObject::Circle { rotation, .. }
            | SlideObject::Shape { rotation, .. }
            | SlideObject::Table { rotation, .. }
            | SlideObject::Chart { rotation, .. }
            | SlideObject::Image { rotation, .. } => *rotation,
        })
        .collect();
    assert!(
        rotations.iter().any(|r| (r - 30.0).abs() < 0.5),
        "Impress read our 30 degree clockwise rect as {rotations:?} — \
         330 means the ODF angle is being written with the wrong sign"
    );
}

/// And the reverse direction: a rotation Impress itself wrote into an odp
/// must come back out of our odp reader as the same clockwise angle. This
/// is the half that catches a `draw:transform` shape our term parser
/// declines and silently reads as square.
#[test]
fn we_read_the_rotation_impress_writes_into_an_odp() {
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Rect {
        x: 100.0, y: 200.0, w: 300.0, h: 100.0, rotation: 30.0,
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("src.pptx");
    write_pptx(path.to_str().unwrap(), &deck).expect("write pptx");

    let as_odp = convert(&path, "odp").expect("Impress could not convert to odp");
    let rt = odp::read(as_odp.to_str().unwrap()).expect("our odp reader failed");
    let rotations: Vec<f64> = rt.slides[0]
        .objects
        .iter()
        .map(|o| match o {
            SlideObject::Rect { rotation, .. }
            | SlideObject::TextBox { rotation, .. }
            | SlideObject::Circle { rotation, .. }
            | SlideObject::Shape { rotation, .. }
            | SlideObject::Table { rotation, .. }
            | SlideObject::Chart { rotation, .. }
            | SlideObject::Image { rotation, .. } => *rotation,
        })
        .collect();
    assert!(
        rotations.iter().any(|r| (r - 30.0).abs() < 0.5),
        "Impress wrote a 30 degree clockwise rect and we read {rotations:?}; \
         0 means the transform was declined and the shape came back square"
    );
}

// ── Masters we write (#322, recovery.md fidelity row) ────────────────

/// A master is only real if Impress honours it. Our own reader can be
/// satisfied by parts that are related to nothing: the pptx chain is slide
/// -> layout -> master across three parts and four relationship files, and
/// a package where any link is missing still round-trips through code that
/// looks in the same places it wrote.
///
/// So this hands both formats to Impress, lets it rewrite the deck in its
/// own terms, and asks our reader what came back. Impress drops a master
/// it cannot resolve, which is what makes the assertion worth making.
#[test]
fn impress_keeps_the_master_we_write() {
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    deck.masters[0].name = "House Style".into();
    deck.masters[0].background = "#204060".into();
    deck.slides[0].master_idx = Some(0);
    deck.slides[0].objects.push(SlideObject::TextBox {
        text: "on the house master".into(),
        x: 100.0, y: 100.0, w: 400.0, h: 60.0,
        rotation: 0.0,
        runs: vec![],
        body: Default::default(),
    });

    let dir = tempfile::tempdir().unwrap();
    let as_pptx = dir.path().join("mastered.pptx");
    write_pptx(as_pptx.to_str().unwrap(), &deck).expect("write pptx");
    let rewritten = convert(&as_pptx, "pptx").expect("Impress could not rewrite our pptx");
    let rt = read_pptx(rewritten.to_str().unwrap()).expect("read back");
    assert!(!rt.masters.is_empty(), "Impress dropped the master from our pptx");
    for s in &rt.slides {
        let mi = s.master_idx.expect("a slide came back on no master");
        assert!(mi < rt.masters.len(), "master_idx {mi} out of range");
    }

    let odp_deck = deck.clone();
    let as_odp = dir.path().join("mastered.odp");
    odp::write(&odp_deck, as_odp.to_str().unwrap()).expect("write odp");
    let rewritten = convert(&as_odp, "odp").expect("Impress could not rewrite our odp");
    let rt = odp::read(rewritten.to_str().unwrap()).expect("read back");
    assert!(!rt.masters.is_empty(), "Impress dropped the master from our odp");
    assert!(
        rt.masters.iter().any(|m| m.background == "#204060"),
        "the master's background is gone after an Impress rewrite: {:?}",
        rt.masters.iter().map(|m| (&m.name, &m.background)).collect::<Vec<_>>(),
    );
}

// ── Pictures we write into an odp (#322, recovery.md fidelity row) ───

/// A picture is only really in the package if Impress can find it. Our own
/// reader goes straight to the `xlink:href` it wrote, so it is satisfied by
/// a package whose manifest never declares the part — and a conforming
/// reader would never open it. Impress is the reader that tells us.
///
/// It crosses formats on purpose: our odp goes through Impress to pptx, and
/// the pptx reader says whether a picture arrived.
#[test]
fn impress_finds_the_picture_we_write_into_an_odp() {
    if !require_or_skip() { return; }
    // A 2x2 red PNG, generated inline so no fixture file is needed.
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

    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Image {
        path: png_path.to_string_lossy().to_string(),
        x: 100.0, y: 100.0, w: 200.0, h: 150.0,
        rotation: 0.0, crop: Default::default()
    });
    let as_odp = dir.path().join("pictured.odp");
    odp::write(&deck, as_odp.to_str().unwrap()).expect("write odp");

    let as_pptx = convert(&as_odp, "pptx").expect("Impress could not convert our odp to pptx");
    let rt = read_pptx(as_pptx.to_str().unwrap()).expect("our pptx reader failed");
    let pictures = rt.slides[0]
        .objects
        .iter()
        .filter(|o| matches!(o, SlideObject::Image { .. }))
        .count();
    assert_eq!(
        pictures, 1,
        "Impress did not carry our odp's picture across; slide came back as {:?}",
        rt.slides[0].objects,
    );
}

/// And the reverse: a picture Impress packaged into an odp must come back
/// out of our odp reader. Impress writes `draw:image` with a caption
/// paragraph inside it, so this is the case that needs the Start arm of the
/// reader rather than the Empty one our own writer produces.
#[test]
fn we_read_the_picture_impress_writes_into_an_odp() {
    if !require_or_skip() { return; }
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

    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Image {
        path: png_path.to_string_lossy().to_string(),
        x: 100.0, y: 100.0, w: 200.0, h: 150.0,
        rotation: 0.0, crop: Default::default()
    });
    let src = dir.path().join("src.pptx");
    write_pptx(src.to_str().unwrap(), &deck).expect("write pptx");

    let as_odp = convert(&src, "odp").expect("Impress could not convert to odp");
    let rt = odp::read(as_odp.to_str().unwrap()).expect("our odp reader failed");
    let recovered: Vec<&String> = rt.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::Image { path, .. } => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(
        recovered.len(), 1,
        "we did not read the picture out of an Impress-written odp: {:?}",
        rt.slides[0].objects,
    );
    let bytes = std::fs::read(recovered[0]).expect("the unpacked picture is unreadable");
    assert!(!bytes.is_empty(), "the picture unpacked to an empty file");
}

/// The font we put in a pptx theme has to reach a real consumer, and the
/// only thing that can say so is a real consumer.
///
/// This crosses formats deliberately: our pptx (theme `a:fontScheme`) →
/// Impress → its odp (`style:default-style`) → our odp reader. It is the
/// measurement the odp writer's design came from — before it, where ODF
/// keeps a default font was a guess.
#[test]
fn impress_carries_the_theme_font_from_our_pptx_into_an_odp() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let mut deck = Deck::new();
    deck.masters[0].default_font = "Liberation Serif".into();
    deck.slides[0].objects.push(SlideObject::TextBox {
        text: "body text".into(),
        x: 100.0, y: 100.0, w: 300.0, h: 80.0,
        rotation: 0.0,
        runs: vec![],
        body: Default::default(),
    });
    let as_pptx = dir.path().join("themed.pptx");
    write_pptx(as_pptx.to_str().unwrap(), &deck).expect("write pptx");

    let as_odp = convert(&as_pptx, "odp").expect("Impress could not convert our pptx to odp");
    let rt = odp::read(as_odp.to_str().unwrap()).expect("our odp reader failed");
    let fonts: Vec<&str> = rt.masters.iter().map(|m| m.default_font.as_str()).collect();
    assert!(
        fonts.contains(&"Liberation Serif"),
        "Impress did not carry our theme font into its odp; masters came back as {fonts:?}",
    );
}

/// And the reverse direction: the font we write into an odp's
/// `style:default-style` has to be honoured by a real consumer.
///
/// This asserts odp → Impress → odp, not odp → Impress → pptx, and the
/// difference was measured rather than assumed. The pptx version of this
/// test failed with `["Arial"]`, which looked like our writer being wrong.
/// It is not: converting the same file odp → odp shows Impress reading our
/// `style:default-style[@style:family="graphic"]` and writing it straight
/// back out.
///
/// What actually loses it is **Impress's pptx exporter**, which does not
/// derive a theme `a:fontScheme` from the document's default graphic font.
/// That is a gap in its export path, not in this package, and asserting
/// over it would have pinned someone else's bug as our contract.
#[test]
fn impress_keeps_the_font_we_write_into_an_odp() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let mut deck = Deck::new();
    deck.masters[0].default_font = "Liberation Serif".into();
    deck.slides[0].objects.push(SlideObject::TextBox {
        text: "body text".into(),
        x: 100.0, y: 100.0, w: 300.0, h: 80.0,
        rotation: 0.0,
        runs: vec![],
        body: Default::default(),
    });
    let as_odp = dir.path().join("fonted.odp");
    odp::write(&deck, as_odp.to_str().unwrap()).expect("write odp");

    let rewritten = convert(&as_odp, "odp").expect("Impress could not rewrite our odp");
    let rt = odp::read(rewritten.to_str().unwrap()).expect("our odp reader failed");
    let fonts: Vec<&str> = rt.masters.iter().map(|m| m.default_font.as_str()).collect();
    assert!(
        fonts.contains(&"Liberation Serif"),
        "Impress dropped the font we wrote into our odp; masters came back as {fonts:?}",
    );
}

/// Two runs in one paragraph, rewritten by Impress and read back by us.
///
/// The round-trip tests in `snapshot_fidelity.rs` pair our writer with our
/// reader, so a convention both sides share cancels itself out and passes.
/// Here Impress writes the `p:txBody`, so the fixture is a real producer's
/// idea of "one paragraph, two runs" rather than ours — which is the only
/// way to be sure our reader joins runs the way the format means and not
/// merely the way we happen to emit them.
///
/// The bug this pins: the reader recorded one entry per `a:t` and joined
/// them with `\n`, so an emphasised word split the line in two, and the
/// break was then written back into the saved package.
#[test]
fn impress_runs_in_one_paragraph_come_back_as_one_line() {
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "T".into(),
        background: String::new(),
        notes: String::new(),
        master_idx: None,
        objects: vec![SlideObject::TextBox {
            text: "Plain Bold".into(),
            x: 20.0,
            y: 20.0,
            w: 400.0,
            h: 60.0,
            rotation: 0.0,
            runs: vec![
                Run { text: "Plain ".into(), style: RunStyle::default() },
                Run {
                    text: "Bold".into(),
                    style: RunStyle { bold: true, ..RunStyle::default() },
                },
            ],
            body: Default::default(),
        }],
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let Some(rt) = through_impress(&deck, "tworuns") else { return };
    let text = all_text(&rt.slides[0]);
    assert!(
        text.contains("Plain Bold"),
        "runs of one paragraph came back split or welded: {text:?}"
    );
    assert!(
        !text.contains("Plain\nBold") && !text.contains("PlainBold"),
        "the line was broken or the space between runs was eaten: {text:?}"
    );
}

/// A styled multi-line text box keeps BOTH its line break and its styling
/// through Impress, in odp.
///
/// ODF collapses a literal newline in text content to a space, and the odp
/// writer's styled path emitted exactly that: one `text:p` containing
/// "Bold one\nplain two". Our own reader could not see the loss, because it
/// split that single paragraph back on the newline it had written — a
/// mirrored convention cancelling itself out, the same trap the transform
/// matrix hit. Impress is what reports it: before the fix it read the box
/// back as one line, `<text:span>Bold one</text:span> plain two`.
///
/// The styling half matters too, and for a second reason: the reader used
/// to keep runs only for single-paragraph boxes, so writing the break
/// correctly would have traded a lost line break for lost styling.
#[test]
fn a_styled_multiline_box_keeps_its_break_and_its_styling_through_impress() {
    if !require_or_skip() {
        return;
    }
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "T".into(),
        background: String::new(),
        notes: String::new(),
        master_idx: None,
        objects: vec![SlideObject::TextBox {
            text: "Bold one\nplain two".into(),
            x: 20.0,
            y: 20.0,
            w: 400.0,
            h: 80.0,
            rotation: 0.0,
            runs: vec![
                Run {
                    text: "Bold one".into(),
                    style: RunStyle { bold: true, ..RunStyle::default() },
                },
                Run { text: "\nplain two".into(), style: RunStyle::default() },
            ],
            body: Default::default(),
        }],
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("styledlines.odp");
    odp::write(&deck, src.to_str().unwrap()).expect("write odp");
    let rewritten = convert(&src, "odp").expect("Impress could not rewrite our odp");
    let rt = odp::read(rewritten.to_str().unwrap()).expect("read back");

    let SlideObject::TextBox { text, runs, .. } = &rt.slides[0].objects[0] else {
        panic!("the text box came back as a different object kind");
    };
    assert!(
        text.contains('\n'),
        "Impress read our line break as a space: {text:?}"
    );
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        *text,
        "concatenated run text must equal `text`"
    );
    assert!(
        runs.iter().any(|r| r.style.bold),
        "the styling was dropped on the way: {runs:?}"
    );

    // The pptx writer had the same defect for the same reason — a newline
    // inside `a:t` instead of a second `a:p`. LibreOffice reads that as a
    // break, so unlike the odp case it round-tripped either way and only
    // the emitted markup was wrong; this asserts the behaviour a stricter
    // consumer depends on.
    let as_pptx = dir.path().join("styledlines.pptx");
    write_pptx(as_pptx.to_str().unwrap(), &deck).expect("write pptx");
    let rewritten = convert(&as_pptx, "pptx").expect("Impress could not rewrite our pptx");
    let rt = read_pptx(rewritten.to_str().unwrap()).expect("read back");
    let SlideObject::TextBox { text, runs, .. } = &rt.slides[0].objects[0] else {
        panic!("the pptx text box came back as a different object kind");
    };
    assert!(text.contains('\n'), "pptx: the line break is gone: {text:?}");
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        *text,
        "pptx: concatenated run text must equal `text`"
    );
    assert!(
        runs.iter().any(|r| r.style.bold),
        "pptx: the styling was dropped: {runs:?}"
    );
}

/// Speaker notes across a format boundary.
///
/// `notes_survive_impress_rewrite` above rewrites a pptx as a pptx, which
/// our own writer's markup survives trivially. Coming the other way,
/// Impress's pptx exporter writes the notes into a shape with **no
/// placeholder at all**, and our reader required `p:ph type="body"` — so
/// the speaker notes of every deck that had been through Impress were
/// dropped on import, silently, while the same-format test stayed green.
///
/// Worth recording how this was diagnosed, because the obvious reading was
/// wrong: an odp -> odp rewrite kept the notes, which proves Impress reads
/// what we write, and its pptx *did* contain `notesSlide1.xml` carrying
/// the text. So this was never Impress's export gap — unlike the font in
/// #733, where the part genuinely was not written. It was our reader.
#[test]
fn speaker_notes_survive_a_conversion_between_the_two_formats() {
    if !require_or_skip() {
        return;
    }
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "body", "remember the demo")];
    let dir = tempfile::tempdir().unwrap();

    let as_odp = dir.path().join("notes.odp");
    odp::write(&deck, as_odp.to_str().unwrap()).expect("write odp");
    let to_pptx = convert(&as_odp, "pptx").expect("Impress could not convert our odp to pptx");
    let rt = read_pptx(to_pptx.to_str().unwrap()).expect("read pptx");
    assert!(
        rt.slides[0].notes.contains("remember the demo"),
        "odp -> Impress -> pptx lost the speaker notes: {:?}",
        rt.slides[0].notes
    );

    let as_pptx = dir.path().join("notes.pptx");
    write_pptx(as_pptx.to_str().unwrap(), &deck).expect("write pptx");
    let to_odp = convert(&as_pptx, "odp").expect("Impress could not convert our pptx to odp");
    let rt = odp::read(to_odp.to_str().unwrap()).expect("read odp");
    assert!(
        rt.slides[0].notes.contains("remember the demo"),
        "pptx -> Impress -> odp lost the speaker notes: {:?}",
        rt.slides[0].notes
    );
}

/// Geometry across a format boundary, which is the only place a wrong
/// scale can show.
///
/// `positions_approx_survive_impress_rewrite` and
/// `odp_geometry_survives_impress_rewrite` both rewrite a file in its own
/// format, and a same-format rewrite cannot detect a wrong unit: Impress
/// reads whatever we wrote and writes the same value back, so our own
/// constant cancels itself out on the way in and out. Crossing formats is
/// what forces a real conversion — Impress turns EMU into centimetres
/// using the true ratio — and it is where the two readers' disagreement
/// showed: a box round-tripped pptx -> odp came back at 0.75x, and
/// odp -> pptx at 1.33x, while both same-format tests stayed green.
#[test]
fn geometry_survives_a_conversion_between_the_two_formats() {
    if !require_or_skip() {
        return;
    }
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "T".into(),
        background: String::new(),
        notes: String::new(),
        master_idx: None,
        objects: vec![SlideObject::Rect {
            x: 96.0,
            y: 54.0,
            w: 192.0,
            h: 108.0,
            rotation: 0.0,
        }],
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout: None,
    }];
    let want = (96.0, 54.0, 192.0, 108.0);
    let dir = tempfile::tempdir().unwrap();

    let geom = |d: &Deck, label: &str| -> (f64, f64, f64, f64) {
        match d.slides.first().and_then(|s| s.objects.first()) {
            Some(SlideObject::Rect { x, y, w, h, .. })
            | Some(SlideObject::Shape { x, y, w, h, .. })
            | Some(SlideObject::Table { x, y, w, h, .. })
            | Some(SlideObject::TextBox { x, y, w, h, .. }) => (*x, *y, *w, *h),
            other => panic!("{label}: the shape came back as {other:?}"),
        }
    };
    // A conversion re-lays-out slightly; this is about scale, not pixels.
    let close = |got: (f64, f64, f64, f64), label: &str| {
        let d = [
            (got.0 - want.0).abs(),
            (got.1 - want.1).abs(),
            (got.2 - want.2).abs(),
            (got.3 - want.3).abs(),
        ];
        assert!(
            d.iter().all(|v| *v < 2.0),
            "{label}: {got:?} is not {want:?} — a scale factor, not a rounding"
        );
    };

    let as_pptx = dir.path().join("cross.pptx");
    write_pptx(as_pptx.to_str().unwrap(), &deck).expect("write pptx");
    let to_odp = convert(&as_pptx, "odp").expect("Impress could not convert our pptx to odp");
    close(
        geom(&odp::read(to_odp.to_str().unwrap()).expect("read odp"), "pptx->odp"),
        "pptx -> Impress -> odp",
    );

    let as_odp = dir.path().join("cross.odp");
    odp::write(&deck, as_odp.to_str().unwrap()).expect("write odp");
    let to_pptx = convert(&as_odp, "pptx").expect("Impress could not convert our odp to pptx");
    close(
        geom(&read_pptx(to_pptx.to_str().unwrap()).expect("read pptx"), "odp->pptx"),
        "odp -> Impress -> pptx",
    );
}

/// A slide's name reaches Impress, in the direction that is a claim about
/// our package.
///
/// `Slide::title` is the slide's name: `p:cSld/@name` in OOXML,
/// `draw:page/@draw:name` in ODF. The pptx writer wrote the slide's
/// `p:cSld` bare while naming the master's and the layout's, so the name
/// never left the package at all — and no test asked, because every
/// fixture's title was the empty string.
///
/// Only the pptx -> odp direction is asserted, and the reason is measured
/// rather than assumed. Coming back the other way, Impress's pptx exporter
/// writes `<p:cSld>` with no name and the string appears nowhere in
/// `slide1.xml`, while an odp -> odp rewrite keeps it — so Impress reads
/// our `draw:name` correctly and simply does not carry a page name into
/// pptx. That is its export gap, the same situation as the master font in
/// #733, and asserting over it would pin someone else's bug as our
/// contract. `a_slides_name_survives_a_snapshot` covers our own odp write.
#[test]
fn a_slide_name_reaches_impress_from_our_pptx() {
    if !require_or_skip() {
        return;
    }
    let mut deck = Deck::new();
    let mut slide = text_slide("T", "body", "");
    slide.title = "Quarterly Review".into();
    deck.slides = vec![slide];

    let dir = tempfile::tempdir().unwrap();
    let as_pptx = dir.path().join("named.pptx");
    write_pptx(as_pptx.to_str().unwrap(), &deck).expect("write pptx");
    let to_odp = convert(&as_pptx, "odp").expect("Impress could not convert our pptx to odp");
    let rt = odp::read(to_odp.to_str().unwrap()).expect("read odp");
    assert_eq!(
        rt.slides[0].title, "Quarterly Review",
        "the slide's name did not survive our pptx through Impress"
    );
}

/// Paragraph alignment, bullets and list levels, written by us in either
/// format, survive LibreOffice rewriting the file in that format, and we
/// read LibreOffice's version back.
#[test]
fn paragraph_styles_survive_impress_rewrite() {
    use decks_core::engine::{Bullet, ParaAlign, ParaStyle, TextBody};
    if !require_or_skip() { return; }
    let bullet = |level: u8, c: &str| ParaStyle {
        level,
        bullet: Bullet::Char(c.into()),
        margin_left: 27.0 * (level as f64 + 1.0),
        indent: -27.0,
        ..Default::default()
    };
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::TextBox {
        text: "Centred\nFirst\nSub".into(),
        x: 60.0, y: 60.0, w: 600.0, h: 300.0,
        rotation: 0.0,
        runs: vec![],
        body: TextBody {
            paras: vec![ParaStyle { align: ParaAlign::Center, ..Default::default() }, bullet(0, "•"), bullet(1, "–")],
            ..Default::default()
        },
    });
    for kind in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("paras.{kind}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
        let back = convert(&path, kind).unwrap_or_else(|e| panic!("{kind}: {e}"));
        let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
        let body = read.slides[0]
            .objects
            .iter()
            .find_map(|o| match o {
                SlideObject::TextBox { text, body, .. } if text.contains("First") => Some(body.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{kind}: the box is gone: {:?}", read.slides[0].objects));
        assert_eq!(body.para(0).align, ParaAlign::Center, "{kind}: {body:?}");
        assert!(matches!(body.para(1).bullet, Bullet::Char(_)), "{kind}: {body:?}");
        assert_eq!(body.para(2).level, 1, "{kind}: {body:?}");
        assert!(matches!(body.para(2).bullet, Bullet::Char(_)), "{kind}: {body:?}");
    }
}

/// Our Magic Move is PowerPoint's Morph inside mc:AlternateContent.
/// LibreOffice doesn't know Morph, so it must take the Fallback: the file
/// opens, and its own rewrite still has a transition on that slide.
#[test]
fn impress_takes_the_fallback_of_our_magic_move() {
    use decks_core::engine::Transition;
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    let mut second = deck.slides[0].clone();
    second.transition = Transition::MagicMove;
    deck.slides.push(second);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("morph.pptx");
    write_pptx(path.to_str().unwrap(), &deck).expect("write pptx");
    let back = convert(&path, "pptx").expect("Impress could not import our Morph");
    let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's pptx");
    assert_eq!(read.slides.len(), 2);
    assert_ne!(read.slides[1].transition, Transition::None, "the fade fallback was kept");
}

/// Slide transitions we write to odp are ones LibreOffice keeps: after it
/// rewrites the file each is still what we wrote. LibreOffice has no Magic
/// Move and plays it as the crossfade we write beside our marker; it even
/// keeps the marker (a foreign attribute on the drawing-page style), so
/// Magic Move survives the round trip through it.
#[test]
fn impress_keeps_our_odp_transitions() {
    use decks_core::engine::Transition;
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    let base = deck.slides[0].clone();
    deck.slides = Transition::ALL.iter().map(|t| Slide { transition: *t, ..base.clone() }).collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transitions.odp");
    decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write odp");
    let back = convert(&path, "odp").expect("Impress rewrites our odp");
    let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's odp");
    let got: Vec<Transition> = read.slides.iter().map(|s| s.transition).collect();
    assert_eq!(
        got,
        Transition::ALL.to_vec(),
        "Impress's odp"
    );
}

/// The builds we write are ones LibreOffice understands: after it rewrites
/// our pptx, each object still builds, in the same order, with the same
/// kind of effect.
#[test]
fn impress_keeps_our_object_builds() {
    use decks_core::builds::{Build, BuildEffect, Edge};
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    for (i, y) in [60.0, 200.0, 340.0].into_iter().enumerate() {
        deck.slides[0].objects.push(SlideObject::TextBox {
            text: format!("Point {}", i + 1),
            x: 60.0, y, w: 400.0, h: 60.0,
            rotation: 0.0,
            runs: vec![],
            body: Default::default(),
        });
    }
    let builds = vec![
        Build { object: 0, effect: BuildEffect::Appear, out: false },
        Build { object: 1, effect: BuildEffect::Dissolve, out: false },
        Build { object: 2, effect: BuildEffect::Move(Edge::Left), out: false },
        Build { object: 0, effect: BuildEffect::Dissolve, out: true },
    ];
    deck.slides[0].builds = builds.clone();
    let want: Vec<(String, BuildEffect, bool)> =
        builds.iter().map(|b| (format!("Point {}", b.object + 1), b.effect, b.out)).collect();
    // Both formats, each rewritten by LibreOffice in the same format.
    for kind in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("builds.{kind}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
        let back = convert(&path, kind).unwrap_or_else(|e| panic!("Impress rewrites our {kind}: {e}"));
        let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
        let texts: Vec<String> = read.slides[0]
            .objects
            .iter()
            .map(|o| match o { SlideObject::TextBox { text, .. } => text.clone(), _ => String::new() })
            .collect();
        let got: Vec<(String, BuildEffect, bool)> =
            read.slides[0].builds.iter().map(|b| (texts[b.object].clone(), b.effect, b.out)).collect();
        assert_eq!(got, want, "{kind}");
    }
}

#[test]
fn impress_sees_the_slide_size_we_kept() {
    // Issue triage for #440: a 4:3 or custom-size pptx, opened and saved,
    // used to come back 16:9. LibreOffice is the judge: what it makes of
    // our saved file (rewritten as pptx and as odp) must be exactly what
    // it makes of the source, which had the size. Impress rounds a size
    // through its own units (a 20cm square reads as 7199313 EMU), so the
    // comparison is between its two readings, not with our EMU.
    if !require_or_skip() {
        return;
    }
    for size in [(9_144_000.0, 6_858_000.0), (7_200_000.0, 7_200_000.0)] {
        let dir = tempfile::tempdir().unwrap();
        let mut deck = Deck::new();
        deck.masters[0].page_emu = Some(size);
        deck.slides[0].objects = vec![SlideObject::Rect { x: 0.0, y: 0.0, w: 960.0, h: 540.0, rotation: 0.0 }];
        let source = dir.path().join("source.pptx");
        write_pptx(source.to_str().unwrap(), &deck).expect("write the source");
        // Open and save, as the app does.
        let opened = read_pptx(source.to_str().unwrap()).expect("open");
        let saved_dir = dir.path().join("saved");
        std::fs::create_dir_all(&saved_dir).unwrap();
        let saved = saved_dir.join("source.pptx");
        write_pptx(saved.to_str().unwrap(), &opened).expect("save");
        for kind in ["pptx", "odp"] {
            let impress = |path: &std::path::Path| {
                let back = convert(path, kind).unwrap_or_else(|e| panic!("Impress rewrites {path:?} as {kind}: {e}"));
                let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
                let geometry = read.slides[0].objects.first().map(|o| format!("{o:?}"));
                (read.masters[0].page_emu, geometry)
            };
            let (from_source, from_saved) = (impress(&source), impress(&saved));
            let (cx, cy) = from_source.0.unwrap_or_else(|| panic!("{kind}: Impress lost the source's own size"));
            assert!(
                (cx - 9_144_000.0).abs() > 100_000.0 || (cy - 5_143_500.0).abs() > 100_000.0,
                "{kind}: the source already reads as our 16:9 default: {cx}x{cy}"
            );
            assert_eq!(from_saved, from_source, "{kind} {size:?}: Impress sees our saved deck differently from its source");
        }
    }
}

#[test]
fn notes_paragraphs_survive_impress_in_both_formats() {
    // What the notes pane writes: several paragraphs, a blank line
    // between two of them, Unicode. Impress's own rewrite of each format
    // must keep every paragraph, in order, blank line included.
    let notes = "Open with the question.\nThen the three numbers — 12, 40, 7.\n\nPause for questions; café at 15:00.";
    let mut deck = Deck::new();
    deck.slides = vec![text_slide("T", "body", notes), text_slide("U", "second", "")];
    let rewrites = [("pptx", through_impress(&deck, "notesparas")), ("odp", odp_through_impress(&deck, "notesparas"))];
    for (kind, rt) in rewrites {
        let Some(rt) = rt else { return };
        assert_eq!(rt.slides[0].notes, notes, "{kind}: Impress changed the notes");
        assert_eq!(rt.slides[1].notes, "", "{kind}: a slide without notes gained some");
    }
}

#[test]
fn impress_sees_our_odp_on_the_decks_own_page() {
    // The odp writer used to put every deck on its own 960x540pt page. What
    // Impress makes of our odp must be what it makes of our pptx of the
    // same deck, whose size it already reads right: the page, and the
    // full-bleed shape covering it.
    if !require_or_skip() {
        return;
    }
    for size in [(9_144_000.0, 6_858_000.0), (7_200_000.0, 7_200_000.0)] {
        let dir = tempfile::tempdir().unwrap();
        let mut deck = Deck::new();
        deck.masters[0].page_emu = Some(size);
        // Painted by us, so its look doesn't depend on each format's
        // default style in Impress.
        let style = decks_core::engine::shape::ShapeStyle { fill: Some(decks_core::engine::shape::Color(0x20, 0x40, 0x60)), gradient: None, stroke: None };
        deck.slides[0].objects = vec![SlideObject::Shape {
            kind: decks_core::engine::shape::ShapeKind::Rect,
            x: 0.0,
            y: 0.0,
            w: 960.0,
            h: 540.0,
            rotation: 0.0,
            style,
        }];
        let mut seen = Vec::new();
        for ext in ["pptx", "odp"] {
            let sub = dir.path().join(ext);
            std::fs::create_dir_all(&sub).unwrap();
            let ours = sub.join(format!("deck.{ext}"));
            decks_core::write_deck(ours.to_str().unwrap(), &deck).expect("write");
            let back = convert(&ours, "odp").unwrap_or_else(|e| panic!("Impress rewrites our {ext}: {e}"));
            let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's odp");
            let Some(SlideObject::Shape { x, y, w, h, style, .. }) = read.slides[0].objects.first().cloned() else {
                panic!("{ext}: the shape came back as {:?}", read.slides[0].objects)
            };
            seen.push((read.masters[0].page_emu, (x, y, w, h), style));
        }
        let (pptx, odp) = (&seen[0], &seen[1]);
        assert!(pptx.0.is_some(), "{size:?}: Impress lost the pptx's size");
        assert_eq!(odp.0, pptx.0, "{size:?}: Impress sees our odp's page differently from our pptx's");
        assert_eq!(odp.2, pptx.2, "{size:?}: the paint");
        // Impress keeps lengths in hundredths of a millimetre. EMU convert
        // to them exactly and our odp's points don't, so the two can differ
        // by that one unit, and by no more: here, in model units.
        let unit = 0.01 / 25.4 * 914_400.0 * 960.0 / size.0;
        let g = [(odp.1 .0, pptx.1 .0), (odp.1 .1, pptx.1 .1), (odp.1 .2, pptx.1 .2), (odp.1 .3, pptx.1 .3)];
        assert!(
            g.iter().all(|(a, b)| (a - b).abs() <= unit + 1e-9),
            "{size:?}: Impress places our odp's shape at {:?}, our pptx's at {:?} (one 1/100 mm is {unit})",
            odp.1,
            pptx.1
        );
    }
}

#[test]
fn impress_keeps_a_themes_decorations_in_both_formats() {
    // Every built-in theme's master decorations (fills, a two-colour
    // gradient, presets) as Impress rewrites our file, in each format.
    if !require_or_skip() {
        return;
    }
    let look = |o: &SlideObject| -> String {
        match o {
            SlideObject::Shape { kind, x, y, w, h, style, .. } => {
                let r = |v: &f64| v.round();
                format!("{kind:?} {} {} {} {} {:?} {:?}", r(x), r(y), r(w), r(h), style.fill, style.gradient)
            }
            other => format!("{other:?}"),
        }
    };
    for (i, t) in decks_core::templates::templates().iter().enumerate() {
        let (slides, masters) = decks_core::templates::deck(i).unwrap();
        let deck = Deck { slides, masters };
        let want: Vec<String> = deck.masters[0].shapes.iter().map(look).collect();
        for ext in ["pptx", "odp"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("theme.{ext}"));
            decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
            let back = convert(&path, ext).unwrap_or_else(|e| panic!("Impress rewrites our {ext}: {e}"));
            let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
            // Impress makes a master of each layout a slide uses (its own
            // model has no layouts), so the theme's decorations are on each
            // master a slide is on.
            for s in &read.slides {
                let m = &read.masters[s.master_idx.unwrap_or(0)];
                let got: Vec<String> = m.shapes.iter().map(look).collect();
                assert_eq!(got, want, "{} {ext}: the master's decorations through Impress", t.name);
            }
        }
    }
}

#[test]
fn impress_turns_our_odp_gradient_into_the_same_drawingml_gradient() {
    // ODF's gradient angle runs the other way from DrawingML's and starts
    // from another axis (odp_graphics). A round trip through one format
    // can't see a wrong conversion; crossing formats can: our odp, saved
    // as pptx by Impress, must carry the gradient we wrote.
    if !require_or_skip() {
        return;
    }
    use decks_core::engine::shape::{Color, GradientStop, LinearGradient, ShapeKind, ShapeStyle};
    for angle in [0.0, 90.0, 45.0] {
        let gradient = LinearGradient {
            stops: vec![GradientStop { pos: 0.0, color: Color(0x0B, 0x3D, 0x6B) }, GradientStop { pos: 1.0, color: Color(0x13, 0x8D, 0x9C) }],
            angle,
        };
        let style = ShapeStyle { fill: gradient.mean(), gradient: Some(gradient.clone()), stroke: None };
        let mut deck = Deck::new();
        deck.slides[0].objects = vec![SlideObject::Shape { kind: ShapeKind::Rect, x: 100.0, y: 100.0, w: 300.0, h: 200.0, rotation: 0.0, style }];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gradient.odp");
        decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write odp");
        let back = convert(&path, "pptx").unwrap_or_else(|e| panic!("Impress saves our odp as pptx: {e}"));
        let read = read_pptx(back.to_str().unwrap()).expect("read Impress's pptx");
        match read.slides[0].objects.first() {
            Some(SlideObject::Shape { style, .. }) => {
                let got = style.gradient.as_ref().unwrap_or_else(|| panic!("{angle}: no gradient: {style:?}"));
                assert_eq!(got.angle, angle, "{angle}: Impress turned our gradient");
                let colors: Vec<Color> = got.stops.iter().map(|s| s.color).collect();
                assert_eq!(colors.first(), Some(&gradient.stops[0].color), "{angle}: start colour");
                assert_eq!(colors.last(), Some(&gradient.stops[1].color), "{angle}: end colour");
            }
            other => panic!("{angle}: the shape came back as {other:?}"),
        }
    }
}

#[test]
fn impress_keeps_a_master_view_edit() {
    // A shape added to a master in the master view, as Impress rewrites
    // our file in each format: it stays on the master, painted as we
    // painted it.
    if !require_or_skip() {
        return;
    }
    use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
    let (slides, masters) = decks_core::templates::deck(3).unwrap();
    let c = decks_core::DecksController::new(slides, masters);
    c.edit_master(0).unwrap();
    let style = ShapeStyle { fill: Some(Color(0xE0, 0x1B, 0x24)), gradient: None, stroke: None };
    c.add_object(0, SlideObject::Shape { kind: ShapeKind::Triangle, x: 860.0, y: 20.0, w: 80.0, h: 80.0, rotation: 0.0, style: style.clone() });
    c.finish_master().unwrap();
    let deck = c.deck();
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("master.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
        let back = convert(&path, ext).unwrap_or_else(|e| panic!("Impress rewrites our {ext}: {e}"));
        let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
        let found = read.masters.iter().flat_map(|m| &m.shapes).any(|o| {
            matches!(o, SlideObject::Shape { kind: ShapeKind::Triangle, x, y, w, h, style: s, .. }
                if x.round() == 860.0 && y.round() == 20.0 && w.round() == 80.0 && h.round() == 80.0 && s.fill == style.fill)
        });
        assert!(found, "{ext}: the master's triangle through Impress: {:#?}", read.masters);
        assert!(read.slides.iter().all(|s| !s.objects.iter().any(|o| matches!(o, SlideObject::Shape { kind: ShapeKind::Triangle, .. }))),
            "{ext}: the triangle is on the master, not a slide");
    }
}

#[test]
fn impress_keeps_our_layouts_and_placeholders() {
    // A themed deck whose second slide was put on Two Content (with the
    // empty second body that added), as Impress rewrites it in each format:
    // every slide keeps the kind of its layout, and its boxes the
    // placeholders they fill, in their places, with their text.
    if !require_or_skip() {
        return;
    }
    let (slides, masters) = decks_core::templates::deck(3).unwrap();
    let c = decks_core::DecksController::new(slides, masters);
    assert!(c.apply_layout(1, 3));
    let deck = c.deck();
    let summary = |d: &Deck| -> Vec<String> {
        d.slides
            .iter()
            .map(|s| {
                let kind = s
                    .master_idx
                    .and_then(|m| d.masters.get(m))
                    .and_then(|m| m.layouts.get(s.layout?))
                    .map(|l| format!("{:?}", l.kind));
                let boxes: Vec<String> = s
                    .objects
                    .iter()
                    .filter_map(|o| match o {
                        SlideObject::TextBox { text, x, y, w, h, body, .. } => Some(format!(
                            "{:?} {:?} {} {} {} {} {:?}",
                            body.placeholder,
                            body.anchor,
                            x.round(),
                            y.round(),
                            w.round(),
                            h.round(),
                            text
                        )),
                        _ => None,
                    })
                    .collect();
                format!("{kind:?} {boxes:?}")
            })
            .collect()
    };
    let want = summary(&deck);
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("layouts.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
        let back = convert(&path, ext).unwrap_or_else(|e| panic!("Impress rewrites our {ext}: {e}"));
        let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
        assert_eq!(summary(&read), want, "{ext}: layouts and placeholders through Impress");
    }
}

// ── Charts ───────────────────────────────────────────────────────────

/// Every chart on the deck: its kind, series, points and box, in slide
/// order.
fn charts_of(d: &Deck) -> Vec<String> {
    d.slides
        .iter()
        .flat_map(|s| &s.objects)
        .filter_map(|o| match o {
            SlideObject::Chart { x, y, w, h, chart, .. } => Some(format!(
                "{:?} {:?} {:?} {} {} {} {}",
                chart.kind,
                chart.series,
                chart.points,
                x.round(),
                y.round(),
                w.round(),
                h.round()
            )),
            _ => None,
        })
        .collect()
}

/// A slide per chart kind, each chart as the Chart button inserts it.
fn chart_deck() -> Deck {
    let mut deck = Deck::new();
    let proto = deck.slides[0].clone();
    deck.slides = decks_core::insert::CHART_KINDS
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            let mut s = proto.clone();
            s.title = format!("Chart {}", i + 1);
            s.objects = vec![decks_core::insert::chart(*kind)];
            s
        })
        .collect();
    deck
}

/// Our `from` file, rewritten by Impress as `to`, read back by us: every
/// chart is still there, the same kind with the same series and values.
/// Across formats, Impress has to have understood our chart part to write
/// its own.
fn charts_through_impress(from: &str, to: &str) {
    if !require_or_skip() {
        return;
    }
    let deck = chart_deck();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("charts.{from}"));
    decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
    let back = convert(&path, to).unwrap_or_else(|e| panic!("Impress converts our {from} to {to}: {e}"));
    let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
    assert_eq!(charts_of(&read), charts_of(&deck), "{from} -> Impress -> {to}");
}

#[test]
fn charts_survive_impress_pptx_rewrite() {
    charts_through_impress("pptx", "pptx");
}

#[test]
fn charts_survive_impress_odp_rewrite() {
    charts_through_impress("odp", "odp");
}

#[test]
fn impress_reads_our_pptx_charts_as_its_own() {
    charts_through_impress("pptx", "odp");
}

#[test]
fn impress_reads_our_odp_charts_as_its_own() {
    charts_through_impress("odp", "pptx");
}

#[path = "support/mod.rs"]
mod picture_support;

fn the_crop(deck: &Deck) -> decks_core::engine::Crop {
    deck.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::Image { crop, .. } => Some(*crop),
            _ => None,
        })
        .expect("a picture")
}

fn close(a: decks_core::engine::Crop, b: decks_core::engine::Crop, what: &str) {
    for (x, y, side) in [(a.left, b.left, "left"), (a.top, b.top, "top"), (a.right, b.right, "right"), (a.bottom, b.bottom, "bottom")] {
        assert!((x - y).abs() < 0.01, "{what}: {side} crop {x} != {y} ({a:?} vs {b:?})");
    }
}

/// A cropped picture (pptx `a:srcRect`, ODF `fo:clip`) stays cropped by the
/// same amount through Impress both ways: our pptx → Impress's odp → our
/// odp reader, and our odp → Impress's pptx → our pptx reader.
#[test]
fn a_picture_crop_survives_impress_both_ways() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("photo.png");
    std::fs::write(&png, picture_support::picture(400, 300, 7)).unwrap();
    let crop = decks_core::engine::Crop { left: 0.25, top: 0.1, right: 0.0, bottom: 0.2 };
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Image {
        path: png.to_string_lossy().to_string(),
        x: 100.0, y: 100.0, w: 300.0, h: 210.0,
        rotation: 0.0, crop,
    });

    let ours_pptx = dir.path().join("crop.pptx");
    write_pptx(ours_pptx.to_str().unwrap(), &deck).unwrap();
    close(the_crop(&read_pptx(ours_pptx.to_str().unwrap()).unwrap()), crop, "our own pptx");
    let impress_odp = convert(&ours_pptx, "odp").expect("Impress converts our pptx");
    close(the_crop(&odp::read(impress_odp.to_str().unwrap()).unwrap()), crop, "Impress's odp of our pptx");

    let odp_dir = dir.path().join("odp");
    std::fs::create_dir_all(&odp_dir).unwrap();
    let ours_odp = odp_dir.join("crop.odp");
    odp::write(&deck, ours_odp.to_str().unwrap()).unwrap();
    close(the_crop(&odp::read(ours_odp.to_str().unwrap()).unwrap()), crop, "our own odp");
    let impress_pptx = convert(&ours_odp, "pptx").expect("Impress converts our odp");
    close(the_crop(&read_pptx(impress_pptx.to_str().unwrap()).unwrap()), crop, "Impress's pptx of our odp");
}

#[test]
fn shape_kind_fill_outline_and_rotation_survive_impress_in_both_formats() {
    // Not just the text: each preset we draw, with its own fill, outline
    // and rotation, as Impress rewrites our file in the same format.
    if !require_or_skip() {
        return;
    }
    use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle, Stroke};
    let shape = |kind: ShapeKind, x: f64, rotation: f64, fill: Color, line: Color| SlideObject::Shape {
        kind,
        x,
        y: 120.0,
        w: 160.0,
        h: 100.0,
        rotation,
        style: ShapeStyle { fill: Some(fill), gradient: None, stroke: Some(Stroke { color: line, width: 3.0 }) },
    };
    let mut deck = Deck::new();
    deck.slides[0].objects = vec![
        shape(ShapeKind::Rect, 20.0, 0.0, Color(0xE0, 0x1B, 0x24), Color(0x10, 0x20, 0x30)),
        shape(ShapeKind::RoundRect { radius: 0.25 }, 200.0, 30.0, Color(0x26, 0xA2, 0x69), Color(0x40, 0x00, 0x40)),
        shape(ShapeKind::Ellipse, 380.0, 0.0, Color(0x35, 0x84, 0xE4), Color(0x80, 0x40, 0x00)),
        shape(ShapeKind::Triangle, 560.0, 315.0, Color(0xF6, 0xD3, 0x2D), Color(0x00, 0x00, 0x00)),
        shape(ShapeKind::Diamond, 740.0, 90.0, Color(0x91, 0x41, 0xAC), Color(0xFF, 0xFF, 0xFF)),
    ];
    let want = deck.slides[0].objects.clone();
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("shapes.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
        let back = convert(&path, ext).unwrap_or_else(|e| panic!("Impress rewrites our {ext}: {e}"));
        let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
        let got = &read.slides[0].objects;
        assert_eq!(got.len(), want.len(), "{ext}: the shapes came back as {got:?}");
        for (w, g) in want.iter().zip(got) {
            let (
                SlideObject::Shape { kind: wk, x: wx, y: wy, w: ww, h: wh, rotation: wr, style: ws },
                SlideObject::Shape { kind: gk, x: gx, y: gy, w: gw, h: gh, rotation: gr, style: gs },
            ) = (w, g)
            else {
                panic!("{ext}: {w:?} came back as {g:?}")
            };
            let what = format!("{ext} {wk:?}");
            match (wk, gk) {
                (ShapeKind::RoundRect { radius: a }, ShapeKind::RoundRect { radius: b }) => {
                    assert!((a - b).abs() < 0.01, "{what}: corner radius {b}")
                }
                _ => assert_eq!(wk, gk, "{what}: the preset"),
            }
            assert_eq!(gs.fill, ws.fill, "{what}: the fill");
            let (Some(wl), Some(gl)) = (&ws.stroke, &gs.stroke) else { panic!("{what}: the outline came back as {:?}", gs.stroke) };
            assert_eq!(gl.color, wl.color, "{what}: the outline colour");
            assert!((gl.width - wl.width).abs() < 0.1, "{what}: the outline width {}", gl.width);
            let turn = (gr - wr).rem_euclid(360.0);
            assert!(turn < 0.1 || turn > 359.9, "{what}: rotation {gr}, wrote {wr}");
            for (a, b, n) in [(gx, wx, "x"), (gy, wy, "y"), (gw, ww, "w"), (gh, wh, "h")] {
                assert!((a - b).abs() < 0.5, "{what}: {n} {a}, wrote {b}");
            }
        }
    }
}

/// Text and run styling across the format boundary, both ways
/// (docs/INTEROP-EVIDENCE.md, #1276).
///
/// The run-styling tests above rewrite a file in its own format, so the
/// pptx Impress writes from its own model, which is what our pptx reader
/// meets in the wild, had only been read for pictures, notes and
/// geometry: here it carries the text and the runs, from our odp. The
/// other direction reads Impress's odp from our pptx.
#[test]
fn text_and_run_styling_survive_a_conversion_between_the_two_formats() {
    use decks_core::odp;
    if !require_or_skip() { return; }
    let mut deck = Deck::new();
    deck.slides = vec![styled_run_slide(vec![
        Run { text: "plain ".into(), style: RunStyle::default() },
        Run { text: "bold".into(), style: RunStyle { bold: true, ..Default::default() } },
        Run { text: " and ".into(), style: RunStyle::default() },
        Run { text: "red".into(), style: RunStyle { color: Some("cc0000".into()), ..Default::default() } },
    ])];
    let check = |rt: &Deck, what: &str| {
        let runs = runs_of(&rt.slides[0]);
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text.trim(), "plain bold and red", "the text after {what}: {runs:?}");
        let bold: String = runs.iter().filter(|r| r.style.bold).map(|r| r.text.as_str()).collect();
        assert_eq!(bold.trim(), "bold", "bold after {what}: {runs:?}");
        let red = runs.iter().find(|r| r.text.contains("red")).expect("the red run");
        assert_eq!(red.style.color.as_deref().map(str::to_lowercase), Some("cc0000".into()), "colour after {what}: {runs:?}");
    };
    let dir = tempfile::tempdir().unwrap();

    let as_odp = dir.path().join("styled.odp");
    odp::write(&deck, as_odp.to_str().unwrap()).expect("write odp");
    let to_pptx = convert(&as_odp, "pptx").expect("Impress could not convert our odp to pptx");
    check(&read_pptx(to_pptx.to_str().unwrap()).expect("read Impress's pptx"), "odp -> Impress -> pptx");

    let sub = dir.path().join("b");
    std::fs::create_dir_all(&sub).unwrap();
    let as_pptx = sub.join("styled.pptx");
    write_pptx(as_pptx.to_str().unwrap(), &deck).expect("write pptx");
    let to_odp = convert(&as_pptx, "odp").expect("Impress could not convert our pptx to odp");
    check(&odp::read(to_odp.to_str().unwrap()).expect("read Impress's odp"), "pptx -> Impress -> odp");
}

/// A merged table cell survives Impress rewriting our pptx: the title still
/// spans the row, and the cells it covers are still covered.
#[test]
fn a_merged_table_cell_survives_impress() {
    if !require_or_skip() {
        return;
    }
    use decks_core::engine::table::{TableCell, TableData};
    let cell = |t: &str| TableCell { runs: vec![letters_core::model::Run::plain(t)], ..Default::default() };
    let rows = vec![
        vec![TableCell { col_span: 2, ..cell("Title") }, TableCell { covered: true, ..Default::default() }],
        vec![cell("a"), cell("b")],
    ];
    let table = TableData { col_widths: vec![200.0, 200.0], row_heights: vec![40.0, 40.0], rows, first_row: true, ..Default::default() };
    let mut deck = Deck::new();
    deck.slides[0].objects = vec![SlideObject::Table { x: 100.0, y: 100.0, w: 400.0, h: 80.0, rotation: 0.0, table }];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spans.pptx");
    write_pptx(path.to_str().unwrap(), &deck).expect("write");
    let back = convert(&path, "pptx").expect("Impress rewrites our pptx");
    let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
    let t = read.slides[0].objects.iter().find_map(|o| match o { SlideObject::Table { table, .. } => Some(table.clone()), _ => None }).expect("a table");
    assert_eq!(t.rows[0][0].span(), (2, 1), "{:?}", t.rows[0]);
    assert!(t.rows[0][1].covered, "{:?}", t.rows[0]);
    assert_eq!(t.rows[0][0].text(), "Title");
}

/// A freeform survives Impress rewriting it in either format: it comes
/// back with its own paths, not as its box.
#[test]
fn a_freeform_survives_impress_in_both_formats() {
    if !require_or_skip() {
        return;
    }
    use decks_core::engine::freeform::{FreePath, PathCmd};
    use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
    let wedge = FreePath {
        w: 1000.0,
        h: 1000.0,
        cmds: vec![PathCmd::Move(500.0, 500.0), PathCmd::Line(1000.0, 500.0), PathCmd::Arc { wr: 500.0, hr: 500.0, start: 0.0, swing: 90.0 }, PathCmd::Close],
        fill: true,
        stroke: true,
    };
    let mut deck = Deck::new();
    deck.slides[0].objects = vec![SlideObject::Shape {
        kind: ShapeKind::Freeform(vec![wedge]),
        x: 100.0,
        y: 100.0,
        w: 200.0,
        h: 200.0,
        rotation: 0.0,
        style: ShapeStyle { fill: Some(Color(0x44, 0x72, 0xC4)), gradient: None, stroke: None },
    }];
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("freeform.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).expect("write");
        let back = convert(&path, ext).unwrap_or_else(|e| panic!("Impress rewrites our {ext}: {e}"));
        let read = decks_core::read_deck(back.to_str().unwrap()).expect("read Impress's file");
        let kinds: Vec<_> = read.slides[0].objects.iter().filter_map(|o| match o { SlideObject::Shape { kind, .. } => Some(kind.clone()), _ => None }).collect();
        assert!(
            matches!(kinds.as_slice(), [ShapeKind::Freeform(p)] if p.len() == 1 && p[0].cmds.len() >= 3),
            "{ext}: {kinds:?}"
        );
    }
}
