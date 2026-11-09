//! The console under the status bar: following the log, the grip that
//! resizes it, polling for new lines, and its CRT grain.

use super::*;

impl NotificationCenter {
    /// Only the user's own scrolling stops the console following its newest
    /// line (see inside).
    pub(super) fn follow_console_until_user_scrolls(&self, widgets: &NotificationCenterWidgets) {
        // Only the user's own scrolling stops the console following its
        // newest line. The text view moves the view by itself as it
        // measures lines (back to the top on its first layout, and when
        // old lines are cut), so a position read alone can't tell that
        // apart from the user scrolling up to read.
        let sw = widgets.console_scroll.clone();
        let adj = sw.vadjustment();
        // A button held in the console: what moves the view now is the
        // user (the scrollbar, a selection drag).
        let held = std::rc::Rc::new(std::cell::Cell::new(false));
        let follow = self.console_follow.clone();
        let h = held.clone();
        adj.connect_value_changed(move |a| {
            if a.value() + a.page_size() >= a.upper() - 1.0 {
                follow.set(true);
            } else if h.get() {
                follow.set(false);
            } else if follow.get() {
                a.set_value(a.upper() - a.page_size());
            }
        });
        let follow = self.console_follow.clone();
        adj.connect_changed(move |a| {
            if follow.get() {
                a.set_value(a.upper() - a.page_size());
            }
        });
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        wheel.set_propagation_phase(gtk::PropagationPhase::Capture);
        let follow = self.console_follow.clone();
        let a = adj.clone();
        wheel.connect_scroll(move |_, _, dy| {
            if dy < 0.0 && a.value() > a.lower() {
                follow.set(false);
            }
            glib::Propagation::Proceed
        });
        sw.add_controller(wheel);
        let input = gtk::EventControllerLegacy::new();
        input.set_propagation_phase(gtk::PropagationPhase::Capture);
        let follow = self.console_follow.clone();
        input.connect_event(move |_, ev| {
            use gtk::gdk::{EventType as E, Key};
            match ev.event_type() {
                E::ButtonPress | E::TouchBegin => held.set(true),
                E::ButtonRelease | E::TouchEnd | E::TouchCancel => held.set(false),
                // Keys are taken one at a time: a key held as the console
                // closes never sees its release here.
                E::KeyPress => {
                    let up = ev.downcast_ref::<gtk::gdk::KeyEvent>().is_some_and(|k| {
                        matches!(
                            k.keyval(),
                            Key::Up
                                | Key::KP_Up
                                | Key::Page_Up
                                | Key::KP_Page_Up
                                | Key::Home
                                | Key::KP_Home
                        )
                    });
                    if up && adj.value() > adj.lower() {
                        follow.set(false);
                    }
                }
                _ => {}
            }
            glib::Propagation::Proceed
        });
        sw.add_controller(input);
    }

    /// Dragging the grip resizes the console (see inside).
    pub(super) fn attach_grip_drag(&self, widgets: &NotificationCenterWidgets) {
        // Dragging the grip resizes the console; 160px stays the floor.
        // The pointer is tracked in ROOT coordinates: the grip itself
        // moves as the console grows, so grip-local offsets feed back
        // into the resize and jitter. A ceiling keeps the console from
        // demanding more height than the window can give (which flashes
        // as the layout fights itself).
        let drag = gtk::GestureDrag::new();
        let top = self.top.clone();
        let sw = widgets.console_scroll.clone();
        let grip = widgets.console_grip.clone();
        let start = std::rc::Rc::new(std::cell::Cell::new((160i32, 0f32)));
        let to_root_y = {
            let grip = grip.clone();
            move |x: f64, y: f64| -> Option<f32> {
                let root = grip.root()?;
                grip.compute_point(
                    root.upcast_ref::<gtk::Widget>(),
                    &gtk::graphene::Point::new(x as f32, y as f32),
                )
                .map(|p| p.y())
            }
        };
        {
            let sw = sw.clone();
            let start = start.clone();
            let to_root_y = to_root_y.clone();
            drag.connect_drag_begin(move |_, x, y| {
                if let Some(ry) = to_root_y(x, y) {
                    start.set((sw.min_content_height(), ry));
                }
            });
        }
        drag.connect_drag_update(move |g, ox, oy| {
            let Some((sx, sy)) = g.start_point() else {
                return;
            };
            let Some(now) = to_root_y(sx + ox, sy + oy) else {
                return;
            };
            let (start_h, start_y) = start.get();
            let ceiling = grip
                .root()
                .map(|r| r.height() - 340)
                .unwrap_or(i32::MAX)
                .max(160);
            // The grip sits on the console's far side from the bar:
            // below it with the bar at the top, above it at the foot.
            let moved = (now - start_y) as i32;
            let h = (start_h + if top.get() { moved } else { -moved }).clamp(160, ceiling);
            sw.set_min_content_height(h);
            sw.set_max_content_height(h);
        });
        widgets.console_grip.add_controller(drag);
    }

