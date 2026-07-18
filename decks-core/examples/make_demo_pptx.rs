// make_demo_pptx — generate the walkthrough demo presentation.
// Usage: cargo run -p decks-core --example make_demo_pptx -- <out.pptx>

use decks_core::engine::{write_pptx, Deck, Slide, SlideObject};

fn slide(title: &str, objects: Vec<SlideObject>, notes: &str) -> Slide {
    Slide {
        title: title.to_string(),
        background: "#ffffff".into(),
        objects,
        notes: notes.to_string(),
        master_idx: Some(0),
    }
}

fn text(text: &str, x: f64, y: f64, w: f64, h: f64) -> SlideObject {
    SlideObject::TextBox { text: text.to_string(), x, y, w, h, runs: vec![] }
}

fn main() -> Result<(), String> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "demo.pptx".into());
    let mut deck = Deck::new();
    deck.slides = vec![
        slide(
            "Title",
            vec![
                text("TunaOS Office Suite", 180.0, 180.0, 600.0, 80.0),
                text("Q2 2026 update", 180.0, 270.0, 400.0, 40.0),
                SlideObject::Rect { x: 180.0, y: 340.0, w: 240.0, h: 8.0 },
            ],
            "Welcome everyone — one-line agenda first.",
        ),
        slide(
            "Numbers",
            vec![
                text("Revenue grew 12% QoQ", 120.0, 120.0, 500.0, 50.0),
                text("Churn at a two-year low", 120.0, 200.0, 500.0, 50.0),
                SlideObject::Circle { x: 720.0, y: 300.0, r: 90.0 },
            ],
            "Pause on the churn number — it is the headline.",
        ),
        slide(
            "Roadmap",
            vec![
                text("Partner portal — July", 120.0, 120.0, 500.0, 40.0),
                text("Support team to 12 — August", 120.0, 180.0, 500.0, 40.0),
                text("SOC 2 audit — September", 120.0, 240.0, 500.0, 40.0),
            ],
            "",
        ),
    ];
    write_pptx(&out, &deck)?;
    println!("wrote {out}");
    Ok(())
}
