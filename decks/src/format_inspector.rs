// format_inspector.rs — Decks' Format inspector: a sidebar that edits the
// selected object (docs/DESIGN-UI.md, "The Format inspector").
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Keynote's inspector in libadwaita's form, as Tables' (#962): three
// AdwViewStack pages switched by an AdwViewSwitcher — Style (fill,
// outline, shape), Text (font, size, emphasis, colour, alignment, list,
// vertical anchor) and Arrange (position, size, rotation, order) — each of
// AdwPreferencesGroup rows. Every edit is a decks_core::format::FormatEdit
// applied through DecksController::format_objects: one undo step over the
// selection. This module only builds widgets and wires signals.

use adw::prelude::*;
use decks_core::engine::shape::{Color, ShapeKind};
use decks_core::engine::{Anchor, ParaAlign, Transition};
use decks_core::builds::BuildEffect;
use decks_core::format::{FormatEdit, ListKind, ObjectFormat};
use decks_core::undo::ZOrderOp;
use decks_core::DecksController;
use gtk4::{self as gtk, gdk};
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;

/// Where the window keeps the inspector's refresh until the inspector
/// exists: the HUD refresh that calls it is built first.
pub type SyncSlot = Rc<std::cell::RefCell<Option<Rc<dyn Fn()>>>>;

type MakeKind = fn() -> ShapeKind;

/// The inspector's sidebar and its model→widget refresh.
pub struct FormatInspector {
    pub sidebar: gtk::Widget,
    /// Show the selected object's format. Call when the selection, the
    /// slide or the object changes.
    pub sync: Rc<dyn Fn()>,
}

fn rgba(c: Color) -> gdk::RGBA {
    let (r, g, b) = c.to_f64();
    gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0)
}

fn color(c: &gdk::RGBA) -> Color {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color(q(c.red()), q(c.green()), q(c.blue()))
}

fn row(title: &str, suffix: &impl IsA<gtk::Widget>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(suffix);
    row
}

fn linked(buttons: &[&gtk::Widget]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("linked");
    b.set_valign(gtk::Align::Center);
    for w in buttons {
        b.append(*w);
    }
    b
}

fn toggle(icon: &str, label: &str, group: Option<&gtk::ToggleButton>) -> gtk::ToggleButton {
    let t = gtk::ToggleButton::new();
    t.set_icon_name(icon);
    t.set_tooltip_text(Some(label));
    t.update_property(&[gtk::accessible::Property::Label(label)]);
    if let Some(g) = group {
        t.set_group(Some(g));
    }
    t
}

fn icon_button(icon: &str, label: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(label));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b.set_valign(gtk::Align::Center);
    b
}

fn clear_button(label: &str) -> gtk::Button {
    let b = icon_button("edit-clear-symbolic", label);
    b.add_css_class("flat");
    b
}

fn color_button(title: &str) -> gtk::ColorDialogButton {
    let b = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::builder().title(title).with_alpha(false).build()));
    b.set_valign(gtk::Align::Center);
    b.update_property(&[gtk::accessible::Property::Label(title)]);
    b
}

fn spin(title: &str, lo: f64, hi: f64, step: f64) -> adw::SpinRow {
    let r = adw::SpinRow::with_range(lo, hi, step);
    r.set_title(title);
    r
}

fn combo(title: &str, items: &[&str]) -> adw::ComboRow {
    adw::ComboRow::builder().title(title).model(&gtk::StringList::new(items)).build()
}

const KINDS: [(&str, MakeKind); 5] = [
    ("Rectangle", || ShapeKind::Rect),
    ("Rounded Rectangle", || ShapeKind::RoundRect { radius: 1.0 / 6.0 }),
    ("Ellipse", || ShapeKind::Ellipse),
    ("Triangle", || ShapeKind::Triangle),
    ("Diamond", || ShapeKind::Diamond),
];

