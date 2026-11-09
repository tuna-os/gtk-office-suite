//! The order of a send: the still before, the reply's card measured in the
//! conversation, then the landing.

use super::capture::{capture, ground_behind};
use super::measure::{parse_text, Measured, CARDS_JS, EDITOR_TEXT_JS};
use super::{After, Before, SendMorph, READY_TIMEOUT_MS, SETTLE_MS};
use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};

/// What the send morph is made from: the pane (`root`, which the stage
/// covers), the composer's slot in it and the editor card inside that, and
/// the reader beside it with its web view.
pub(crate) struct Pieces {
    pub(crate) root: gtk::Widget,
    pub(crate) composer: gtk::Widget,
    pub(crate) editor: gtk::Widget,
    pub(crate) reader: gtk::Widget,
    pub(crate) view: webkit6::WebView,
    /// The reader's header bar, and whether it is showing now.
    pub(crate) header: Option<gtk::Widget>,
    pub(crate) header_shown: bool,
    /// The editor's web view, whose first line of text the card's meets.
    pub(crate) editor_view: Option<webkit6::WebView>,
}

/// Take the still the morph starts from and measure where the conversation's
/// cards stand, then answer `ready`: true once the pane may change under the
/// stage, false when the pane could not be captured.
pub(crate) fn start(stage: &SendMorph, p: Pieces, ready: impl FnOnce(bool) + 'static) {
    let stage = stage.clone();
    capture_next_frame(&p.root.clone(), move |still| {
        let bounds = |w: &gtk::Widget| w.compute_bounds(&p.root);
        let rects = (
            bounds(&p.composer),
            bounds(&p.editor),
            bounds(&p.reader),
            bounds(p.view.upcast_ref()),
        );
        let (Some((still, scale)), (Some(composer), Some(editor), Some(reader), Some(view))) =
            (still, rects)
        else {
            tracing::info!(target: "hylki::morph", "send morph: the pane could not be captured");
            ready(false);
            return;
        };
        if editor.height() < 8.0 || view.height() < 8.0 {
            tracing::info!(target: "hylki::morph", "send morph: the editor or the reader has no height");
            ready(false);
            return;
        }
        let header = p
            .header
            .as_ref()
            .filter(|_| p.header_shown)
            .and_then(bounds);
        // Where the reader leaves its background to the window, the window's
        // color is what shows.
        let ground = ground_behind(
            &p.root,
            &still,
            scale,
            &[
                (reader.x() + 2.0, reader.y() + 2.0),
                (reader.x() + 2.0, view.y() - 2.0),
                (
                    reader.x() + reader.width() - 2.0,
                    reader.y() + reader.height() - 2.0,
                ),
            ],
        );
        let before = Before {
            still,
            composer,
            editor,
            view,
            header,
            ground,
            cards: Vec::new(),
            text: None,
        };
        stage.begin(before, scale);
        // Two questions for the web views; the pane may change once both
        // are answered.
        let ready = std::rc::Rc::new(RefCell::new(Some(ready)));
        let left = std::rc::Rc::new(Cell::new(2u8));
        let answered = move || {
            left.set(left.get() - 1);
            if left.get() == 0 {
                if let Some(ready) = ready.borrow_mut().take() {
                    ready(true);
                }
            }
        };
        let answered = std::rc::Rc::new(answered);
        {
            let (stage, answered) = (stage.clone(), answered.clone());
            webkit6::prelude::WebViewExt::evaluate_javascript(
                &p.view,
                CARDS_JS,
                None,
                None,
                None::<&gtk::gio::Cancellable>,
                move |r| {
                    let cards = r
                        .ok()
                        .and_then(|v| Measured::parse(&v.to_str(), &view))
                        .map(|m| m.cards())
                        .unwrap_or_default();
                    stage.set_before_cards(cards);
                    answered();
                },
            );
        }
        match p
            .editor_view
            .as_ref()
            .and_then(|ev| Some((ev, ev.compute_bounds(&p.root)?)))
        {
            Some((ev, at)) => {
                webkit6::prelude::WebViewExt::evaluate_javascript(
                    ev,
                    EDITOR_TEXT_JS,
                    None,
                    None,
                    None::<&gtk::gio::Cancellable>,
                    move |r| {
                        let text = r.ok().and_then(|v| parse_text(&v.to_str(), &at));
                        if let Some(b) = stage.imp().before.borrow_mut().as_mut() {
                            b.text = text;
                        }
                        answered();
                    },
                );
            }
            None => answered(),
        }
    });
}

