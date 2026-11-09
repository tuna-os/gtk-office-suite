//! Drawing the stage: the scene for a frame, the two stills, the carried card
//! and the geometry and easing behind them.

use super::capture::sample;
use super::{After, Before, Card, Cut, Scene, SendMorph, CARD_RADIUS, EDITOR_RADIUS};
use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, graphene, gsk};

/// Draw the whole still, shifted by (dx, dy); callers clip to the part they
/// want. Shifts are snapped to device pixels so text stays sharp in motion.
fn draw_still(
    s: &gtk::Snapshot,
    tex: &gdk::Texture,
    scale: f32,
    full: &graphene::Rect,
    dx: f32,
    dy: f32,
) {
    let snap = |v: f32| (v * scale).round() / scale;
    let r = graphene::Rect::new(
        full.x() + snap(dx),
        full.y() + snap(dy),
        tex.width() as f32 / scale,
        tex.height() as f32 / scale,
    );
    s.append_texture(tex, &r);
}

/// The part of `still` inside `rect`, less `hole`, as a texture of its own;
/// `rect` is widened to whole device pixels so the piece lands on them.
pub(super) fn cut(
    renderer: &gsk::Renderer,
    still: &gdk::Texture,
    scale: f32,
    rect: &graphene::Rect,
    hole: Option<&graphene::Rect>,
) -> Option<Cut> {
    let x0 = (rect.x() * scale).floor();
    let y0 = (rect.y() * scale).floor();
    let x1 = ((rect.x() + rect.width()) * scale)
        .ceil()
        .min(still.width() as f32);
    let y1 = ((rect.y() + rect.height()) * scale)
        .ceil()
        .min(still.height() as f32);
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return None;
    }
    let at = graphene::Rect::new(x0 / scale, y0 / scale, (x1 - x0) / scale, (y1 - y0) / scale);
    let full = graphene::Rect::new(
        0.0,
        0.0,
        still.width() as f32 / scale,
        still.height() as f32 / scale,
    );
    let snap = gtk::Snapshot::new();
    snap.scale(scale, scale);
    match hole {
        None => snap.append_texture(still, &full),
        Some(hole) => {
            for r in around(&at, hole) {
                snap.push_clip(&r);
                snap.append_texture(still, &full);
                snap.pop();
            }
        }
    }
    let node = snap.to_node()?;
    let tex = renderer.render_texture(&node, Some(&graphene::Rect::new(x0, y0, x1 - x0, y1 - y0)));
    Some(Cut { tex, at })
}

/// Draw a piece where it was cut from, shifted by (dx, dy), as it falls:
/// under a scale, device pixels are not where whole numbers put them.
fn draw_piece(s: &gtk::Snapshot, cut: &Cut, dx: f32, dy: f32) {
    let r = graphene::Rect::new(
        cut.at.x() + dx,
        cut.at.y() + dy,
        cut.at.width(),
        cut.at.height(),
    );
    s.append_scaled_texture(&cut.tex, gsk::ScalingFilter::Linear, &r);
}

/// Draw a piece where it was cut from, shifted by (dx, dy), on whole device
/// pixels.
fn draw_cut(s: &gtk::Snapshot, cut: &Cut, scale: f32, dx: f32, dy: f32) {
    let snap = |v: f32| (v * scale).round() / scale;
    let r = graphene::Rect::new(
        cut.at.x() + snap(dx),
        cut.at.y() + snap(dy),
        cut.at.width(),
        cut.at.height(),
    );
    s.append_texture(&cut.tex, &r);
}

fn rounded_rect(r: &graphene::Rect, top: f32, bottom: f32) -> gsk::RoundedRect {
    gsk::RoundedRect::new(
        *r,
        graphene::Size::new(top, top),
        graphene::Size::new(top, top),
        graphene::Size::new(bottom, bottom),
        graphene::Size::new(bottom, bottom),
    )
}

/// The lifted card's shadow, `lift` of the way up.
fn shadow(s: &gtk::Snapshot, card: &gsk::RoundedRect, lift: f32) {
    if lift <= 0.0 {
        return;
    }
    let shade = gdk::RGBA::new(0.0, 0.0, 0.0, 0.16 * lift);
    s.append_outset_shadow(card, &shade, 0.0, 6.0 * lift, 0.0, 22.0 * lift);
}

