//! Selecting pills across To, Cc and Bcc, and moving the selection between
//! fields.

use super::RecipientRow;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::gdk;

pub(super) fn is_selected(pill: &gtk::Widget) -> bool {
    pill.has_css_class("selected")
}

pub(super) fn set_selected(pill: &gtk::Widget, on: bool) {
    if on {
        pill.add_css_class("selected");
    } else {
        pill.remove_css_class("selected");
    }
    pill.update_state(&[gtk::accessible::State::Selected(Some(on))]);
}

/// The pills an action on `pill` takes: every selected pill of every
/// field, in order, when `pill` is one of them, or else `pill` alone.
pub(super) fn acting_on(pill: &gtk::Widget) -> Vec<(RecipientRow, gtk::Widget)> {
    let Some(row) = pill
        .ancestor(RecipientRow::static_type())
        .and_downcast::<RecipientRow>()
    else {
        return Vec::new();
    };
    if !is_selected(pill) {
        return vec![(row, pill.clone())];
    }
    let mut out = Vec::new();
    for field in row.fields() {
        for p in field.pill_list().into_iter().filter(is_selected) {
            out.push((field.clone(), p));
        }
    }
    out
}

/// The drag icon: the pill dragged, drawn as a stack when others go with
/// it.
pub(super) fn drag_icon(pill: &gdk::Paintable, count: usize) -> gdk::Paintable {
    if count < 2 {
        return pill.clone();
    }
    let (w, h) = (
        pill.intrinsic_width() as f32,
        pill.intrinsic_height() as f32,
    );
    let snapshot = gtk::Snapshot::new();
    // The others as a pill's edge behind it.
    snapshot.save();
    snapshot.translate(&gtk::graphene::Point::new(4.0, 4.0));
    snapshot.push_opacity(0.5);
    pill.snapshot(&snapshot, w as f64, h as f64);
    snapshot.pop();
    snapshot.restore();
    pill.snapshot(&snapshot, w as f64, h as f64);
    snapshot
        .to_paintable(Some(&gtk::graphene::Size::new(w + 4.0, h + 4.0)))
        .unwrap_or_else(|| pill.clone())
}

/// Whether `widget` is a pill or inside one.
pub(super) fn is_in_pill(widget: &gtk::Widget) -> bool {
    let mut w = Some(widget.clone());
    while let Some(c) = w {
        if c.has_css_class("recipient-pill") {
            return true;
        }
        w = c.parent();
    }
    false
}

