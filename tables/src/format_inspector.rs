// format_inspector.rs — the Format inspector: a sidebar that edits the
// selected cells' style (docs/DESIGN-UI.md, "The Format inspector").
// SPDX-License-Identifier: GPL-3.0-or-later
//
// iWork's idea, in libadwaita's form: an AdwOverlaySplitView at the end of
// the window, AdwPreferencesGroup rows for the CellStyle fields, shown and
// hidden by a header-bar toggle. Every edit goes through
// tables_core::controller's format commands, so each is one undo step over
// the whole selection. The widgets only wire signals; the rules live in
// tables-core.

use gtk4::{self as gtk, gdk, prelude::*};
use libadwaita as adw;
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use tables_core::controller::{BorderPreset, WorkbookController};
use tables_core::sheet::BorderStyle;
use tables_core::style::{CellStyle, HAlign, Rgb, VAlign};

/// The inspector, wrapped round the window's content.
pub struct FormatInspector {
    pub split: adw::OverlaySplitView,
    /// Show the active cell's style. Call when the selection changes.
    pub sync: Rc<dyn Fn()>,
}

type Ctl = Rc<RefCell<WorkbookController>>;

/// Where the window keeps the inspector's sync until the inspector exists:
/// the selection-change handler is built before it.
pub type SyncSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

fn rgba(c: Rgb) -> gdk::RGBA {
    let (r, g, b) = c.to_f64();
    gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0)
}

fn rgb(c: &gdk::RGBA) -> Rgb {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb(q(c.red()), q(c.green()), q(c.blue()))
}

/// A row with a widget at its end.
fn row(title: &str, suffix: &impl IsA<gtk::Widget>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(suffix);
    row
}

/// A linked box of buttons, as a segmented control.
fn linked(buttons: &[&gtk::ToggleButton]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("linked");
    b.set_valign(gtk::Align::Center);
    for button in buttons {
        b.append(*button);
    }
    b
}

fn toggle(icon: Option<&str>, label: &str, group: Option<&gtk::ToggleButton>) -> gtk::ToggleButton {
    let t = gtk::ToggleButton::new();
    match icon {
        Some(icon) => {
            t.set_icon_name(icon);
            t.set_tooltip_text(Some(label));
        }
        None => t.set_label(label),
    }
    // The accessible name is the label either way, so AT-SPI can find it.
    t.update_property(&[gtk::accessible::Property::Label(label)]);
    if let Some(g) = group {
        t.set_group(Some(g));
    }
    t
}

fn clear_button(tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name("edit-clear-symbolic");
    b.set_tooltip_text(Some(tooltip));
    b.add_css_class("flat");
    b.set_valign(gtk::Align::Center);
    b
}

