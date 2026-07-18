// lo_parity.rs — LibreOffice-authored parity for Decks (ratcheted).
//
// Impress has no cheap authoring input like Writer's HTML, so scenarios go
// *through* the oracle: we write a .pptx, headless Impress imports it and
// re-exports pptx (rewriting every part in LO's own grammar), and our
// reader must then read LibreOffice's version back with content intact.
// This measures exactly the thing users hit: files that lived in another
// office suite. Pass count ratchets via corpus/lo-parity-baseline.txt.
//
// Skips without soffice unless REQUIRE_SOFFICE=1 (CI sets it).

use std::process::Command;

use decks_core::engine::{read_pptx, write_pptx, Deck, Slide, SlideObject};

struct Scenario {
    name: &'static str,
    deck: Deck,
    check: fn(&Deck) -> Result<(), String>,
}

fn slide(title: &str, objects: Vec<SlideObject>) -> Slide {
    Slide {
        title: title.to_string(),
        background: "#ffffff".into(),
        objects,
        notes: String::new(),
        master_idx: Some(0),
    }
}

fn text_box(text: &str) -> SlideObject {
    SlideObject::TextBox { text: text.to_string(), x: 100.0, y: 100.0, w: 400.0, h: 80.0 , runs: vec![] }
}

fn all_text(deck: &Deck) -> Vec<String> {
    let mut out = Vec::new();
    for s in &deck.slides {
        for o in &s.objects {
            if let SlideObject::TextBox { text, .. } = o {
                out.push(text.clone());
            }
        }
    }
    out
}

fn expect_texts(deck: &Deck, wanted: &[&str]) -> Result<(), String> {
    let texts = all_text(deck);
    for w in wanted {
        if !texts.iter().any(|t| t.contains(w)) {
            return Err(format!("text {w:?} missing; got {texts:?}"));
        }
    }
    Ok(())
}

fn scenarios() -> Vec<Scenario> {
    let mut v = Vec::new();

    v.push(Scenario {
        name: "single-textbox",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(text_box("hello from decks"));
            d
        },
        check: |d| expect_texts(d, &["hello from decks"]),
    });

    v.push(Scenario {
        name: "three-slides",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(text_box("slide one"));
            d.slides.push(slide("Two", vec![text_box("slide two")]));
            d.slides.push(slide("Three", vec![text_box("slide three")]));
            d
        },
        check: |d| {
            if d.slides.len() != 3 {
                return Err(format!("slide count {} != 3", d.slides.len()));
            }
            expect_texts(d, &["slide one", "slide two", "slide three"])
        },
    });

    v.push(Scenario {
        name: "shapes-survive",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(SlideObject::Rect { x: 50.0, y: 50.0, w: 200.0, h: 100.0 });
            d.slides[0].objects.push(SlideObject::Circle { x: 400.0, y: 200.0, r: 60.0 });
            d.slides[0].objects.push(text_box("with shapes"));
            d
        },
        check: |d| {
            let objs = &d.slides[0].objects;
            let rects = objs.iter().filter(|o| matches!(o, SlideObject::Rect { .. })).count();
            let shapes = objs.len();
            if shapes < 3 {
                return Err(format!("only {shapes} objects survived (rects: {rects})"));
            }
            expect_texts(d, &["with shapes"])
        },
    });

    v.push(Scenario {
        name: "multiline-textbox",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(text_box("first line\nsecond line"));
            d
        },
        check: |d| expect_texts(d, &["first line", "second line"]),
    });

    v.push(Scenario {
        name: "unicode-text",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(text_box("café 中文 — “quotes” ✨"));
            d
        },
        check: |d| expect_texts(d, &["café", "中文"]),
    });

    v.push(Scenario {
        name: "speaker-notes",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(text_box("has notes"));
            d.slides[0].notes = "remember the demo".into();
            d
        },
        check: |d| {
            if d.slides[0].notes.contains("remember the demo") {
                Ok(())
            } else {
                Err(format!("notes lost: {:?}", d.slides[0].notes))
            }
        },
    });

    v.push(Scenario {
        name: "many-slides",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(text_box("s1"));
            for i in 2..=10 {
                d.slides.push(slide(&format!("S{i}"), vec![text_box(&format!("s{i}"))]));
            }
            d
        },
        check: |d| {
            if d.slides.len() == 10 { Ok(()) } else { Err(format!("{} slides != 10", d.slides.len())) }
        },
    });

    v.push(Scenario {
        name: "textbox-positions",
        deck: {
            let mut d = Deck::new();
            d.slides[0].objects.push(SlideObject::TextBox {
                text: "top-left".into(), x: 10.0, y: 10.0, w: 200.0, h: 40.0,
                runs: vec![],
            });
            d.slides[0].objects.push(SlideObject::TextBox {
                text: "lower".into(), x: 300.0, y: 400.0, w: 200.0, h: 40.0,
                runs: vec![],
            });
            d
        },
        check: |d| {
            // positions may shift units, but relative order must hold
            let mut ys = Vec::new();
            for o in &d.slides[0].objects {
                if let SlideObject::TextBox { text, y, .. } = o {
                    ys.push((text.clone(), *y));
                }
            }
            let top = ys.iter().find(|(t, _)| t.contains("top-left")).ok_or("top-left missing")?;
            let low = ys.iter().find(|(t, _)| t.contains("lower")).ok_or("lower missing")?;
            if top.1 < low.1 { Ok(()) } else { Err(format!("y-order wrong: {ys:?}")) }
        },
    });

    v.push(Scenario {
        name: "styled-runs",
        deck: {
            use letters_core::model::{Run, RunStyle};
            let mut d = Deck::new();
            d.slides[0].objects.push(SlideObject::TextBox {
                text: "plain bold italic".into(),
                x: 100.0, y: 100.0, w: 400.0, h: 60.0,
                runs: vec![
                    Run { text: "plain ".into(), style: RunStyle::default() },
                    Run { text: "bold ".into(), style: RunStyle { bold: true, ..Default::default() } },
                    Run { text: "italic".into(), style: RunStyle { italic: true, ..Default::default() } },
                ],
            });
            d
        },
        check: |d| {
            for s in &d.slides {
                for o in &s.objects {
                    if let SlideObject::TextBox { runs, .. } = o {
                        let bold = runs.iter().find(|r| r.text.contains("bold"));
                        let italic = runs.iter().find(|r| r.text.contains("italic"));
                        if let (Some(b), Some(i)) = (bold, italic) {
                            return if b.style.bold && i.style.italic {
                                Ok(())
                            } else {
                                Err(format!(
                                    "styles lost through Impress: bold={} italic={}",
                                    b.style.bold, i.style.italic
                                ))
                            };
                        }
                    }
                }
            }
            Err("styled text box missing".into())
        },
    });

    v
}

