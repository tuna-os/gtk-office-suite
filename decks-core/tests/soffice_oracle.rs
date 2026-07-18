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