/// Build the inspector round `content` and add its toggle to `header`.
pub fn build(
    ctl: &Ctl,
    grid: &gtk::DrawingArea,
    header: &adw::HeaderBar,
    breakpoints: &[&adw::Breakpoint],
    content: &impl IsA<gtk::Widget>,
) -> FormatInspector {
    // While the widgets are being set from the model, their change signals
    // must not write back into it.
    let syncing = Rc::new(Cell::new(false));
    let apply = {
        let ctl = ctl.clone();
        let grid = grid.clone();
        let syncing = syncing.clone();
        Rc::new(move |description: &'static str, change: &dyn Fn(&mut CellStyle)| {
            if syncing.get() {
                return;
            }
            ctl.borrow_mut().format_selection(description, change);
            // A bigger font or wrapped text needs a taller row.
            {
                let c = ctl.borrow();
                let state = c.state.borrow();
                crate::grid_render::fit_rows_to_content(&mut state.sheet_mut());
            }
            grid.queue_draw();
        })
    };

    // ── Text ────────────────────────────────────────────────────────────
    let text = adw::PreferencesGroup::builder().title("Text").build();

    // A button naming the family, not a GtkFontDialogButton: that one
    // shows "None" for a family that isn't installed under its own name,
    // which is the common case (Calibri drawn as metric-compatible Carlito).
    let font = gtk::Button::with_label(tables_core::sheet::DEFAULT_FONT_FAMILY);
    font.set_valign(gtk::Align::Center);
    font.update_property(&[gtk::accessible::Property::Label("Font")]);
    text.add(&row("Font", &font));

    let size = adw::SpinRow::with_range(6.0, 96.0, 1.0);
    size.set_title("Size");
    size.set_digits(1);
    text.add(&size);

    let bold = toggle(Some("format-text-bold-symbolic"), "Bold", None);
    let italic = toggle(Some("format-text-italic-symbolic"), "Italic", None);
    let underline = toggle(Some("format-text-underline-symbolic"), "Underline", None);
    let strike = toggle(Some("format-text-strikethrough-symbolic"), "Strikethrough", None);
    text.add(&row("Style", &linked(&[&bold, &italic, &underline, &strike])));

    let color = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::builder().title("Text Colour").with_alpha(false).build()));
    color.set_valign(gtk::Align::Center);
    let color_auto = clear_button("Automatic Colour");
    let color_row = row("Colour", &color);
    color_row.add_suffix(&color_auto);
    text.add(&color_row);

    // ── Cell ────────────────────────────────────────────────────────────
    let cell = adw::PreferencesGroup::builder().title("Cell").build();

    let fill = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::builder().title("Fill").with_alpha(false).build()));
    fill.set_valign(gtk::Align::Center);
    let fill_none = clear_button("No Fill");
    let fill_row = row("Fill", &fill);
    fill_row.add_suffix(&fill_none);
    cell.add(&fill_row);

    let h_auto = toggle(None, "Auto", None);
    let h_left = toggle(Some("format-justify-left-symbolic"), "Align Left", Some(&h_auto));
    let h_center = toggle(Some("format-justify-center-symbolic"), "Align Centre", Some(&h_auto));
    let h_right = toggle(Some("format-justify-right-symbolic"), "Align Right", Some(&h_auto));
    cell.add(&row("Align", &linked(&[&h_auto, &h_left, &h_center, &h_right])));

    let vertical = adw::ComboRow::builder()
        .title("Vertical")
        .model(&gtk::StringList::new(&["Top", "Middle", "Bottom"]))
        .build();
    cell.add(&vertical);

    let wrap = adw::SwitchRow::builder().title("Wrap Text").build();
    cell.add(&wrap);

    // ── Borders ─────────────────────────────────────────────────────────
    let borders = adw::PreferencesGroup::builder().title("Borders").build();
    let weights = ["Thin", "Medium", "Thick"];
    let weight = adw::ComboRow::builder().title("Weight").model(&gtk::StringList::new(&weights)).build();
    borders.add(&weight);
    let b_all = gtk::Button::with_label("All");
    let b_outline = gtk::Button::with_label("Outline");
    let b_none = gtk::Button::with_label("None");
    let b_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b_box.add_css_class("linked");
    b_box.set_valign(gtk::Align::Center);
    for b in [&b_all, &b_outline, &b_none] {
        b_box.append(b);
    }
    borders.add(&row("Apply", &b_box));

    let page = adw::PreferencesPage::new();
    page.add(&text);
    page.add(&cell);
    page.add(&borders);

    let split = adw::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::End)
        .show_sidebar(false)
        .min_sidebar_width(280.0)
        .content(content)
        .sidebar(&page)
        .build();
    // Narrow windows: the inspector slides over the grid instead of
    // squeezing it. (A window applies one breakpoint at a time, so each
    // one that should collapse it says so.)
    for b in breakpoints {
        b.add_setter(&split, "collapsed", Some(&true.to_value()));
    }

    let show = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-right-symbolic")
        .tooltip_text("Format")
        .build();
    show.update_property(&[gtk::accessible::Property::Label("Format")]);
    show.bind_property("active", &split, "show-sidebar").bidirectional().sync_create().build();
    header.pack_end(&show);

    // ── Model → widgets ─────────────────────────────────────────────────
    let sync: Rc<dyn Fn()> = {
        let ctl = ctl.clone();
        let syncing = syncing.clone();
        let (font, size, color, fill, wrap, vertical) =
            (font.clone(), size.clone(), color.clone(), fill.clone(), wrap.clone(), vertical.clone());
        let flags = [bold.clone(), italic.clone(), underline.clone(), strike.clone()];
        let h = [h_auto.clone(), h_left.clone(), h_center.clone(), h_right.clone()];
        Rc::new(move || {
            // Skip rather than panic if called mid-edit; the next
            // selection change catches up.
            let Ok(c) = ctl.try_borrow() else { return };
            let (s, _) = c.active_style();
            let (default_family, default_size) = c.default_font();
            drop(c);
            syncing.set(true);
            let family = s.font_family.as_deref().unwrap_or(&default_family);
            font.set_label(family);
            size.set_value(s.font_size.unwrap_or(default_size));
            for (t, on) in flags.iter().zip([s.bold, s.italic, s.underline, s.strikethrough]) {
                t.set_active(on);
            }
            color.set_rgba(&rgba(s.color.unwrap_or(Rgb(0, 0, 0))));
            fill.set_rgba(&rgba(s.fill.unwrap_or(Rgb(0xFF, 0xFF, 0xFF))));
            h[match s.h_align {
                HAlign::General => 0,
                HAlign::Left => 1,
                HAlign::Center => 2,
                HAlign::Right => 3,
            }]
            .set_active(true);
            vertical.set_selected(match s.v_align {
                VAlign::Top => 0,
                VAlign::Center => 1,
                VAlign::Bottom => 2,
            });
            wrap.set_active(s.wrap);
            syncing.set(false);
        })
    };

    // ── Widgets → model ─────────────────────────────────────────────────
    {
        let apply = apply.clone();
        font.connect_clicked(move |b| {
            let apply = apply.clone();
            let b2 = b.clone();
            let parent = b.root().and_downcast::<gtk::Window>();
            gtk::FontDialog::builder().title("Font").build().choose_family(
                parent.as_ref(),
                None::<&gtk::pango::FontFamily>,
                None::<&gtk::gio::Cancellable>,
                move |chosen| {
                    // Cancelling the dialog is an error here, and changes nothing.
                    let Ok(family) = chosen else { return };
                    let name = family.name().to_string();
                    b2.set_label(&name);
                    apply("Font", &move |s: &mut CellStyle| {
                        // Explicit, not "the default": which font that is
                        // depends on the workbook (Calibri, Liberation Sans…).
                        s.font_family = Some(name.clone())
                    });
                },
            );
        });
    }
    {
        let apply = apply.clone();
        size.connect_value_notify(move |r| {
            let v = r.value();
            apply("Font Size", &move |s: &mut CellStyle| {
                s.font_size = Some(v)
            });
        });
    }
    type Flag = fn(&mut CellStyle) -> &mut bool;
    let flag_edits: [(&gtk::ToggleButton, &'static str, Flag); 4] = [
        (&bold, "Bold", |s| &mut s.bold),
        (&italic, "Italic", |s| &mut s.italic),
        (&underline, "Underline", |s| &mut s.underline),
        (&strike, "Strikethrough", |s| &mut s.strikethrough),
    ];
    for (button, description, field) in flag_edits {
        let apply = apply.clone();
        button.connect_toggled(move |t| {
            let on = t.is_active();
            apply(description, &move |s: &mut CellStyle| *field(s) = on);
        });
    }
    {
        let apply = apply.clone();
        color.connect_rgba_notify(move |b| {
            let c = rgb(&b.rgba());
            apply("Text Colour", &move |s: &mut CellStyle| s.color = Some(c).filter(|c| *c != Rgb(0, 0, 0)));
        });
    }
    {
        let (apply, sync) = (apply.clone(), sync.clone());
        color_auto.connect_clicked(move |_| {
            apply("Text Colour", &|s: &mut CellStyle| s.color = None);
            sync();
        });
    }
    {
        let apply = apply.clone();
        fill.connect_rgba_notify(move |b| {
            let c = rgb(&b.rgba());
            apply("Fill Colour", &move |s: &mut CellStyle| s.fill = Some(c));
        });
    }
    {
        let (apply, sync) = (apply.clone(), sync.clone());
        fill_none.connect_clicked(move |_| {
            apply("No Fill", &|s: &mut CellStyle| s.fill = None);
            sync();
        });
    }
    for (button, align) in [(&h_auto, HAlign::General), (&h_left, HAlign::Left), (&h_center, HAlign::Center), (&h_right, HAlign::Right)] {
        let apply = apply.clone();
        button.connect_toggled(move |t| {
            if t.is_active() {
                apply("Alignment", &move |s: &mut CellStyle| s.h_align = align);
            }
        });
    }
    {
        let apply = apply.clone();
        vertical.connect_selected_notify(move |r| {
            let align = match r.selected() {
                0 => VAlign::Top,
                1 => VAlign::Center,
                _ => VAlign::Bottom,
            };
            apply("Vertical Alignment", &move |s: &mut CellStyle| s.v_align = align);
        });
    }
    {
        let apply = apply.clone();
        wrap.connect_active_notify(move |r| {
            let on = r.is_active();
            apply("Wrap Text", &move |s: &mut CellStyle| s.wrap = on);
        });
    }
    for (button, preset) in [(&b_all, BorderPreset::All), (&b_outline, BorderPreset::Outline), (&b_none, BorderPreset::None)] {
        let (ctl, grid, weight) = (ctl.clone(), grid.clone(), weight.clone());
        button.connect_clicked(move |_| {
            let style = match weight.selected() {
                1 => BorderStyle::Medium,
                2 => BorderStyle::Thick,
                _ => BorderStyle::Solid,
            };
            ctl.borrow_mut().border_selection(preset, style, (0.0, 0.0, 0.0));
            grid.queue_draw();
        });
    }

    // A closed inspector is not part of the window: an AdwOverlaySplitView
    // keeps its hidden sidebar in the accessibility tree, where its combo
    // rows were found by tests (and screen readers) looking for the sheet
    // switcher. Show the right values each time it opens.
    page.set_visible(false);
    {
        let sync = sync.clone();
        let page = page.clone();
        split.connect_show_sidebar_notify(move |s| {
            page.set_visible(s.shows_sidebar());
            if s.shows_sidebar() {
                sync();
            }
        });
    }

    FormatInspector { split, sync }
}