fn soffice_available() -> bool {
    Command::new("soffice").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn baseline() -> usize {
    include_str!("corpus/lo-parity-baseline.txt").trim().parse().expect("baseline int")
}

#[test]
fn libreoffice_parity_ratchet() {
    if !soffice_available() {
        if std::env::var("REQUIRE_SOFFICE").is_ok() {
            panic!("REQUIRE_SOFFICE set but no soffice binary found");
        }
        eprintln!("skipping: soffice not installed");
        return;
    }

    let scenarios = scenarios();
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("lo-profile");
    let out_dir = dir.path().join("lo-out");
    std::fs::create_dir(&out_dir).unwrap();

    // Write all inputs, then one batched soffice round-trip pptx→pptx.
    let mut inputs = Vec::new();
    for sc in &scenarios {
        let p = dir.path().join(format!("{}.pptx", sc.name));
        write_pptx(p.to_str().unwrap(), &sc.deck).expect("write pptx");
        inputs.push(p);
    }
    let out = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "pptx", "--outdir"])
        .arg(&out_dir)
        .args(&inputs)
        .output()
        .expect("run soffice");
    if !out.status.success() {
        panic!("batch conversion failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    let mut passed = 0usize;
    let mut failures = Vec::new();
    for sc in &scenarios {
        let lo_path = out_dir.join(format!("{}.pptx", sc.name));
        let verdict = if !lo_path.exists() {
            Err("Impress produced no output".to_string())
        } else {
            match read_pptx(lo_path.to_str().unwrap()) {
                Err(e) => Err(format!("our reader failed on LO output: {e}")),
                Ok(deck) => (sc.check)(&deck),
            }
        };
        match verdict {
            Ok(()) => passed += 1,
            Err(e) => failures.push((sc.name, e)),
        }
    }

    let total = scenarios.len();
    println!("\nDecks LO-authored parity: {passed}/{total}");
    for (name, e) in &failures {
        println!("  FAIL {name}: {e}");
    }
    let base = baseline();
    assert!(passed >= base, "REGRESSION: {passed}/{total} below baseline {base}");
    if passed > base {
        println!("IMPROVEMENT: {passed} > baseline {base} — bump tests/corpus/lo-parity-baseline.txt");
    }
}
