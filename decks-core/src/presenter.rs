//! GTK-free presenter state and presentation-readiness checks.
//!
//! The application owns the windows/displays; this module owns the state
//! contract that both presenter UI and deterministic tests consume.

use crate::engine::{Deck, SlideObject};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisplayTarget {
    Primary,
    External(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenterSnapshot {
    pub current_index: usize,
    pub current_title: String,
    pub current_notes: String,
    pub next_index: Option<usize>,
    pub next_title: Option<String>,
    pub elapsed: Duration,
    pub display: DisplayTarget,
}

#[derive(Clone, Debug)]
pub struct PresenterState {
    current_index: usize,
    started_at: Option<Instant>,
    display: DisplayTarget,
}

impl Default for PresenterState {
    fn default() -> Self { Self::new() }
}

impl PresenterState {
    pub fn new() -> Self {
        Self { current_index: 0, started_at: None, display: DisplayTarget::Primary }
    }

    pub fn current_index(&self) -> usize { self.current_index }
    pub fn display(&self) -> &DisplayTarget { &self.display }

    pub fn select_display(&mut self, display: DisplayTarget) {
        self.display = display;
    }

    pub fn start_at(&mut self, now: Instant) {
        if self.started_at.is_none() { self.started_at = Some(now); }
    }

    pub fn stop(&mut self) { self.started_at = None; }

    pub fn next(&mut self, deck: &Deck) -> bool {
        if self.current_index + 1 >= deck.slides.len() { return false; }
        self.current_index += 1;
        true
    }

    pub fn previous(&mut self) -> bool {
        if self.current_index == 0 { return false; }
        self.current_index -= 1;
        true
    }

    pub fn snapshot_at(&self, deck: &Deck, now: Instant) -> Option<PresenterSnapshot> {
        let current = deck.slides.get(self.current_index)?;
        let next = deck.slides.get(self.current_index + 1);
        let elapsed = self.started_at.map(|start| now.saturating_duration_since(start)).unwrap_or_default();
        Some(PresenterSnapshot {
            current_index: self.current_index,
            current_title: current.title.clone(),
            current_notes: current.notes.clone(),
            next_index: next.map(|_| self.current_index + 1),
            next_title: next.map(|slide| slide.title.clone()),
            elapsed,
            display: self.display.clone(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingMedia {
    pub slide_index: usize,
    pub path: String,
}

/// Return media that would make a presentation incomplete. Missing media is
/// a structured readiness error so the UI can identify each file and offer a
/// repair action; it must not become a silent blank slide.
pub fn missing_media(deck: &Deck) -> Vec<MissingMedia> {
    deck.slides.iter().enumerate().flat_map(|(slide_index, slide)| {
        slide.objects.iter().filter_map(move |object| {
            let SlideObject::Image { path, .. } = object else { return None; };
            (!Path::new(path).is_file()).then(|| MissingMedia { slide_index, path: path.clone() })
        })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Deck, Slide};

    fn deck() -> Deck {
        Deck {
            slides: vec![
                Slide { title: "Opening".into(), background: "#fff".into(), objects: vec![], notes: "Welcome".into(), master_idx: Some(0) },
                Slide { title: "Details".into(), background: "#fff".into(), objects: vec![], notes: "Explain this".into(), master_idx: Some(0) },
            ],
            masters: vec![],
        }
    }

    #[test]
    fn presenter_snapshot_contains_notes_next_slide_and_timer() {
        let mut state = PresenterState::new();
        let start = Instant::now();
        state.start_at(start);
        state.select_display(DisplayTarget::External(1));
        let view = state.snapshot_at(&deck(), start + Duration::from_secs(12)).unwrap();
        assert_eq!(view.current_title, "Opening");
        assert_eq!(view.current_notes, "Welcome");
        assert_eq!(view.next_title.as_deref(), Some("Details"));
        assert_eq!(view.elapsed, Duration::from_secs(12));
        assert_eq!(view.display, DisplayTarget::External(1));
    }

    #[test]
    fn navigation_stays_within_deck() {
        let deck = deck();
        let mut state = PresenterState::new();
        assert!(!state.previous());
        assert!(state.next(&deck));
        assert!(!state.next(&deck));
        assert!(state.previous());
        assert_eq!(state.current_index(), 0);
    }

    #[test]
    fn missing_media_is_reported_by_slide_and_path() {
        let mut deck = deck();
        deck.slides[1].objects.push(SlideObject::Image { path: "/missing/video.mp4".into(), x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        assert_eq!(missing_media(&deck), vec![MissingMedia { slide_index: 1, path: "/missing/video.mp4".into() }]);
    }
}