impl RecipientRow {
    pub(super) fn pill_list(&self) -> Vec<gtk::Widget> {
        self.imp()
            .pills
            .borrow()
            .iter()
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// The pills of every field shown, in order.
    pub(super) fn all_pills(&self) -> Vec<gtk::Widget> {
        self.fields()
            .iter()
            .filter(|f| f.is_mapped())
            .flat_map(|f| f.pill_list())
            .collect()
    }

    /// Where a sweep at `(x, y)` in this field is, as a place among
    /// `all_pills`: above a field is before its pills, and below the last
    /// is after all of them.
    pub(super) fn sweep_index(&self, x: f64, y: f64) -> usize {
        let fields: Vec<_> = self
            .fields()
            .into_iter()
            .filter(|f| f.is_mapped())
            .collect();
        let mut before = 0;
        for field in &fields {
            let Some(p) = self.compute_point(field, &gtk::graphene::Point::new(x as f32, y as f32))
            else {
                continue;
            };
            if p.y() < 0.0 {
                break;
            }
            if (p.y() as i32) < field.height() {
                return before + field.drop_index(p.x() as f64, p.y() as f64);
            }
            before += field.imp().pills.borrow().len();
        }
        before
    }

    pub(super) fn index_of(&self, pill: &gtk::Widget) -> Option<usize> {
        self.imp()
            .pills
            .borrow()
            .iter()
            .position(|(p, _)| p == pill)
    }

    /// Unselect every pill of every field.
    pub(super) fn clear_selection(&self) {
        for field in self.fields() {
            for p in field.pill_list().iter().filter(|p| is_selected(p)) {
                set_selected(p, false);
            }
        }
    }

    /// Select `pill` and nothing else, in any field.
    pub(super) fn select_only(&self, pill: &gtk::Widget) {
        self.clear_selection();
        set_selected(pill, true);
        self.imp().anchor.replace(Some(pill.clone()));
    }

    /// Select every pill of this field, and none of the others.
    pub(super) fn select_all(&self) {
        self.clear_selection();
        let pills = self.pill_list();
        for p in &pills {
            set_selected(p, true);
        }
        self.imp().anchor.replace(pills.first().cloned());
    }

    /// Select the pills of this field from the anchor to `pill`, and none
    /// of the others.
    pub(super) fn select_range(&self, pill: &gtk::Widget) {
        let pills = self.pill_list();
        let Some(to) = pills.iter().position(|p| p == pill) else {
            return;
        };
        let anchor = self.imp().anchor.borrow().clone();
        let from = anchor.and_then(|a| pills.iter().position(|p| *p == a));
        if from.is_none() {
            self.imp().anchor.replace(Some(pill.clone()));
        }
        let from = from.unwrap_or(to);
        self.clear_selection();
        for p in &pills[from.min(to)..=from.max(to)] {
            set_selected(p, true);
        }
    }

    /// Where among the pills a pill let go at `(x, y)` goes: before the
    /// first pill on the pointer's line that is past it, after those on
    /// the lines above.
    pub(super) fn drop_index(&self, x: f64, y: f64) -> usize {
        let imp = self.imp();
        let flow = &imp.flow;
        let Some(p) = self.compute_point(flow, &gtk::graphene::Point::new(x as f32, y as f32))
        else {
            return 0;
        };
        let rtl = flow.direction() == gtk::TextDirection::Rtl;
        let y = (p.y() as f64).clamp(0.0, (flow.height() - 1).max(0) as f64);
        imp.pills
            .borrow()
            .iter()
            .filter_map(|(pill, _)| pill.compute_bounds(flow))
            .filter(|b| {
                let (top, bottom) = (b.y() as f64, (b.y() + b.height()) as f64);
                let middle = (b.x() + b.width() / 2.0) as f64;
                let past = if rtl {
                    (p.x() as f64) > middle
                } else {
                    (p.x() as f64) < middle
                };
                bottom <= y || (top <= y && !past)
            })
            .count()
    }

    /// The order this field's pills would have with `moving` put at place
    /// `at`, or `None` when some come from another field.
    fn order_after(
        &self,
        moving: &[(RecipientRow, gtk::Widget)],
        at: usize,
    ) -> Option<Vec<gtk::Widget>> {
        if moving.iter().any(|(f, _)| f != self) {
            return None;
        }
        let pills = self.pill_list();
        let is_moving = |p: &gtk::Widget| moving.iter().any(|(_, m)| m == p);
        let next = pills.iter().skip(at).find(|p| !is_moving(p));
        let mut order: Vec<_> = pills.iter().filter(|p| !is_moving(p)).cloned().collect();
        let i = next
            .and_then(|n| order.iter().position(|p| p == n))
            .unwrap_or(order.len());
        order.splice(i..i, moving.iter().map(|(_, p)| p.clone()));
        Some(order)
    }

    /// Whether putting `moving` at place `at` leaves this field as it is.
    pub(super) fn would_stay(&self, moving: &[(RecipientRow, gtk::Widget)], at: usize) -> bool {
        !moving.is_empty()
            && self
                .order_after(moving, at)
                .is_some_and(|o| o == self.pill_list())
    }

    /// Move `moving`, pills of any field this one included, to place `at`
    /// among this field's pills, in their order. An address this field
    /// already has is not added twice. The pills stay selected in their new
    /// place, and the one that had the focus keeps it. Returns them in
    /// their new place, in order.
    pub(super) fn take_pills(
        &self,
        moving: Vec<(RecipientRow, gtk::Widget)>,
        at: usize,
    ) -> Vec<gtk::Widget> {
        if moving.is_empty() || self.would_stay(&moving, at) {
            return moving.into_iter().map(|(_, p)| p).collect();
        }
        let imp = self.imp();
        let next = self
            .pill_list()
            .into_iter()
            .skip(at)
            .find(|p| !moving.iter().any(|(_, m)| m == p));
        let mut taken = Vec::new();
        let mut changed = vec![self.clone()];
        for (field, pill) in &moving {
            let Some(i) = field.index_of(pill) else {
                continue;
            };
            let recipient = field.imp().pills.borrow()[i].1.clone();
            // One that does not read as an address stays where it is
            // rather than be lost.
            let Some((name, addr)) = crate::worker::parse_recipients(&recipient)
                .into_iter()
                .next()
            else {
                continue;
            };
            let focused = pill.is_focus();
            field.imp().pills.borrow_mut().remove(i);
            field.imp().flow.remove(pill);
            if !changed.contains(field) {
                changed.push(field.clone());
            }
            taken.push((name, addr, recipient, focused));
        }
        let mut at = next
            .and_then(|n| self.index_of(&n))
            .unwrap_or(imp.pills.borrow().len());
        let mut focus = None;
        let mut first = None;
        let mut placed = Vec::new();
        for (name, addr, recipient, focused) in taken {
            let have = imp.pills.borrow().iter().find_map(|(p, r)| {
                let other = crate::worker::parse_recipients(r)
                    .into_iter()
                    .next()
                    .map(|(_, a)| a);
                other
                    .is_some_and(|a| a.eq_ignore_ascii_case(&addr))
                    .then(|| p.clone())
            });
            let pill = have.unwrap_or_else(|| {
                let p = self.insert_pill(at, &name, &addr, &recipient);
                at += 1;
                p
            });
            set_selected(&pill, true);
            first.get_or_insert_with(|| pill.clone());
            placed.push(pill.clone());
            if focused {
                focus = Some(pill);
            }
        }
        imp.anchor.replace(first);
        for field in changed {
            field.emit_changed();
        }
        if let Some(p) = focus {
            p.grab_focus();
        }
        placed
    }
}