    pub(super) fn on_show_console(&mut self, sender: &ComponentSender<Self>) {
        // The console button (and shortcut) toggles: a second press
        // folds the console back into the plain status bar.
        if self.console_open {
            self.console_open = false;
            self.console_theme = false;
            if self.console_opened_bar {
                // The bar only opened to host the console; take it
                // back down with it.
                self.panel_open = false;
                self.transient_visible = false;
            }
            return;
        }
        let bar_was_closed = !self.panel_open && !self.transient_visible;
        self.console_opened_bar = bar_was_closed;
        self.console_theme = true;
        self.panel_open = true;
        if bar_was_closed {
            // Two-stage open: the bar slides down first; once it's
            // mapped, the console gets its own animated slide (set
            // together, the inner revealer starts unmapped and snaps).
            let s = sender.clone();
            glib::timeout_add_local_once(Duration::from_millis(140), move || {
                s.input(NotifyInput::RevealConsole);
            });
        } else {
            self.console_open = true;
            self.follow_console();
            sender.input(NotifyInput::PollConsole);
        }
    }

    pub(super) fn on_reveal_console(&mut self, sender: &ComponentSender<Self>) {
        if self.panel_open && !self.console_open {
            self.console_open = true;
            self.follow_console();
            sender.input(NotifyInput::PollConsole);
        }
    }

    pub(super) fn on_set_console_enabled(&mut self, on: bool) {
        self.console_enabled = on;
        if !on {
            self.console_open = false;
            self.console_theme = false;
        }
    }

    pub(super) fn on_poll_console(&mut self, sender: &ComponentSender<Self>) {
        if !self.console_open {
            return;
        }
        let (seq, lines) = crate::desktop::console_log::lines_since(self.console_seq);
        self.console_seq = seq;
        if !lines.is_empty() {
            // The adjustment's changed handler keeps a following
            // view on the newest line as these land.
            let mut end = self.console_buf.end_iter();
            for line in &lines {
                self.console_buf.insert(&mut end, line);
                self.console_buf.insert(&mut end, "\n");
            }
            // Keep the widget buffer bounded like the ring buffer.
            // Cutting from the top shifts whatever the user scrolled
            // up to read, so that waits until they are back at the
            // bottom, within a far looser ceiling.
            let lc = self.console_buf.line_count();
            let cap = if self.console_follow.get() {
                2200
            } else {
                20_000
            };
            if lc > cap {
                let mut start = self.console_buf.start_iter();
                if let Some(mut cut) = self.console_buf.iter_at_line(lc - 2000) {
                    self.console_buf.delete(&mut start, &mut cut);
                }
            }
        }
        // A fresh sprinkle of grain each tick keeps the CRT alive.
        if let Some(g) = &self.console_grain {
            g.queue_draw();
        }
        let s = sender.clone();
        glib::timeout_add_local_once(Duration::from_millis(350), move || {
            s.input(NotifyInput::PollConsole);
        });
    }
}

/// The CRT grain: a per-tick sprinkle of light and dark specks plus a soft
/// vignette. Redrawn on every console poll, so the noise "lives" like an old
/// tube; a cheap xorshift keeps it allocation-free.
pub(super) fn draw_crt_grain(_a: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: i32, h: i32) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(0x9e3779b9);
    if w <= 0 || h <= 0 {
        return;
    }
    let mut s = SEED.fetch_add(0x6d2b79f5, Ordering::Relaxed) | 1;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    };
    let specks = ((w * h) / 900).clamp(40, 2000) as u32;
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.045);
    for _ in 0..specks {
        let x = (next() % w as u32) as f64;
        let y = (next() % h as u32) as f64;
        cr.rectangle(x, y, 1.0, 1.0);
    }
    let _ = cr.fill();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.09);
    for _ in 0..specks {
        let x = (next() % w as u32) as f64;
        let y = (next() % h as u32) as f64;
        cr.rectangle(x, y, 1.0, 1.0);
    }
    let _ = cr.fill();
    // Vignette: the corners fall off like a tube's face.
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let r = (cx * cx + cy * cy).sqrt();
    let grad = gtk::cairo::RadialGradient::new(cx, cy, r * 0.55, cx, cy, r);
    grad.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
    grad.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.28);
    let _ = cr.set_source(&grad);
    let _ = cr.paint();
}
