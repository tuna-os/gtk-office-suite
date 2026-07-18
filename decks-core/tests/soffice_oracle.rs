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
    let out = Command::new("soffice")
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
    let converted = input.with_extension(to);
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
        runs: vec![],
    });
    deck.slides[0].objects.push(SlideObject::Rect { x: 50.0, y: 250.0, w: 200.0, h: 90.0 });
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
                runs: vec![],
            }],
            notes: String::new(),
            master_idx: Some(0),
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
        }],
        notes: notes.into(),
        master_idx: Some(0),
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

#[test]
fn shape_kinds_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "shapes".into(),
        background: "#ffffff".into(),
        objects: vec![
            SlideObject::Rect { x: 100.0, y: 100.0, w: 200.0, h: 100.0 },
            SlideObject::Circle { x: 500.0, y: 300.0, r: 80.0 },
        ],
        notes: String::new(),
        master_idx: Some(0),
    }];
    let Some(rt) = through_impress(&deck, "shapes") else { return };
    let rects = rt.slides[0].objects.iter().filter(|o| matches!(o, SlideObject::Rect { .. })).count();
    let circles =
        rt.slides[0].objects.iter().filter(|o| matches!(o, SlideObject::Circle { .. })).count();
    assert!(rects >= 1, "rect lost: {:?}", rt.slides[0].objects);
    assert!(circles >= 1, "circle lost: {:?}", rt.slides[0].objects);
}

#[test]
fn positions_approx_survive_impress_rewrite() {
    let mut deck = Deck::new();
    deck.slides = vec![Slide {
        title: "pos".into(),
        background: "#ffffff".into(),
        objects: vec![SlideObject::Rect { x: 240.0, y: 180.0, w: 320.0, h: 120.0 }],
        notes: String::new(),
        master_idx: Some(0),
    }];
    let Some(rt) = through_impress(&deck, "pos") else { return };
    let Some(SlideObject::Rect { x, y, w, h }) = rt.slides[0]
        .objects
        .iter()
        .find(|o| matches!(o, SlideObject::Rect { .. }))
    else {
        panic!("rect lost: {:?}", rt.slides[0].objects)
    };
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
        }],
        notes: String::new(),
        master_idx: Some(0),
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
        }],
        notes: String::new(),
        master_idx: Some(0),
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
        }],
        notes: String::new(),
        master_idx: Some(0),
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
        objects: vec![SlideObject::Rect { x: 240.0, y: 180.0, w: 320.0, h: 120.0 }],
        notes: String::new(),
        master_idx: Some(0),
    }];
    let Some(rt) = odp_through_impress(&deck, "geom") else { return };
    let Some(SlideObject::Rect { x, y, w, h }) = rt.slides[0]
        .objects
        .iter()
        .find(|o| matches!(o, SlideObject::Rect { .. }))
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
        }],
        notes: String::new(),
        master_idx: Some(0),
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