/// The parts of `outer` not covered by `hole`, as up to four rectangles.
pub(super) fn around(outer: &graphene::Rect, hole: &graphene::Rect) -> Vec<graphene::Rect> {
    let (ox0, oy0) = (outer.x(), outer.y());
    let (ox1, oy1) = (ox0 + outer.width(), oy0 + outer.height());
    let hx0 = hole.x().clamp(ox0, ox1);
    let hy0 = hole.y().clamp(oy0, oy1);
    let hx1 = (hole.x() + hole.width()).clamp(ox0, ox1);
    let hy1 = (hole.y() + hole.height()).clamp(oy0, oy1);
    let mut v = Vec::new();
    let mut push = |x0: f32, y0: f32, x1: f32, y1: f32| {
        if x1 > x0 && y1 > y0 {
            v.push(graphene::Rect::new(x0, y0, x1 - x0, y1 - y0));
        }
    };
    push(ox0, oy0, ox1, hy0);
    push(ox0, hy1, ox1, oy1);
    push(ox0, hy0, hx0, hy1);
    push(hx1, hy0, ox1, hy1);
    v
}

pub(super) fn inflate(r: &graphene::Rect, by: f32) -> graphene::Rect {
    graphene::Rect::new(
        r.x() - by,
        r.y() - by,
        r.width() + 2.0 * by,
        r.height() + 2.0 * by,
    )
}

fn lerp_rect(a: &graphene::Rect, b: &graphene::Rect, e: f32) -> graphene::Rect {
    let l = |x: f32, y: f32| x + (y - x) * e;
    graphene::Rect::new(
        l(a.x(), b.x()),
        l(a.y(), b.y()),
        l(a.width(), b.width()),
        l(a.height(), b.height()),
    )
}

fn mix(a: &gdk::RGBA, b: &gdk::RGBA, e: f32) -> gdk::RGBA {
    let l = |x: f32, y: f32| x + (y - x) * e;
    gdk::RGBA::new(
        l(a.red(), b.red()),
        l(a.green(), b.green()),
        l(a.blue(), b.blue()),
        l(a.alpha(), b.alpha()),
    )
}

