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
    /// Builds of the current slide already played (decks_core::builds).
    build_step: usize,
    started_at: Option<Instant>,
    display: DisplayTarget,
}

/// What one click of a show does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Advance {
    /// Plays build `n` (0-based) of the current slide.
    Build(usize),
    /// Moves to the next slide.
    Slide(usize),
}

impl Default for PresenterState {
    fn default() -> Self { Self::new() }
}

impl PresenterState {
    pub fn new() -> Self {
        Self { current_index: 0, build_step: 0, started_at: None, display: DisplayTarget::Primary }
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
        let Some(to) = next_shown(deck, self.current_index) else { return false };
        self.current_index = to;
        true
    }

    /// Jump to slide `index` (a show starts at the slide being edited),
    /// before any of its builds.
    pub fn go_to(&mut self, index: usize, deck: &Deck) -> bool {
        if index >= deck.slides.len() || index == self.current_index { return false; }
        self.current_index = index;
        self.build_step = 0;
        true
    }

    /// Builds of the current slide already played.
    pub fn build_step(&self) -> usize { self.build_step }

    /// One click: the current slide's next build if it has one left,
    /// else the next slide; `None` at the end of the show.
    pub fn advance(&mut self, deck: &Deck) -> Option<Advance> {
        let slide = deck.slides.get(self.current_index)?;
        if self.build_step < crate::builds::steps(slide) {
            self.build_step += 1;
            return Some(Advance::Build(self.build_step - 1));
        }
        let to = next_shown(deck, self.current_index)?;
        self.current_index = to;
        self.build_step = 0;
        Some(Advance::Slide(self.current_index))
    }

    /// One click back: undo the last build played, or go to the previous
    /// slide fully built (as Keynote does). Returns whether anything moved.
    pub fn back(&mut self, deck: &Deck) -> bool {
        if self.build_step > 0 {
            self.build_step -= 1;
            return true;
        }
        let Some(to) = previous_shown(deck, self.current_index) else { return false };
        self.current_index = to;
        self.build_step = deck.slides.get(self.current_index).map_or(0, crate::builds::steps);
        true
    }

    pub fn previous(&mut self, deck: &Deck) -> bool {
        let Some(to) = previous_shown(deck, self.current_index) else { return false };
        self.current_index = to;
        true
    }

    pub fn snapshot_at(&self, deck: &Deck, now: Instant) -> Option<PresenterSnapshot> {
        let current = deck.slides.get(self.current_index)?;
        let next_index = next_shown(deck, self.current_index);
        let next = next_index.and_then(|i| deck.slides.get(i));
        let elapsed = self.started_at.map(|start| now.saturating_duration_since(start)).unwrap_or_default();
        Some(PresenterSnapshot {
            current_index: self.current_index,
            current_title: current.title.clone(),
            current_notes: current.notes.clone(),
            next_index,
            next_title: next.map(|slide| slide.title.clone()),
            elapsed,
            display: self.display.clone(),
        })
    }
}

/// The shown slide after `index`, stepping over hidden ones
/// (`engine::shown_slides`).
fn next_shown(deck: &Deck, index: usize) -> Option<usize> {
    crate::engine::shown_slides(&deck.slides).into_iter().find(|&i| i > index)
}

