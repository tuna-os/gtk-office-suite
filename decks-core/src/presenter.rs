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

    /// Jump to slide `index` (a show starts at the slide being edited).
    pub fn go_to(&mut self, index: usize, deck: &Deck) -> bool {
        if index >= deck.slides.len() || index == self.current_index { return false; }
        self.current_index = index;
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

/// The presenter's clock: `m:ss`, or `h:mm:ss` from an hour on.
pub fn format_elapsed(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// "Slide 3 of 12".
pub fn slide_counter(index: usize, count: usize) -> String {
    format!("Slide {} of {}", index + 1, count)
}

/// Which monitor shows what, by index into the display's monitor list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShowLayout {
    /// The fullscreen slides the audience sees.
    pub audience: Option<usize>,
    /// The presenter display: current and next slide, notes, clock.
    pub presenter: Option<usize>,
}

/// Where a show goes. With a second monitor the audience gets it (the
/// usual projector setup) and the presenter display stays on the laptop;
/// with one monitor the audience has it to themselves. Rehearsing shows
/// the presenter display alone, on the first monitor.
pub fn show_layout(monitors: usize, rehearse: bool) -> ShowLayout {
    match (rehearse, monitors) {
        (true, _) => ShowLayout { audience: None, presenter: Some(0) },
        (false, 0 | 1) => ShowLayout { audience: Some(0), presenter: None },
        (false, _) => ShowLayout { audience: Some(1), presenter: Some(0) },
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
                Slide { title: "Opening".into(), background: "#fff".into(), objects: vec![], notes: "Welcome".into(), master_idx: Some(0), transition: Default::default() },
                Slide { title: "Details".into(), background: "#fff".into(), objects: vec![], notes: "Explain this".into(), master_idx: Some(0), transition: Default::default() },
            ],
            masters: vec![],
        }
    }

    #[test]
    fn the_clock_and_counter_read_as_a_presenter_expects() {
        assert_eq!(format_elapsed(Duration::from_secs(0)), "0:00");
        assert_eq!(format_elapsed(Duration::from_secs(65)), "1:05");
        assert_eq!(format_elapsed(Duration::from_secs(3600 + 62)), "1:01:02");
        assert_eq!(slide_counter(2, 12), "Slide 3 of 12");
    }

    #[test]
    fn a_second_monitor_takes_the_audience_and_rehearsal_is_presenter_only() {
        assert_eq!(show_layout(1, false), ShowLayout { audience: Some(0), presenter: None });
        assert_eq!(show_layout(0, false), ShowLayout { audience: Some(0), presenter: None });
        assert_eq!(show_layout(2, false), ShowLayout { audience: Some(1), presenter: Some(0) });
        assert_eq!(show_layout(3, true), ShowLayout { audience: None, presenter: Some(0) });
    }

    #[test]
    fn a_show_starts_at_the_slide_being_edited() {
        let d = deck();
        let mut state = PresenterState::new();
        assert!(state.go_to(1, &d));
        assert_eq!(state.current_index(), 1);
        assert!(!state.go_to(1, &d), "already there");
        assert!(!state.go_to(5, &d), "no such slide");
        assert!(!state.next(&d), "the last slide");
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
        deck.slides[1].objects.push(SlideObject::Image { path: "/missing/video.mp4".into(), x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0 });
        assert_eq!(missing_media(&deck), vec![MissingMedia { slide_index: 1, path: "/missing/video.mp4".into() }]);
    }
}
