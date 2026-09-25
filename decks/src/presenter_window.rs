//! presenter_window.rs — a slide show: the audience's fullscreen slides and
//! the presenter display (current and next slide, notes, clock).
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! docs/DESIGN-UI.md, "Presenter display" (Keynote). Which monitor shows
//! what, the slide order, the clock and its text are decks_core::presenter;
//! this module builds the two windows and wires keys and buttons. With a
//! second monitor the audience window goes there and the presenter display
//! stays on the laptop; with one, the audience window alone; "Rehearse"
//! opens the presenter display alone. The presenter display is an
//! AdwWindow whose layout turns vertical under an AdwBreakpoint.

use adw::prelude::*;
use decks_core::engine::Deck;
use decks_core::presenter::{format_elapsed, show_layout, slide_counter, PresenterState};
use gtk4::{self as gtk, gdk, glib};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::canvas::{draw_slide_in, draw_slide_show, Chrome};
use crate::transition::{draw_transition, TransitionState, TransitionType};

/// One running show.
struct Show {
    deck: Deck,
    state: RefCell<PresenterState>,
    transition: Rc<RefCell<TransitionState>>,
    /// Filled in once the windows exist (they refer back to the show).
    audience: RefCell<Option<(gtk::Window, gtk::DrawingArea)>>,
    presenter: RefCell<Option<Presenter>>,
}

/// The presenter display's live parts.
struct Presenter {
    window: adw::Window,
    current: gtk::DrawingArea,
    next: gtk::DrawingArea,
    counter: gtk::Label,
    clock: gtk::Label,
    notes: gtk::Label,
}

fn monitor(n: usize) -> Option<gdk::Monitor> {
    let monitors = gdk::Display::default()?.monitors();
    monitors.item(n as u32).and_downcast::<gdk::Monitor>()
}

fn monitor_count() -> usize {
    gdk::Display::default().map_or(1, |d| d.monitors().n_items() as usize)
}

impl Show {
    fn index(&self) -> usize {
        self.state.borrow().current_index()
    }

    /// Everything that shows the current slide, brought up to date.
    fn refresh(&self) {
        let i = self.index();
        if let Some((_, area)) = self.audience.borrow().as_ref() {
            area.queue_draw();
        }
        if let Some(p) = self.presenter.borrow().as_ref() {
            p.current.queue_draw();
            p.next.queue_draw();
            p.counter.set_text(&slide_counter(i, self.deck.slides.len()));
            let notes = self.deck.slides.get(i).map(|s| s.notes.trim().to_string()).unwrap_or_default();
            p.notes.set_text(if notes.is_empty() { "No notes for this slide" } else { &notes });
            p.notes.set_css_classes(if notes.is_empty() { &["dim-label"] } else { &[] });
        }
    }

    /// Move to slide `to`, playing its transition where the audience sees
    /// it (going back plays the slide being left's, as the editor does).
    fn go(&self, to: usize) {
        let from = self.index();
        if !self.state.borrow_mut().go_to(to, &self.deck) {
            return;
        }
        if let Some((_, area)) = self.audience.borrow().as_ref() {
            let (a, b) = (&self.deck.slides[from], &self.deck.slides[to]);
            let kind = TransitionType::of(if to > from { b.transition } else { a.transition });
            self.transition.borrow_mut().chrome = Chrome::Show;
            TransitionState::start(&self.transition, kind, a, b, &self.deck.masters, area);
        }
        self.refresh();
    }

    fn step(&self, forward: bool) {
        let i = self.index();
        if forward {
            self.go(i + 1);
        } else if i > 0 {
            self.go(i - 1);
        }
    }

    /// Close both windows. Taking them out of the show also breaks the
    /// cycle show → window → close handler → show, so the show is freed.
    fn end(&self) {
        let audience = self.audience.borrow_mut().take();
        let presenter = self.presenter.borrow_mut().take();
        if let Some((w, _)) = audience {
            w.close();
        }
        if let Some(p) = presenter {
            p.window.close();
        }
    }

