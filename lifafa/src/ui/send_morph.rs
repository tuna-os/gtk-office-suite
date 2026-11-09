//! The split reply turning into its own card in the conversation when it is
//! sent.
//!
//! A stage covers the reading pane and draws two stills of it: the pane as
//! it was when Send was pressed, and the pane once the composer has gone and
//! the reply's card is in the thread. The editor card is carried to where
//! the message card now sits and becomes it, the conversation slides to its
//! new place around it, and the composer's buttons fade. Everything that
//! moves is a texture on one frame clock: nothing is laid out mid-flight,
//! the web view and GTK cannot drift apart, and the last frame is the
//! pane's own pixels, so lifting the stage changes nothing on screen.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene};

mod capture;
mod draw;
mod flow;
mod measure;
#[cfg(test)]
mod tests;

use capture::{record_final, record_frame, sample, slow_motion};
use draw::{cut, inflate};
pub(crate) use flow::{await_arrival, capture_next_frame, find_widget, start, Pieces};

/// How long the editor card takes to reach its place in the conversation.
const MOTION_MS: f64 = 480.0;
/// How long the composer's buttons and fields take to fade, from Send.
const CHROME_MS: f64 = 200.0;
/// How long to wait for the conversation to be ready before giving up and
/// fading the stage instead.
const READY_TIMEOUT_MS: u64 = 1500;
/// How long the reply's card may keep moving in the document before the
/// stage lands on it as it stands.
const SETTLE_MS: u64 = 450;
/// The editor card's corners (libadwaita's `.card`) and the conversation
/// card's (`.hylki-msg`): the same 12px, kept apart in case either moves.
const EDITOR_RADIUS: f32 = 12.0;
const CARD_RADIUS: f32 = 12.0;

/// One conversation card as the reader document lays it out, in stage
/// coordinates.
#[derive(Clone, Debug)]
struct Card {
    key: String,
    rect: graphene::Rect,
    /// The height of its header line, where the sender and date sit.
    head: f32,
    /// Where its first line of text begins, and the size it is set in.
    text: Option<(graphene::Point, f32)>,
}

/// The pane at the moment Send was pressed.
pub(crate) struct Before {
    still: gdk::Texture,
    /// The split slot the composer fills.
    composer: graphene::Rect,
    /// The editor card inside it.
    editor: graphene::Rect,
    /// The reader's web view, beside the composer. (The reader's own box is
    /// only measured, for where the window's ground shows.)
    view: graphene::Rect,

    /// The reader's header bar, when it was showing (a split above the
    /// reader hides it).
    header: Option<graphene::Rect>,
    /// What the window paints behind the pane, where the stills are
    /// see-through: the pane's own widgets leave their background to it.
    ground: Option<gdk::RGBA>,
    /// The conversation's cards as the document had them, once measured.
    cards: Vec<Card>,
    /// Where the first line written in the editor begins, and its size.
    text: Option<(graphene::Point, f32)>,
}

/// The pane with the composer gone and the reply in the conversation.
pub(crate) struct After {
    still: gdk::Texture,
    reader: graphene::Rect,
    view: graphene::Rect,
    header: Option<graphene::Rect>,
    /// Every card, in document order.
    cards: Vec<Card>,
    /// The reply's own card among them.
    arrival: String,
}

/// Everything the frames are drawn from, worked out once when the motion
/// starts.
struct Scene {
    after: After,
    /// The reply's card, cut to what the web view shows of it.
    card: graphene::Rect,
    /// Its corner radii: none on an edge the web view cuts off.
    card_radius: (f32, f32),
    /// Where the first line of text starts in the editor card and in the
    /// message card, from each card's corner. The two are carried to meet,
    /// so the words written stay put while the card changes around them.
    editor_text: (f32, f32),
    card_text: (f32, f32),
    /// How much larger the card sets the words than the editor did.
    text_ratio: f32,
    /// How far the cards above and below the reply move.
    shift_above: f32,
    shift_below: f32,
    /// The composer's own ground, and the window's behind the pane.
    composer_ground: gdk::RGBA,
    ground: gdk::RGBA,
    page: gdk::RGBA,
    editor_ground: gdk::RGBA,
    card_ground: gdk::RGBA,
    /// The pieces that fade, cut out of the after still.
    header_cut: Option<Cut>,
    card_cut: Option<Cut>,
}