fn smoothstep(a: f64, b: f64, t: f64) -> f64 {
    let x = ((t - a) / (b - a)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// The motion's curve: a cubic Bézier that leaves gently and arrives
/// softly, so the card neither jumps off the composer nor thuds into place.
pub(super) fn ease(t: f64) -> f64 {
    cubic_bezier(0.32, 0.0, 0.12, 1.0, t)
}

fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let bez = |a: f64, b: f64, u: f64| {
        let v = 1.0 - u;
        3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
    };
    // Solve x(u) = t by bisection: monotonic for x1, x2 in [0, 1].
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

impl SendMorph {
    /// Work out where everything goes, from the two stills and the card
    /// positions measured in each.
    pub(super) fn scene_for(&self, before: &Before, after: After) -> Option<Scene> {
        let at = after.cards.iter().position(|c| c.key == after.arrival)?;
        let arrival = after.cards[at].clone();
        let view = &after.view;
        let top = arrival.rect.y().max(view.y());
        let bottom = (arrival.rect.y() + arrival.rect.height()).min(view.y() + view.height());
        if bottom - top < 8.0 {
            // The card landed out of sight: nothing to carry the editor to.
            tracing::info!(target: "hylki::morph", "send morph: the reply's card is out of sight");
            return None;
        }
        let card = graphene::Rect::new(arrival.rect.x(), top, arrival.rect.width(), bottom - top);
        let card_radius = (
            if arrival.rect.y() >= view.y() - 0.5 {
                CARD_RADIUS
            } else {
                0.0
            },
            if arrival.rect.y() + arrival.rect.height() <= view.y() + view.height() + 0.5 {
                CARD_RADIUS
            } else {
                0.0
            },
        );
        // The cards on either side of the reply each move as one: the
        // shift of any card on that side that both documents have.
        let shift_of = |cards: &[Card]| -> Option<f32> {
            cards.iter().find_map(|c| {
                before
                    .cards
                    .iter()
                    .find(|o| o.key == c.key)
                    .map(|o| c.rect.y() - o.rect.y())
            })
        };
        let above: Vec<Card> = after.cards[..at].iter().rev().cloned().collect();
        let below: Vec<Card> = after.cards[at + 1..].to_vec();
        let shift_above = shift_of(&above);
        let shift_below = shift_of(&below);
        // A side with no card in both moves with the other side, or failing
        // that with the top of the view.
        let shift_above = shift_above
            .or(shift_below)
            .unwrap_or(view.y() - before.view.y());
        let shift_below = shift_below.unwrap_or(shift_above);
        let scale = self.imp().scale.get();
        let renderer = self.native().and_then(|n| n.renderer())?;
        let sample = |tex: &gdk::Texture, x: f32, y: f32| sample(&renderer, tex, scale, x, y);
        let e = &before.editor;
        let composer_ground = self.imp().base.get()?;
        let ground = before.ground.unwrap_or(composer_ground);
        let page = sample(&after.still, view.x() + 3.0, view.y() + view.height() / 2.0)?;
        let editor_ground = sample(&before.still, e.x() + 6.0, e.y() + e.height() - 6.0)?;
        let card_ground = sample(&after.still, card.x() + 6.0, card.y() + card.height() - 6.0)?;
        let header_cut = after
            .header
            .and_then(|hb| cut(&renderer, &after.still, scale, &hb, None));
        let card_cut = cut(&renderer, &after.still, scale, &card, None);
        // Without a measured line (an empty body), the cards' tops meet.
        let (editor_text, card_text, text_ratio) = match (before.text, arrival.text) {
            (Some((et, es)), Some((ct, cs))) => (
                (et.x() - e.x(), et.y() - e.y()),
                (ct.x() - card.x(), ct.y() - card.y()),
                if es > 0.0 && cs > 0.0 {
                    (cs / es).clamp(0.5, 2.0)
                } else {
                    1.0
                },
            ),
            _ => ((0.0, 0.0), (0.0, arrival.head), 1.0),
        };
        Some(Scene {
            editor_text,
            card_text,
            text_ratio,
            card,
            card_radius,
            shift_above,
            shift_below,
            composer_ground,
            ground,
            page,
            editor_ground,
            card_ground,
            header_cut,
            card_cut,
            after,
        })
    }

    pub(super) fn draw(&self, s: &gtk::Snapshot) {
        let imp = self.imp();
        // At rest before the clock starts, the scene is drawn a hair past
        // its start, so every way the flight draws is already in use.
        let resting = imp.scene.borrow().is_some() && imp.motion_start.get() == 0;
        let t = if resting {
            0.001
        } else {
            self.motion_progress()
        };
        self.draw_at(s, t, self.chrome_progress());
    }

    /// Draw the stage `t` of the way through the motion, the chrome fade
    /// `c` of the way through its own.
    fn draw_at(&self, s: &gtk::Snapshot, t: f64, c: f64) {
        let imp = self.imp();
        let before = imp.before.borrow();
        let Some(before) = before.as_ref() else {
            return;
        };
        let scale = imp.scale.get();
        let (w, h) = (self.width() as f32, self.height() as f32);
        let full = graphene::Rect::new(0.0, 0.0, w, h);
        let chrome = ease_out(c) as f32;
        let scene = imp.scene.borrow();
        let Some(scene) = scene.as_ref() else {
            self.draw_waiting(s, before, scale, &full, chrome);
            return;
        };
        let after = &scene.after;
        s.append_color(&scene.ground, &full);
        if t >= 1.0 {
            // The last frame is the pane itself.
            draw_still(s, &after.still, scale, &full, 0.0, 0.0);
            return;
        }
        let e = ease(t) as f32;
        let p = Paint {
            s,
            scale,
            full,
            e,
            t,
        };

        // Where the composer stood, its ground giving way to the window's.
        {
            let mut c = scene.composer_ground;
            c.set_alpha(1.0 - e);
            s.append_color(&c, &before.composer);
        }

        let view = lerp_rect(&before.view, &after.view, e);
        draw_reader_around(&p, before, scene, &view);

        // The morphing card's frame, which the old conversation keeps clear of.
        let frame = lerp_rect(&before.editor, &scene.card, e);
        draw_conversation(&p, before, scene, &view, &frame);

        // The composer's buttons and fields, fading where they stood.
        if let (true, Some(c)) = (chrome < 1.0, imp.chrome_cut.borrow().as_ref()) {
            s.push_opacity((1.0 - chrome) as f64);
            draw_cut(s, c, scale, 0.0, 0.0);
            s.pop();
        }

        self.draw_card(&p, before, scene, &frame, chrome);
    }

    /// Waiting for the conversation: the pane as it was, the composer's
    /// buttons and fields already on their way out, and the editor card
    /// lifting off the composer.
    fn draw_waiting(
        &self,
        s: &gtk::Snapshot,
        before: &Before,
        scale: f32,
        full: &graphene::Rect,
        chrome: f32,
    ) {
        let imp = self.imp();
        let editor = &before.editor;
        let cutout = inflate(editor, 4.0);
        if let Some(ground) = before.ground {
            s.append_color(&ground, full);
        }
        draw_still(s, &before.still, scale, full, 0.0, 0.0);
        if let (true, Some(base)) = (chrome > 0.0, imp.base.get()) {
            let mut c = base;
            c.set_alpha(chrome);
            for r in around(&before.composer, &cutout) {
                s.append_color(&c, &r);
            }
        }
        // The editor card lifts off the composer while it waits.
        let rounded = rounded_rect(editor, EDITOR_RADIUS, EDITOR_RADIUS);
        shadow(s, &rounded, chrome);
        if let Some(c) = imp.editor_cut.borrow().as_ref() {
            s.push_rounded_clip(&rounded);
            draw_cut(s, c, scale, 0.0, 0.0);
            s.pop();
        }
    }

    /// The editor card becoming the message card. It lifts off the
    /// composer, its shadow growing and settling again as it lands, and
    /// its frame takes the message card's place and size.
    fn draw_card(
        &self,
        p: &Paint,
        before: &Before,
        scene: &Scene,
        frame: &graphene::Rect,
        chrome: f32,
    ) {
        let imp = self.imp();
        let (s, e, t) = (p.s, p.e, p.t);
        let lerp = |a: f32, b: f32| a + (b - a) * e;
        let editor = &before.editor;
        let card = &scene.card;
        let (rt, rb) = scene.card_radius;
        let top_r = lerp(EDITOR_RADIUS, rt);
        let bottom_r = lerp(EDITOR_RADIUS, rb);
        let rounded = rounded_rect(frame, top_r, bottom_r);
        // Lifted while waiting, it stays up for the first half of the
        // flight and settles as it lands.
        shadow(
            s,
            &rounded,
            chrome * (1.0 - smoothstep(0.45, 1.0, t) as f32),
        );
        s.push_rounded_clip(&rounded);
        s.append_color(&mix(&scene.editor_ground, &scene.card_ground, e), frame);
        // The same words lie on top of each other, so the card's own
        // picture comes up over the editor's as the editor's goes: the text
        // never thins out on the way. Brief, because the two set the words
        // in slightly different sizes, and the editor's quoted original has
        // no place in the card.
        let out = 1.0 - smoothstep(0.28, 0.42, t);
        let inn = smoothstep(0.24, 0.4, t);
        // Inside it the editor's picture gives way to the card's, both
        // placed so their first lines of text meet, and scaled about that
        // line from the editor's size of type to the card's, so the letters
        // stay on each other too; each is at its own size at its own end of
        // the flight. Each is cut to its own card's shape: the pieces carry
        // a sliver of what was round their corners, which inside the moving
        // frame would show.
        let (ex, ey) = scene.editor_text;
        let (cx, cy) = scene.card_text;
        let (tx, ty) = (frame.x() + lerp(ex, cx), frame.y() + lerp(ey, cy));
        let size = lerp(1.0, scene.text_ratio);
        if let (true, Some(c)) = (out > 0.0, imp.editor_cut.borrow().as_ref()) {
            s.push_opacity(out);
            s.save();
            s.translate(&graphene::Point::new(tx, ty));
            s.scale(size, size);
            let at = graphene::Rect::new(-ex, -ey, editor.width(), editor.height());
            s.push_rounded_clip(&rounded_rect(&at, EDITOR_RADIUS, EDITOR_RADIUS));
            draw_piece(s, c, at.x() - editor.x(), at.y() - editor.y());
            s.pop();
            s.restore();
            s.pop();
        }
        if let (true, Some(c)) = (inn > 0.0, scene.card_cut.as_ref()) {
            let k = size / scene.text_ratio;
            s.push_opacity(inn);
            s.save();
            s.translate(&graphene::Point::new(tx, ty));
            s.scale(k, k);
            let at = graphene::Rect::new(-cx, -cy, card.width(), card.height());
            s.push_rounded_clip(&rounded_rect(&at, rt, rb));
            draw_piece(s, c, at.x() - card.x(), at.y() - card.y());
            s.pop();
            s.restore();
            s.pop();
        }
        s.pop();
    }
}

/// What every part of one frame of the flight is drawn with: the snapshot,
/// the device scale, the stage's box, the eased progress `e` and the raw
/// progress `t`.
struct Paint<'a> {
    s: &'a gtk::Snapshot,
    scale: f32,
    full: graphene::Rect,
    e: f32,
    t: f64,
}

/// The reader around the conversation: its header bar (back in from the
/// top edge if the split had hidden it, as the split's own close brings it
/// back), the subject and everything else above the conversation rising
/// (or sinking) with its top edge, and what sits under it (the attachment
/// drawer).
fn draw_reader_around(p: &Paint, before: &Before, scene: &Scene, view: &graphene::Rect) {
    let (s, scale, full, e, t) = (p.s, p.scale, &p.full, p.e, p.t);
    let after = &scene.after;
    let head_top = match (before.header, after.header) {
        (None, Some(hb)) => {
            if let Some(c) = scene.header_cut.as_ref() {
                let dy = -(1.0 - e) * (hb.y() + hb.height());
                s.push_opacity(smoothstep(0.0, 0.6, t));
                draw_cut(s, c, scale, 0.0, dy);
                s.pop();
            }
            hb.y() + hb.height()
        }
        (_, Some(hb)) => {
            s.push_clip(&hb);
            draw_still(s, &after.still, scale, full, 0.0, 0.0);
            s.pop();
            hb.y() + hb.height()
        }
        (_, None) => after.reader.y(),
    };
    {
        let dy = (1.0 - e) * (before.view.y() - after.view.y());
        let top = head_top + dy;
        let clip = graphene::Rect::new(
            after.reader.x(),
            top,
            after.reader.width(),
            (view.y() - top).max(0.0),
        );
        s.push_clip(&clip);
        draw_still(s, &after.still, scale, full, 0.0, dy);
        s.pop();
    }
    {
        let bottom_after = after.view.y() + after.view.height();
        let dy = (1.0 - e) * ((before.view.y() + before.view.height()) - bottom_after);
        let top = view.y() + view.height();
        let bottom = after.reader.y() + after.reader.height() + dy;
        let clip = graphene::Rect::new(
            after.reader.x(),
            top,
            after.reader.width(),
            (bottom - top).max(0.0),
        );
        s.push_clip(&clip);
        draw_still(s, &after.still, scale, full, 0.0, dy);
        s.pop();
    }
}

/// The conversation, sliding to its new place: what the reader showed
/// before, with the finished document over it wherever the two meet, so
/// whatever comes into view on the way is already the new one. The old
/// conversation keeps clear of the morphing card's `frame`.
fn draw_conversation(
    p: &Paint,
    before: &Before,
    scene: &Scene,
    view: &graphene::Rect,
    frame: &graphene::Rect,
) {
    let (s, e) = (p.s, p.e);
    let after = &scene.after;
    let frame_bottom = frame.y() + frame.height();
    s.push_clip(view);
    s.append_color(&scene.page, view);
    let card = &scene.card;
    let (sa, sb) = (scene.shift_above, scene.shift_below);
    let old_split_above = card.y() - sa;
    let old_split_below = card.y() + card.height() - sb;
    {
        let dy = e * sa;
        let top = before.view.y() + dy;
        let bottom = (old_split_above + dy).min(frame.y());
        draw_band(p, &before.still, view, top, bottom, dy);
    }
    {
        let dy = e * sb;
        let top = (old_split_below + dy).max(frame_bottom);
        let bottom = before.view.y() + before.view.height() + dy;
        draw_band(p, &before.still, view, top, bottom, dy);
    }
    {
        let dy = (e - 1.0) * sa;
        let top = after.view.y() + dy;
        let bottom = card.y() + dy;
        draw_band(p, &after.still, view, top, bottom, dy);
    }
    {
        let dy = (e - 1.0) * sb;
        let top = card.y() + card.height() + dy;
        let bottom = after.view.y() + after.view.height() + dy;
        draw_band(p, &after.still, view, top, bottom, dy);
    }
    s.pop();
}

/// The band of `view` from `top` to `bottom` (nothing when it is empty),
/// filled with `still` shifted down by `dy`.
fn draw_band(
    p: &Paint,
    still: &gdk::Texture,
    view: &graphene::Rect,
    top: f32,
    bottom: f32,
    dy: f32,
) {
    if bottom > top {
        p.s.push_clip(&graphene::Rect::new(
            view.x(),
            top,
            view.width(),
            bottom - top,
        ));
        draw_still(p.s, still, p.scale, &p.full, 0.0, dy);
        p.s.pop();
    }
}