    /// The presenter display's clock, once a second while it is open.
    fn tick(&self) -> bool {
        let Some(p) = self.presenter.borrow().as_ref().map(|p| p.clock.clone()) else { return false };
        if let Some(snap) = self.state.borrow().snapshot_at(&self.deck, Instant::now()) {
            p.set_text(&format_elapsed(snap.elapsed));
        }
        true
    }
}

/// Keys that run a show, on either window.
fn add_keys(widget: &impl IsA<gtk::Widget>, show: &Rc<Show>) {
    let key = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(show);
    key.connect_key_pressed(move |_, keyval, _, _| {
        let Some(show) = weak.upgrade() else { return glib::Propagation::Proceed };
        use gdk::Key;
        match keyval {
            Key::Right | Key::Down | Key::space | Key::Page_Down | Key::Return | Key::n => show.step(true),
            Key::Left | Key::Up | Key::Page_Up | Key::BackSpace | Key::p => show.step(false),
            Key::Home => show.go(0),
            Key::End => show.go(show.deck.slides.len().saturating_sub(1)),
            Key::Escape => show.end(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    widget.add_controller(key);
}

/// A drawing area showing slide `offset` after the current one.
fn slide_area(show: &Rc<Show>, offset: usize, label: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.update_property(&[gtk::accessible::Property::Label(label)]);
    let weak = Rc::downgrade(show);
    area.set_draw_func(move |_, cr, w, h| {
        let Some(show) = weak.upgrade() else { return };
        let i = show.index() + offset;
        if i < show.deck.slides.len() {
            draw_slide_in(cr, w as f64, h as f64, &show.deck.slides, i, &show.deck.masters, Chrome::Preview);
        }
        // After the last slide there is no next one: the area stays empty.
    });
    area
}

fn build_audience(app: &adw::Application, show: &Rc<Show>) -> (gtk::Window, gtk::DrawingArea) {
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.update_property(&[gtk::accessible::Property::Label("Slide show")]);
    let weak = Rc::downgrade(show);
    area.set_draw_func(move |_, cr, w, h| {
        let Some(show) = weak.upgrade() else { return };
        if draw_transition(cr, &show.transition.borrow(), w as f64, h as f64) {
            return;
        }
        draw_slide_show(cr, w as f64, h as f64, &show.deck.slides, show.index(), &show.deck.masters);
    });
    let window = gtk::Window::builder().application(app).title("Slide Show").decorated(false).child(&area).build();
    // Clicking the slide advances, as in every presentation app.
    let click = gtk::GestureClick::new();
    let weak = Rc::downgrade(show);
    click.connect_released(move |_, _, _, _| {
        if let Some(show) = weak.upgrade() {
            show.step(true);
        }
    });
    area.add_controller(click);
    (window, area)
}

fn build_presenter(app: &adw::Application, show: &Rc<Show>) -> Presenter {
    let current = slide_area(show, 0, "Current slide");
    current.set_hexpand(true);
    current.set_vexpand(true);
    current.set_size_request(320, 180);
    let next = slide_area(show, 1, "Next slide");
    next.set_size_request(240, 135);

    let heading = |t: &str| {
        let l = gtk::Label::new(Some(t));
        l.add_css_class("heading");
        l.set_halign(gtk::Align::Start);
        l
    };
    let clock = gtk::Label::new(Some("0:00"));
    clock.add_css_class("title-1");
    clock.add_css_class("numeric");
    clock.set_halign(gtk::Align::Start);
    let counter = gtk::Label::new(None);
    counter.add_css_class("dim-label");
    counter.set_halign(gtk::Align::Start);
    let notes = gtk::Label::new(None);
    notes.set_wrap(true);
    notes.set_xalign(0.0);
    notes.set_yalign(0.0);
    notes.add_css_class("body");
    let notes_scroll = gtk::ScrolledWindow::builder().child(&notes).vexpand(true).min_content_height(120).build();

    let side = gtk::Box::new(gtk::Orientation::Vertical, 12);
    side.set_width_request(280);
    side.append(&clock);
    side.append(&counter);
    side.append(&heading("Next"));
    side.append(&next);
    side.append(&heading("Notes"));
    side.append(&notes_scroll);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    body.set_margin_top(18);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.append(&current);
    body.append(&side);

    let header = adw::HeaderBar::new();
    let prev = gtk::Button::from_icon_name("go-previous-symbolic");
    prev.set_tooltip_text(Some("Previous Slide"));
    prev.update_property(&[gtk::accessible::Property::Label("Previous Slide")]);
    let fwd = gtk::Button::from_icon_name("go-next-symbolic");
    fwd.set_tooltip_text(Some("Next Slide"));
    fwd.update_property(&[gtk::accessible::Property::Label("Next Slide")]);
    let nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    nav.add_css_class("linked");
    nav.append(&prev);
    nav.append(&fwd);
    header.pack_start(&nav);
    let end = gtk::Button::with_label("End Show");
    header.pack_end(&end);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&body));

    let window = adw::Window::builder()
        .application(app)
        .title("Presenter Display")
        .default_width(1000)
        .default_height(640)
        .width_request(360)
        .height_request(360)
        .content(&view)
        .build();
    // A narrow window stacks the current slide above the rest.
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 700sp").expect("valid condition"));
    narrow.add_setter(&body, "orientation", Some(&gtk::Orientation::Vertical.to_value()));
    window.add_breakpoint(narrow);

    for (button, forward) in [(&prev, false), (&fwd, true)] {
        let weak = Rc::downgrade(show);
        button.connect_clicked(move |_| {
            if let Some(show) = weak.upgrade() {
                show.step(forward);
            }
        });
    }
    let weak = Rc::downgrade(show);
    end.connect_clicked(move |_| {
        if let Some(show) = weak.upgrade() {
            show.end();
        }
    });
    Presenter { window, current, next, counter, clock, notes }
}

/// Start a show of `deck` from slide `start`. `rehearse` opens the
/// presenter display alone.
pub fn start(app: &adw::Application, deck: Deck, start: usize, rehearse: bool) {
    if deck.slides.is_empty() {
        return;
    }
    let layout = show_layout(monitor_count(), rehearse);
    let mut state = PresenterState::new();
    state.go_to(start.min(deck.slides.len() - 1), &deck);
    state.start_at(Instant::now());
    let show = Rc::new(Show {
        deck,
        state: RefCell::new(state),
        transition: Rc::new(RefCell::new(TransitionState::new())),
        audience: RefCell::new(None),
        presenter: RefCell::new(None),
    });
    if let Some(m) = layout.audience {
        let (window, area) = build_audience(app, &show);
        add_keys(&window, &show);
        // Closing either window ends the show; the handler's strong
        // reference is what keeps the show alive until then.
        let s = show.clone();
        window.connect_close_request(move |_| {
            s.end();
            glib::Propagation::Proceed
        });
        match monitor(m) {
            Some(mon) => window.fullscreen_on_monitor(&mon),
            None => window.fullscreen(),
        }
        window.present();
        *show.audience.borrow_mut() = Some((window, area));
    }
    if let Some(m) = layout.presenter {
        let p = build_presenter(app, &show);
        add_keys(&p.window, &show);
        let s = show.clone();
        p.window.connect_close_request(move |_| {
            s.end();
            glib::Propagation::Proceed
        });
        // GTK can't place a normal window on a monitor; the window manager
        // opens it where the user is, which is the laptop (monitor `m`)
        // while the audience window is fullscreen on the other one.
        let _ = m;
        p.window.present();
        *show.presenter.borrow_mut() = Some(p);
        let weak = Rc::downgrade(&show);
        glib::timeout_add_seconds_local(1, move || {
            let alive = weak.upgrade().is_some_and(|s| s.tick());
            if alive { glib::ControlFlow::Continue } else { glib::ControlFlow::Break }
        });
    }
    show.refresh();
}