/// The shown slide before `index`, stepping over hidden ones.
fn previous_shown(deck: &Deck, index: usize) -> Option<usize> {
    crate::engine::shown_slides(&deck.slides).into_iter().rev().find(|&i| i < index)
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

/// Where a show is: "Slide 3 of 12", and on a slide with builds how many
/// have played, "Slide 3 of 12 · Build 1 of 4".
pub fn show_position(index: usize, count: usize, step: usize, steps: usize) -> String {
    if steps == 0 {
        slide_counter(index, count)
    } else {
        format!("{} · Build {} of {}", slide_counter(index, count), step, steps)
    }
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

/// Where a show goes when the presenter chose the audience's display
/// (Preferences ▸ Presentation Display, ADR 0004's "explicit external
/// display selection"). The chosen monitor gets the slides and the
/// presenter display goes on another one, the first that isn't it; with
/// only the chosen one, the slides alone. A choice that isn't connected,
/// or none, is [`show_layout`]'s automatic one. Rehearsing ignores it.
pub fn show_layout_on(monitors: usize, rehearse: bool, chosen: Option<usize>) -> ShowLayout {
    match chosen {
        Some(m) if !rehearse && m < monitors => {
            ShowLayout { audience: Some(m), presenter: (0..monitors).find(|&p| p != m) }
        }
        _ => show_layout(monitors, rehearse),
    }
}

/// What a running show does when the display's monitors change: `None`
/// while every window it uses still has its monitor, else the layout with
/// each lost window back on the primary one (0). ADR 0004, "Presenter
/// display": "If external display disappears, return to primary and show
/// a visible status" ([`DISPLAY_LOST`]). A monitor coming back changes
/// nothing: the show stays where the presenter can see it.
pub fn layout_after_monitor_change(layout: ShowLayout, monitors: usize) -> Option<ShowLayout> {
    let back = |m: Option<usize>| m.map(|m| if m >= monitors { 0 } else { m });
    let after = ShowLayout { audience: back(layout.audience), presenter: back(layout.presenter) };
    (after != layout).then_some(after)
}

/// The status a show shows when its external display goes away.
pub const DISPLAY_LOST: &str = "The external display was disconnected. The show continues on this screen.";

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

    #[test]
    fn a_chosen_display_gets_the_slides_and_the_presenter_display_another() {
        // Projector on the laptop's own screen, presenter display on the other.
        assert_eq!(show_layout_on(2, false, Some(0)), ShowLayout { audience: Some(0), presenter: Some(1) });
        assert_eq!(show_layout_on(3, false, Some(2)), ShowLayout { audience: Some(2), presenter: Some(0) });
        assert_eq!(show_layout_on(1, false, Some(0)), ShowLayout { audience: Some(0), presenter: None });
        // Not connected, or automatic: the usual layout.
        assert_eq!(show_layout_on(2, false, Some(5)), show_layout(2, false));
        assert_eq!(show_layout_on(2, false, None), show_layout(2, false));
        assert_eq!(show_layout_on(2, true, Some(1)), show_layout(2, true), "a rehearsal is the presenter display alone");
        // And losing the chosen display falls back like any other.
        assert_eq!(
            layout_after_monitor_change(show_layout_on(3, false, Some(2)), 2),
            Some(ShowLayout { audience: Some(0), presenter: Some(0) })
        );
    }

    #[test]
    fn a_lost_external_display_brings_the_show_back_to_the_primary_one() {
        let two = show_layout(2, false);
        assert_eq!(two, ShowLayout { audience: Some(1), presenter: Some(0) });
        assert_eq!(layout_after_monitor_change(two, 2), None, "nothing lost");
        assert_eq!(layout_after_monitor_change(two, 3), None, "a monitor added changes nothing");
        assert_eq!(
            layout_after_monitor_change(two, 1),
            Some(ShowLayout { audience: Some(0), presenter: Some(0) }),
            "the slides come back to the laptop, the presenter display stays"
        );
        // Unplugged with no monitor reported at all, still the primary.
        assert_eq!(layout_after_monitor_change(two, 0), Some(ShowLayout { audience: Some(0), presenter: Some(0) }));
        // A one-monitor show or a rehearsal has nothing to lose.
        assert_eq!(layout_after_monitor_change(show_layout(1, false), 1), None);
        assert_eq!(layout_after_monitor_change(show_layout(2, true), 1), None);
    }

    fn deck() -> Deck {
        Deck {
            slides: vec![
                Slide { title: "Opening".into(), background: "#fff".into(), background_image: None, hidden: false, objects: vec![], notes: "Welcome".into(), master_idx: Some(0), transition: Default::default(), builds: Vec::new(), ids: Default::default(), layout: None },
                Slide { title: "Details".into(), background: "#fff".into(), background_image: None, hidden: false, objects: vec![], notes: "Explain this".into(), master_idx: Some(0), transition: Default::default(), builds: Vec::new(), ids: Default::default(), layout: None },
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
        assert_eq!(show_position(2, 12, 0, 0), "Slide 3 of 12");
        assert_eq!(show_position(2, 12, 1, 4), "Slide 3 of 12 · Build 1 of 4");
    }

    #[test]
    fn a_second_monitor_takes_the_audience_and_rehearsal_is_presenter_only() {
        assert_eq!(show_layout(1, false), ShowLayout { audience: Some(0), presenter: None });
        assert_eq!(show_layout(0, false), ShowLayout { audience: Some(0), presenter: None });
        assert_eq!(show_layout(2, false), ShowLayout { audience: Some(1), presenter: Some(0) });
        assert_eq!(show_layout(3, true), ShowLayout { audience: None, presenter: Some(0) });
    }

    #[test]
    fn clicks_play_a_slides_builds_before_moving_on_and_back_undoes_them() {
        use crate::builds::{Build, BuildEffect};
        let mut d = deck();
        d.slides[0].builds = vec![
            Build { object: 0, effect: BuildEffect::Appear, out: false },
            Build { object: 1, effect: BuildEffect::Dissolve, out: false },
        ];
        let mut s = PresenterState::new();
        assert_eq!(s.advance(&d), Some(Advance::Build(0)));
        assert_eq!(s.advance(&d), Some(Advance::Build(1)));
        assert_eq!(s.advance(&d), Some(Advance::Slide(1)));
        assert_eq!((s.current_index(), s.build_step()), (1, 0));
        assert_eq!(s.advance(&d), None, "the end of the show");
        assert!(s.back(&d));
        assert_eq!((s.current_index(), s.build_step()), (0, 2), "back to the slide as it was left");
        assert!(s.back(&d));
        assert_eq!(s.build_step(), 1);
        assert!(s.go_to(1, &d) && s.build_step() == 0);
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
        assert!(!state.previous(&deck));
        assert!(state.next(&deck));
        assert!(!state.next(&deck));
        assert!(state.previous(&deck));
        assert_eq!(state.current_index(), 0);
    }

    /// A show steps over a hidden slide both ways, and the presenter's
    /// "next" names the slide that will actually come.
    #[test]
    fn a_show_steps_over_a_hidden_slide() {
        let mut deck = deck();
        let mut third = deck.slides[1].clone();
        third.title = "Close".into();
        deck.slides.push(third);
        deck.slides[1].hidden = true;
        let mut state = PresenterState::new();
        let view = state.snapshot_at(&deck, Instant::now()).unwrap();
        assert_eq!((view.next_index, view.next_title.as_deref()), (Some(2), Some("Close")));
        assert_eq!(state.advance(&deck), Some(Advance::Slide(2)));
        assert_eq!(state.advance(&deck), None, "the end of the show");
        assert!(state.back(&deck));
        assert_eq!(state.current_index(), 0);
        assert!(state.next(&deck));
        assert_eq!(state.current_index(), 2);
        assert!(state.previous(&deck));
        assert_eq!(state.current_index(), 0);
    }

    #[test]
    fn missing_media_is_reported_by_slide_and_path() {
        let mut deck = deck();
        deck.slides[1].objects.push(SlideObject::Image { path: "/missing/video.mp4".into(), x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0, crop: Default::default() });
        assert_eq!(missing_media(&deck), vec![MissingMedia { slide_index: 1, path: "/missing/video.mp4".into() }]);
    }
}