/// A still of `widget` taken just after the next frame is drawn. GTK keeps
/// each widget's last drawing and hands it to [`gtk::WidgetPaintable`], but
/// anything waiting to be redrawn (a blinking caret is enough) has none until
/// the frame is drawn; right after it, every part of the pane has one.
pub(crate) fn capture_next_frame(
    widget: &gtk::Widget,
    then: impl FnOnce(Option<(gdk::Texture, f32)>) + 'static,
) {
    type Then = Box<dyn FnOnce(Option<(gdk::Texture, f32)>)>;
    let then: std::rc::Rc<RefCell<Option<Then>>> =
        std::rc::Rc::new(RefCell::new(Some(Box::new(then))));
    let Some(clock) = widget.frame_clock() else {
        if let Some(f) = then.take() {
            f(capture(widget));
        }
        return;
    };
    let handler = std::rc::Rc::new(Cell::new(None));
    let tries = Cell::new(0);
    let id = {
        let (w, then, handler) = (widget.clone(), then.clone(), handler.clone());
        clock.connect_after_paint(move |clock| {
            let still = capture(&w);
            tries.set(tries.get() + 1);
            if still.is_none() && tries.get() < 3 {
                w.queue_draw();
                return;
            }
            if let Some(id) = handler.take() {
                clock.disconnect(id);
            }
            if let Some(f) = then.take() {
                f(still);
            }
        })
    };
    handler.set(Some(id));
    widget.queue_draw();
    // A window that stops drawing stops its clock: no still, then.
    glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
        if let Some(id) = handler.take() {
            clock.disconnect(id);
        }
        if let Some(f) = then.take() {
            f(None);
        }
    });
}

/// Wait for the reply's card to be laid out in the reader, then land the
/// stage on it. The document is asked where its cards are every frame or
/// so; the card counts as settled once it is there, every message frame is
/// sized, and two answers in a row agree. A wait past
/// [`READY_TIMEOUT_MS`] fades the stage instead.
pub(crate) fn await_arrival(
    stage: &SendMorph,
    root: &gtk::Widget,
    reader: &gtk::Widget,
    view: &webkit6::WebView,
    header: Option<gtk::Widget>,
    key: String,
) {
    let poll = std::rc::Rc::new(Poll {
        stage: stage.clone(),
        header,
        root: root.clone(),
        reader: reader.clone(),
        view: view.clone(),
        key,
        started: std::time::Instant::now(),
        last: RefCell::new(None),
        same: Cell::new(0),
    });
    poll_once(poll);
}

struct Poll {
    stage: SendMorph,
    header: Option<gtk::Widget>,
    root: gtk::Widget,
    reader: gtk::Widget,
    view: webkit6::WebView,
    key: String,
    started: std::time::Instant,
    last: RefCell<Option<Measured>>,
    same: Cell<u32>,
}

fn poll_once(poll: std::rc::Rc<Poll>) {
    use webkit6::prelude::WebViewExt;
    if !poll.stage.running() {
        return;
    }
    if poll.started.elapsed() > std::time::Duration::from_millis(READY_TIMEOUT_MS) {
        tracing::info!(target: "hylki::morph", "send morph: the reply's card never settled ({:?})", poll.last.borrow().as_ref().map(|m| (m.live, m.cards.len())));
        poll.stage.abort();
        return;
    }
    let p = poll.clone();
    poll.view.evaluate_javascript(
        CARDS_JS,
        None,
        None,
        None::<&gtk::gio::Cancellable>,
        move |r| {
            let view_rect = p.view.compute_bounds(&p.root);
            let measured = r
                .ok()
                .zip(view_rect)
                .and_then(|(v, rect)| Measured::parse(&v.to_str(), &rect));
            let ready = measured
                .as_ref()
                .is_some_and(|m| m.live && m.cards.iter().any(|(k, _, _)| *k == p.key));
            let same = ready && *p.last.borrow() == measured;
            p.same.set(if same { p.same.get() + 1 } else { 0 });
            p.last.replace(measured);
            // A document still moving after a while (pictures coming in, say)
            // is landed on as it stands: the card is there, and waiting on would
            // only keep the reply off the screen.
            let long = p.started.elapsed() > std::time::Duration::from_millis(SETTLE_MS);
            if p.same.get() >= 1 || (ready && long) {
                // Settled: the still is taken after the next frame, by when
                // GTK holds the web view's picture of it.
                land_on(&p);
                return;
            }
            let p2 = p.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(16), move || {
                poll_once(p2)
            });
        },
    );
}

fn land_on(p: &Poll) {
    let (Some(m), Some(reader), Some(view)) = (
        p.last.borrow().clone(),
        p.reader.compute_bounds(&p.root),
        p.view.compute_bounds(&p.root),
    ) else {
        p.stage.abort();
        return;
    };
    let (stage, key, started) = (p.stage.clone(), p.key.clone(), p.started);
    let header = p
        .header
        .as_ref()
        .and_then(|h| h.compute_bounds(&p.root))
        .filter(|r| r.height() > 0.0);
    capture_next_frame(&p.root, move |still| {
        let Some((still, _)) = still else {
            stage.abort();
            return;
        };
        tracing::info!(target: "hylki::morph", "send morph: landing after {:?}", started.elapsed());
        stage.land(After {
            still,
            reader,
            view,
            header,
            cards: m.cards(),
            arrival: key,
        });
    });
}

/// The first widget under `root` (itself included) that `pick` accepts.
pub(crate) fn find_widget(
    root: &gtk::Widget,
    pick: &dyn Fn(&gtk::Widget) -> bool,
) -> Option<gtk::Widget> {
    if pick(root) {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        if let Some(found) = find_widget(&c, pick) {
            return Some(found);
        }
        child = c.next_sibling();
    }
    None
}