/// A piece of a still in a texture of its own, and where it sits. Faded on
/// its own, a piece costs the GPU only its own size; faded as a clip of the
/// whole still, it costs the whole pane, every frame.
struct Cut {
    tex: gdk::Texture,
    at: graphene::Rect,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub(crate) struct SendMorph {
        pub(super) before: RefCell<Option<Before>>,
        pub(super) scene: RefCell<Option<Scene>>,
        /// The texture scale: device pixels per logical pixel.
        pub(super) scale: Cell<f32>,
        /// Frame times (µs) the chrome fade and the motion started at; 0
        /// until the first frame after they were asked for.
        pub(super) chrome_start: Cell<i64>,
        pub(super) motion_start: Cell<i64>,
        /// Frames the landed scene has been drawn at rest, before the clock
        /// starts.
        pub(super) rest_frames: Cell<u32>,
        pub(super) now: Cell<i64>,
        pub(super) tick: RefCell<Option<gtk::TickCallbackId>>,
        /// The stage's size when it went up: a resize mid-flight ends it.
        pub(super) size: Cell<(i32, i32)>,
        pub(super) fading: RefCell<Option<adw::TimedAnimation>>,
        /// The composer's ground, which the stage shows where it stood.
        pub(super) base: Cell<Option<gdk::RGBA>>,
        /// The composer without its editor card, and the editor card.
        pub(super) chrome_cut: RefCell<Option<Cut>>,
        pub(super) editor_cut: RefCell<Option<Cut>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SendMorph {
        const NAME: &'static str = "HylkiSendMorph";
        type Type = super::SendMorph;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SendMorph {
        fn dispose(&self) {
            if let Some(id) = self.tick.take() {
                id.remove();
            }
        }
    }

    impl WidgetImpl for SendMorph {
        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let (w, h) = self.size.get();
            if self.before.borrow().is_some() && (w, h) != (width, height) && w > 0 {
                // The stills no longer fit the pane: let it show itself.
                let obj = self.obj().clone();
                glib::idle_add_local_once(move || obj.finish());
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub(crate) struct SendMorph(ObjectSubclass<imp::SendMorph>)
        @extends gtk::Widget;
}

impl Default for SendMorph {
    fn default() -> Self {
        Self::new()
    }
}

impl SendMorph {
    pub(crate) fn new() -> Self {
        let obj: Self = glib::Object::new();
        obj.set_visible(false);
        obj.set_can_target(false);
        obj.set_can_focus(false);
        obj
    }

    /// Whether a morph is on screen.
    pub(crate) fn running(&self) -> bool {
        self.imp().before.borrow().is_some()
    }

    /// Take the pane as it looks now, the still the morph starts from. The
    /// stage goes up over it with [`Self::show`].
    pub(crate) fn begin(&self, before: Before, scale: f32) {
        let imp = self.imp();
        if let Some(a) = imp.fading.take() {
            a.skip();
        }
        imp.scale.set(scale);
        // The stage is still hidden, so the pane's size is the one it will
        // be given.
        imp.size
            .set(self.pane().map_or((0, 0), |p| (p.width(), p.height())));
        let e = &before.editor;
        imp.base
            .set(self.native().and_then(|n| n.renderer()).and_then(|r| {
                sample(
                    &r,
                    &before.still,
                    scale,
                    (before.composer.x() + 4.0).max(e.x() - 6.0),
                    e.y() + e.height() / 2.0,
                )
            }));
        if let Some(r) = self.native().and_then(|n| n.renderer()) {
            imp.chrome_cut.replace(cut(
                &r,
                &before.still,
                scale,
                &before.composer,
                Some(&inflate(e, 4.0)),
            ));
            imp.editor_cut
                .replace(cut(&r, &before.still, scale, e, None));
        }
        imp.before.replace(Some(before));
        imp.scene.replace(None);
        imp.chrome_start.set(0);
        imp.motion_start.set(0);
        imp.now.set(0);
    }

    /// Put the stage up, over a pane about to change underneath it. Until
    /// now the real composer has stayed on screen: the stills are complete
    /// only once the web views have answered.
    pub(crate) fn show(&self) {
        if !self.running() {
            return;
        }
        self.set_opacity(1.0);
        self.set_visible(true);
        // Clicks during the motion would land on a pane that is not where
        // it looks to be.
        self.set_can_target(true);
        self.ensure_ticking();
        self.queue_draw();
    }

    /// Where the conversation's cards were when Send was pressed.
    fn set_before_cards(&self, cards: Vec<Card>) {
        if let Some(b) = self.imp().before.borrow_mut().as_mut() {
            b.cards = cards;
        }
    }

    /// The pane is ready in its new state: move everything there.
    pub(crate) fn land(&self, after: After) {
        let imp = self.imp();
        let scene = match imp.before.borrow().as_ref() {
            Some(before) => self.scene_for(before, after),
            None => return,
        };
        let Some(scene) = scene else {
            tracing::info!(target: "hylki::morph", "send morph: the stills could not be laid out");
            self.abort();
            return;
        };
        imp.scene.replace(Some(scene));
        imp.rest_frames.set(0);
        imp.motion_start.set(0);
        // The stage hides the pane completely from here to the last frame:
        // GTK need not draw it under the stage meanwhile. Its web view has
        // painted what the after still shows; nothing waits on it now.
        self.set_pane_drawn(false);
        self.ensure_ticking();
        self.queue_draw();
    }

    /// The pane under the stage: the overlay's main child.
    fn pane(&self) -> Option<gtk::Widget> {
        self.parent()
            .and_downcast::<gtk::Overlay>()
            .and_then(|o| o.child())
    }

    fn set_pane_drawn(&self, drawn: bool) {
        if let Some(pane) = self.pane() {
            pane.set_opacity(if drawn { 1.0 } else { 0.0 });
        }
    }

    /// Give up on the motion: the pane underneath is already in its new
    /// state, so the stage fades away over it.
    pub(crate) fn abort(&self) {
        let imp = self.imp();
        if imp.before.borrow().is_none() || imp.fading.borrow().is_some() {
            return;
        }
        if let Some(id) = imp.tick.take() {
            id.remove();
        }
        self.set_can_target(false);
        self.set_pane_drawn(true);
        let target = adw::CallbackAnimationTarget::new({
            let obj = self.clone();
            move |v| obj.set_opacity(v)
        });
        let anim = adw::TimedAnimation::new(self, self.opacity(), 0.0, 160, target);
        anim.set_easing(adw::Easing::EaseOutCubic);
        let obj = self.clone();
        anim.connect_done(move |_| {
            obj.imp().fading.take();
            obj.finish();
        });
        imp.fading.replace(Some(anim.clone()));
        anim.play();
    }

    /// Lift the stage and let go of the stills.
    pub(crate) fn finish(&self) {
        let imp = self.imp();
        if let Some(id) = imp.tick.take() {
            id.remove();
        }
        if let Some(a) = imp.fading.take() {
            a.pause();
        }
        if imp.before.take().is_some() {
            record_final(self);
        }
        imp.chrome_cut.replace(None);
        imp.editor_cut.replace(None);
        imp.scene.replace(None);
        self.set_pane_drawn(true);
        self.set_visible(false);
        self.set_can_target(false);
        self.set_opacity(1.0);
    }

    fn ensure_ticking(&self) {
        let imp = self.imp();
        if imp.tick.borrow().is_some() {
            return;
        }
        let id = self.add_tick_callback(|obj, clock| {
            let imp = obj.imp();
            let now = clock.frame_time();
            let interval = now - imp.now.get();
            imp.now.set(now);
            if imp.chrome_start.get() == 0 {
                imp.chrome_start.set(now);
            }
            if imp.scene.borrow().is_some() && imp.motion_start.get() == 0 {
                // The first frames of the scene can be slow to draw (its
                // stills going up to the GPU, its shaders being built). They
                // are drawn at rest, and the card sets off once a frame has
                // come in on time: a slow frame mid-flight would be a jump.
                let rest = imp.rest_frames.get();
                if (rest >= 1 && interval < 22_000) || rest >= 5 {
                    imp.motion_start.set(now);
                } else {
                    imp.rest_frames.set(rest + 1);
                }
            }
            obj.queue_draw();
            record_frame(obj);
            if imp.scene.borrow().is_some() && obj.motion_progress() >= 1.0 {
                // The frame just queued is the last: the pane underneath
                // looks the same, so the stage goes after it is drawn.
                imp.tick.take();
                let obj = obj.clone();
                glib::idle_add_local_once(move || obj.finish());
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
        imp.tick.replace(Some(id));
    }

    fn elapsed_ms(&self, start: i64) -> f64 {
        if start == 0 {
            return 0.0;
        }
        (self.imp().now.get() - start).max(0) as f64 / 1000.0 / slow_motion()
    }

    fn motion_progress(&self) -> f64 {
        (self.elapsed_ms(self.imp().motion_start.get()) / MOTION_MS).clamp(0.0, 1.0)
    }

    fn chrome_progress(&self) -> f64 {
        (self.elapsed_ms(self.imp().chrome_start.get()) / CHROME_MS).clamp(0.0, 1.0)
    }
}