fn kind_index(k: &ShapeKind) -> Option<u32> {
    Some(match k {
        ShapeKind::Rect => 0,
        ShapeKind::RoundRect { .. } => 1,
        ShapeKind::Ellipse => 2,
        ShapeKind::Triangle => 3,
        ShapeKind::Diamond => 4,
        ShapeKind::Other(_) => return None,
    })
}

fn page(groups: &[&adw::PreferencesGroup]) -> adw::PreferencesPage {
    let p = adw::PreferencesPage::new();
    for g in groups {
        p.add(*g);
    }
    p
}

/// Build the inspector. `changed` redraws the canvas and the rest of the
/// window after an edit.
pub fn build(
    ctl: &Rc<DecksController>,
    current_slide: &Rc<Cell<usize>>,
    selected: &Rc<Cell<Option<usize>>>,
    changed: Rc<dyn Fn()>,
) -> FormatInspector {
    // Set while widgets are filled from the model, so their change signals
    // don't write straight back into it.
    let syncing = Rc::new(Cell::new(false));
    let apply: Rc<dyn Fn(FormatEdit)> = {
        let (ctl, cs, sel, syncing, changed) = (ctl.clone(), current_slide.clone(), selected.clone(), syncing.clone(), changed.clone());
        Rc::new(move |edit: FormatEdit| {
            if syncing.get() {
                return;
            }
            let Some(oi) = sel.get() else { return };
            if ctl.format_objects(cs.get(), &[oi], &edit) {
                changed();
            }
        })
    };

    // ── Style ────────────────────────────────────────────────────────────
    let fill_group = adw::PreferencesGroup::builder().title("Fill").build();
    let fill = color_button("Fill Colour");
    let fill_none = clear_button("No Fill");
    let fill_row = row("Colour", &fill);
    fill_row.add_suffix(&fill_none);
    fill_group.add(&fill_row);

    let outline_group = adw::PreferencesGroup::builder().title("Outline").build();
    let outline = color_button("Outline Colour");
    let outline_none = clear_button("No Outline");
    let outline_row = row("Colour", &outline);
    outline_row.add_suffix(&outline_none);
    outline_group.add(&outline_row);
    let outline_width = spin("Width", 0.0, 50.0, 0.5);
    outline_width.set_digits(1);
    outline_group.add(&outline_width);

    let shape_group = adw::PreferencesGroup::builder().title("Shape").build();
    let kind_names: Vec<&str> = KINDS.iter().map(|(n, _)| *n).collect();
    let kind = combo("Kind", &kind_names);
    shape_group.add(&kind);
    let style_page = page(&[&fill_group, &outline_group, &shape_group]);

    // ── Text ─────────────────────────────────────────────────────────────
    let font_group = adw::PreferencesGroup::builder().title("Font").build();
    // A button naming the family, as in Tables: GtkFontDialogButton shows
    // "None" for a family installed under another name (Calibri drawn as
    // Carlito), the common case in imported decks.
    let font = gtk::Button::with_label("Default");
    font.set_valign(gtk::Align::Center);
    font.update_property(&[gtk::accessible::Property::Label("Font")]);
    let font_default = clear_button("Default Font");
    let font_row = row("Family", &font);
    font_row.add_suffix(&font_default);
    font_group.add(&font_row);
    let size = spin("Size", 4.0, 400.0, 1.0);
    size.set_digits(1);
    font_group.add(&size);
    let bold = toggle("format-text-bold-symbolic", "Bold", None);
    let italic = toggle("format-text-italic-symbolic", "Italic", None);
    font_group.add(&row("Style", &linked(&[bold.upcast_ref(), italic.upcast_ref()])));
    let text_color = color_button("Text Colour");
    let text_color_auto = clear_button("Automatic Colour");
    let text_color_row = row("Colour", &text_color);
    text_color_row.add_suffix(&text_color_auto);
    font_group.add(&text_color_row);

    let para_group = adw::PreferencesGroup::builder().title("Paragraph").build();
    let a_left = toggle("format-justify-left-symbolic", "Align Left", None);
    let a_center = toggle("format-justify-center-symbolic", "Align Centre", Some(&a_left));
    let a_right = toggle("format-justify-right-symbolic", "Align Right", Some(&a_left));
    let a_just = toggle("format-justify-fill-symbolic", "Justify", Some(&a_left));
    para_group.add(&row(
        "Alignment",
        &linked(&[a_left.upcast_ref(), a_center.upcast_ref(), a_right.upcast_ref(), a_just.upcast_ref()]),
    ));
    let list = combo("List", &["None", "Bullets", "Numbers"]);
    para_group.add(&list);
    let anchor = combo("Vertical", &["Top", "Middle", "Bottom"]);
    para_group.add(&anchor);
    let text_page = page(&[&font_group, &para_group]);

    // ── Arrange ──────────────────────────────────────────────────────────
    let pos_group = adw::PreferencesGroup::builder().title("Position").build();
    let x = spin("X", -2000.0, 4000.0, 1.0);
    let y = spin("Y", -2000.0, 4000.0, 1.0);
    pos_group.add(&x);
    pos_group.add(&y);
    let size_group = adw::PreferencesGroup::builder().title("Size").build();
    let w = spin("Width", 1.0, 4000.0, 1.0);
    let h = spin("Height", 1.0, 4000.0, 1.0);
    size_group.add(&w);
    size_group.add(&h);
    let turn_group = adw::PreferencesGroup::builder().title("Rotation").build();
    let rotation = spin("Angle", 0.0, 359.0, 1.0);
    rotation.set_wrap(true);
    turn_group.add(&rotation);
    let order_group = adw::PreferencesGroup::builder().title("Order").build();
    let to_back = icon_button("go-bottom-symbolic", "Send to Back");
    let backward = icon_button("go-down-symbolic", "Send Backward");
    let forward = icon_button("go-up-symbolic", "Bring Forward");
    let to_front = icon_button("go-top-symbolic", "Bring to Front");
    order_group.add(&row(
        "Layer",
        &linked(&[to_back.upcast_ref(), backward.upcast_ref(), forward.upcast_ref(), to_front.upcast_ref()]),
    ));
    let arrange_page = page(&[&pos_group, &size_group, &turn_group, &order_group]);

    // ── Animate (Keynote's builds) ─────────────────────────────────────────
    let build_group = adw::PreferencesGroup::builder()
        .title("Build")
        .description("How the object arrives and leaves during a show, one click each")
        .build();
    let mut effect_names = vec!["None"];
    effect_names.extend(BuildEffect::ALL.iter().map(|e| e.label()));
    let build_in = combo("Build In", &effect_names);
    let out_names: Vec<String> = effect_names.iter().map(|n| n.replace(" from ", " to ")).collect();
    let out_refs: Vec<&str> = out_names.iter().map(String::as_str).collect();
    let build_out = combo("Build Out", &out_refs);
    let build_order = adw::ActionRow::builder().title("Order").build();
    build_order.add_css_class("property");
    build_group.add(&build_in);
    build_group.add(&build_out);
    build_group.add(&build_order);
    let animate_page = page(&[&build_group]);

    // ── Tabs ─────────────────────────────────────────────────────────────
    let stack = adw::ViewStack::new();
    let style_tab = stack.add_titled_with_icon(&style_page, Some("style"), "Style", "applications-graphics-symbolic");
    let text_tab = stack.add_titled_with_icon(&text_page, Some("text"), "Text", "format-text-rich-symbolic");
    stack.add_titled_with_icon(&arrange_page, Some("arrange"), "Arrange", "object-select-symbolic");
    stack.add_titled_with_icon(&animate_page, Some("animate"), "Animate", "media-playback-start-symbolic");
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
    switcher.set_margin_top(6);
    switcher.set_margin_bottom(6);
    switcher.set_margin_start(6);
    switcher.set_margin_end(6);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&switcher);
    view.set_content(Some(&stack));

    // Nothing selected: the slide itself (Keynote's Animate/Slide
    // inspector), with the hint that selecting an object formats it.
    let slide_group = adw::PreferencesGroup::builder()
        .title("Slide")
        .description("Select an object on the slide to format it")
        .build();
    let labels: Vec<&str> = Transition::ALL.iter().map(|t| t.label()).collect();
    let transition = combo("Transition", &labels);
    transition.set_subtitle("How this slide arrives");
    let preview = gtk::Button::from_icon_name("media-playback-start-symbolic");
    preview.set_tooltip_text(Some("Preview Transition"));
    preview.update_property(&[gtk::accessible::Property::Label("Preview Transition")]);
    preview.set_valign(gtk::Align::Center);
    preview.add_css_class("flat");
    preview.set_action_name(Some("app.preview-transition"));
    transition.add_suffix(&preview);
    slide_group.add(&transition);
    let empty = page(&[&slide_group]);
    let outer = gtk::Stack::new();
    outer.add_named(&empty, Some("empty"));
    outer.add_named(&view, Some("inspector"));
    outer.set_width_request(280);

    // ── Model → widgets ──────────────────────────────────────────────────
    let sync: Rc<dyn Fn()> = {
        let (ctl, cs, sel, syncing) = (ctl.clone(), current_slide.clone(), selected.clone(), syncing.clone());
        let (outer, stack) = (outer.clone(), stack.clone());
        let (style_tab, text_tab) = (style_tab.clone(), text_tab.clone());
        let (fill, outline, outline_width, kind) = (fill.clone(), outline.clone(), outline_width.clone(), kind.clone());
        let (font, size, bold, italic, text_color) =
            (font.clone(), size.clone(), bold.clone(), italic.clone(), text_color.clone());
        let aligns = [a_left.clone(), a_center.clone(), a_right.clone(), a_just.clone()];
        let (list, anchor, transition) = (list.clone(), anchor.clone(), transition.clone());
        let (x, y, w, h, rotation) = (x.clone(), y.clone(), w.clone(), h.clone(), rotation.clone());
        let (build_in, build_out, build_order) = (build_in.clone(), build_out.clone(), build_order.clone());
        Rc::new(move || {
            let f: Option<ObjectFormat> = sel.get().and_then(|oi| ctl.object_format(cs.get(), oi));
            let Some(f) = f else {
                outer.set_visible_child_name("empty");
                let current = ctl.slides.borrow().get(cs.get()).map(|s| s.transition).unwrap_or_default();
                syncing.set(true);
                if let Some(i) = Transition::ALL.iter().position(|t| *t == current) {
                    transition.set_selected(i as u32);
                }
                syncing.set(false);
                return;
            };
            outer.set_visible_child_name("inspector");
            syncing.set(true);
            style_tab.set_visible(f.has_style);
            text_tab.set_visible(f.has_text);
            let visible = stack.visible_child_name().map(|n| n.to_string());
            let shown_ok = match visible.as_deref() {
                Some("style") => f.has_style,
                Some("text") => f.has_text,
                _ => true,
            };
            if !shown_ok {
                stack.set_visible_child_name(if f.has_style { "style" } else if f.has_text { "text" } else { "arrange" });
            }
            fill.set_rgba(&rgba(f.fill.unwrap_or(Color(0xFF, 0xFF, 0xFF))));
            outline.set_rgba(&rgba(f.outline.map_or(Color(0, 0, 0), |s| s.color)));
            outline_width.set_value(f.outline.map_or(0.0, |s| s.width));
            if let Some(i) = f.kind.as_ref().and_then(kind_index) {
                kind.set_selected(i);
            }
            font.set_label(f.font_family.as_deref().unwrap_or("Default"));
            size.set_value(f.font_size.unwrap_or(decks_core::format::DEFAULT_FONT_PT));
            bold.set_active(f.bold);
            italic.set_active(f.italic);
            let tc = f.text_color.as_deref().and_then(Color::from_hex).unwrap_or(Color(0, 0, 0));
            text_color.set_rgba(&rgba(tc));
            aligns[match f.align {
                ParaAlign::Left => 0,
                ParaAlign::Center => 1,
                ParaAlign::Right => 2,
                ParaAlign::Justify => 3,
            }]
            .set_active(true);
            list.set_selected(match f.list {
                ListKind::None => 0,
                ListKind::Bullet => 1,
                ListKind::Number => 2,
            });
            anchor.set_selected(match f.anchor {
                Anchor::Top => 0,
                Anchor::Middle => 1,
                Anchor::Bottom => 2,
            });
            let (bx, by, bw, bh) = f.bounds;
            x.set_value(bx);
            y.set_value(by);
            w.set_value(bw);
            h.set_value(bh);
            rotation.set_value(f.rotation.rem_euclid(360.0).round());
            if let (Some(oi), Some(slide)) = (sel.get(), ctl.slides.borrow().get(cs.get())) {
                let pick = |out: bool| {
                    slide
                        .builds
                        .iter()
                        .find(|b| b.object == oi && b.out == out)
                        .and_then(|b| BuildEffect::ALL.iter().position(|e| *e == b.effect))
                        .map_or(0, |i| i as u32 + 1)
                };
                build_in.set_selected(pick(false));
                build_out.set_selected(pick(true));
                let mine: Vec<usize> = slide
                    .builds
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.object == oi)
                    .map(|(i, _)| i + 1)
                    .collect();
                let n = slide.builds.len();
                build_order.set_subtitle(&match mine.as_slice() {
                    [] => "Always on the slide".to_string(),
                    steps => format!(
                        "Click {} of {n}",
                        steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(" and ")
                    ),
                });
            }
            syncing.set(false);
        })
    };

    // ── Widgets → model ──────────────────────────────────────────────────
    let on = |apply: &Rc<dyn Fn(FormatEdit)>| apply.clone();
    {
        let apply = on(&apply);
        fill.connect_rgba_notify(move |b| apply(FormatEdit::Fill(Some(color(&b.rgba())))));
    }
    {
        let (apply, sync) = (on(&apply), sync.clone());
        fill_none.connect_clicked(move |_| {
            apply(FormatEdit::Fill(None));
            sync();
        });
    }
    {
        let apply = on(&apply);
        outline.connect_rgba_notify(move |b| apply(FormatEdit::OutlineColor(color(&b.rgba()))));
    }
    {
        let (apply, sync) = (on(&apply), sync.clone());
        outline_none.connect_clicked(move |_| {
            apply(FormatEdit::NoOutline);
            sync();
        });
    }
    {
        let apply = on(&apply);
        outline_width.connect_value_notify(move |r| apply(FormatEdit::OutlineWidth(r.value())));
    }
    {
        let apply = on(&apply);
        kind.connect_selected_notify(move |r| {
            if let Some((_, make)) = KINDS.get(r.selected() as usize) {
                apply(FormatEdit::Kind(make()));
            }
        });
    }
    {
        let apply = on(&apply);
        font.connect_clicked(move |b| {
            let apply = apply.clone();
            let b2 = b.clone();
            let parent = b.root().and_downcast::<gtk::Window>();
            gtk::FontDialog::builder().title("Font").build().choose_family(
                parent.as_ref(),
                None::<&gtk::pango::FontFamily>,
                None::<&gtk::gio::Cancellable>,
                move |chosen| {
                    // Cancelling is an error here, and changes nothing.
                    let Ok(family) = chosen else { return };
                    let name = family.name().to_string();
                    b2.set_label(&name);
                    apply(FormatEdit::FontFamily(Some(name)));
                },
            );
        });
    }
    {
        let (apply, sync) = (on(&apply), sync.clone());
        font_default.connect_clicked(move |_| {
            apply(FormatEdit::FontFamily(None));
            sync();
        });
    }
    {
        let apply = on(&apply);
        size.connect_value_notify(move |r| apply(FormatEdit::FontSize(r.value())));
    }
    {
        let apply = on(&apply);
        bold.connect_toggled(move |t| apply(FormatEdit::Bold(t.is_active())));
    }
    {
        let apply = on(&apply);
        italic.connect_toggled(move |t| apply(FormatEdit::Italic(t.is_active())));
    }
    {
        let apply = on(&apply);
        text_color.connect_rgba_notify(move |b| apply(FormatEdit::TextColor(Some(color(&b.rgba()).to_hex().to_lowercase()))));
    }
    {
        let (apply, sync) = (on(&apply), sync.clone());
        text_color_auto.connect_clicked(move |_| {
            apply(FormatEdit::TextColor(None));
            sync();
        });
    }
    for (button, align) in [
        (&a_left, ParaAlign::Left),
        (&a_center, ParaAlign::Center),
        (&a_right, ParaAlign::Right),
        (&a_just, ParaAlign::Justify),
    ] {
        let apply = on(&apply);
        button.connect_toggled(move |t| {
            if t.is_active() {
                apply(FormatEdit::Align(align));
            }
        });
    }
    {
        let apply = on(&apply);
        list.connect_selected_notify(move |r| {
            apply(FormatEdit::List(match r.selected() {
                1 => ListKind::Bullet,
                2 => ListKind::Number,
                _ => ListKind::None,
            }))
        });
    }
    {
        let apply = on(&apply);
        anchor.connect_selected_notify(move |r| {
            apply(FormatEdit::Anchor(match r.selected() {
                1 => Anchor::Middle,
                2 => Anchor::Bottom,
                _ => Anchor::Top,
            }))
        });
    }
    type Make = fn(f64) -> FormatEdit;
    let arrange: [(&adw::SpinRow, Make); 5] =
        [(&x, FormatEdit::X), (&y, FormatEdit::Y), (&w, FormatEdit::Width), (&h, FormatEdit::Height), (&rotation, FormatEdit::Rotation)];
    for (spin, make) in arrange {
        let apply = on(&apply);
        spin.connect_value_notify(move |r| apply(make(r.value())));
    }
    for (button, op) in [
        (&to_back, ZOrderOp::SendToBack),
        (&backward, ZOrderOp::SendBackward),
        (&forward, ZOrderOp::BringForward),
        (&to_front, ZOrderOp::BringToFront),
    ] {
        let (ctl, cs, sel, changed) = (ctl.clone(), current_slide.clone(), selected.clone(), changed.clone());
        button.connect_clicked(move |_| {
            let Some(oi) = sel.get() else { return };
            let n = ctl.slides.borrow().get(cs.get()).map_or(0, |s| s.objects.len());
            ctl.z_order_object(cs.get(), oi, op);
            // The selection follows the object to its new place.
            sel.set(Some(decks_core::undo::z_order_index(oi, n, op)));
            changed();
        });
    }

    {
        let (ctl, cs, syncing, changed) = (ctl.clone(), current_slide.clone(), syncing.clone(), changed.clone());
        transition.connect_selected_notify(move |r| {
            if syncing.get() {
                return;
            }
            if let Some(t) = Transition::ALL.get(r.selected() as usize) {
                if ctl.set_transition(cs.get(), *t) {
                    changed();
                }
            }
        });
    }

    for (row, out) in [(&build_in, false), (&build_out, true)] {
        let (ctl, cs, sel, syncing, changed, sync) =
            (ctl.clone(), current_slide.clone(), selected.clone(), syncing.clone(), changed.clone(), sync.clone());
        row.connect_selected_notify(move |r| {
            if syncing.get() {
                return;
            }
            let Some(oi) = sel.get() else { return };
            let effect = (r.selected() as usize).checked_sub(1).and_then(|i| BuildEffect::ALL.get(i).copied());
            if ctl.set_build(cs.get(), oi, out, effect) {
                changed();
                sync();
            }
        });
    }

    FormatInspector { sidebar: outer.upcast(), sync }
}
