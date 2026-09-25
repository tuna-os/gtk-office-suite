// notes_round_trip.rs — speaker notes, as the notes pane writes them
// (several paragraphs, a blank line, Unicode), come back exactly from our
// own pptx and odp. soffice_oracle.rs checks what LibreOffice keeps.
// SPDX-License-Identifier: GPL-3.0-or-later

use decks_core::engine::Deck;

const NOTES: &str = "Open with the question.\nThen the three numbers — 12, 40, 7.\n\nPause for questions; café at 15:00.";

fn deck() -> Deck {
    let mut d = Deck::new();
    d.slides[0].notes = NOTES.into();
    let mut second = d.slides[0].clone();
    second.notes = String::new();
    second.title = "Two".into();
    d.slides.push(second);
    d
}

#[test]
fn multi_paragraph_notes_come_back_exactly_from_pptx_and_odp() {
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("notes.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck()).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        assert_eq!(back.slides[0].notes, NOTES, "{ext}");
        assert_eq!(back.slides[1].notes, "", "{ext}: a slide without notes stays without");
        // And a second save keeps them as they are.
        let again = dir.path().join(format!("again.{ext}"));
        decks_core::write_deck(again.to_str().unwrap(), &back).unwrap();
        assert_eq!(decks_core::read_deck(again.to_str().unwrap()).unwrap().slides[0].notes, NOTES, "{ext}");
    }
}
