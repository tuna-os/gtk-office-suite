//! The toolbar's text and highlight color menus.

use super::toolbar::{bar_menu, js_str, run_restored};
use crate::i18n::i18n;
use adw::prelude::*;

/// The color palette (#358): the GNOME palette, a column per hue from light
/// to dark, then the greys from white to black.
const PALETTE: &[[&str; 5]] = &[
    ["#99c1f1", "#62a0ea", "#3584e4", "#1c71d8", "#1a5fb4"],
    ["#8ff0a4", "#57e389", "#33d17a", "#2ec27e", "#26a269"],
    ["#f9f06b", "#f8e45c", "#f6d32d", "#f5c211", "#e5a50a"],
    ["#ffbe6f", "#ffa348", "#ff7800", "#e66100", "#c64600"],
    ["#f66151", "#ed333b", "#e01b24", "#c01c28", "#a51d2d"],
    ["#dc8add", "#c061cb", "#9141ac", "#813d9c", "#613583"],
    ["#cdab8f", "#b5835a", "#986a44", "#865e3c", "#63452c"],
    ["#ffffff", "#f6f5f4", "#deddda", "#c0bfbc", "#9a9996"],
    ["#77767b", "#5e5c64", "#3d3846", "#241f31", "#000000"],
];

/// A color menu (#358): a clearing item, the palette, and Custom… for any
/// other color. `cmd` is the editing command (`foreColor`, `hiliteColor`),
/// and `glyph` the button's face, repainted with the color last chosen.
pub(super) fn color_menu(
    webview: &webkit6::WebView,
    tip: &str,
    clear: &str,
    cmd: &'static str,
    none: &'static str,
    (glyph, repaint): (gtk::Widget, std::rc::Rc<dyn Fn()>),
    chosen: std::rc::Rc<std::cell::RefCell<Option<gtk::gdk::RGBA>>>,
) -> gtk::MenuButton {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let grid = gtk::Grid::new();
    grid.set_row_spacing(2);
    grid.set_column_spacing(2);
    let (btn, popover) = bar_menu(webview, tip, &content);
    btn.set_child(Some(&glyph));
    let weak_pop = popover.downgrade();
    // The one place every way of choosing ends: the button's face, then the
    // command, with the caret put back first.
    let apply: std::rc::Rc<dyn Fn(Option<gtk::gdk::RGBA>)> = {
        let (weak, repaint, chosen) = (webview.downgrade(), repaint.clone(), chosen.clone());
        std::rc::Rc::new(move |color| {
            let value = color
                .as_ref()
                .map_or_else(|| none.to_string(), crate::color::to_hex);
            *chosen.borrow_mut() = color;
            repaint();
            run_restored(
                &weak,
                &format!("window.__hylkiStyled('{cmd}',{})", js_str(&value)),
            );
        })
    };
    let reset = gtk::Button::with_label(&i18n(clear));
    reset.add_css_class("flat");
    {
        let (pop, apply) = (weak_pop.clone(), apply.clone());
        reset.connect_clicked(move |_| {
            if let Some(p) = pop.upgrade() {
                p.popdown();
            }
            apply(None);
        });
    }
    content.append(&reset);
    for (col, hue) in PALETTE.iter().enumerate() {
        for (row, color) in hue.iter().enumerate() {
            let swatch = gtk::Button::new();
            swatch.set_child(Some(&crate::ui::context_menu::swatch_widget(color, true)));
            swatch.add_css_class("flat");
            swatch.set_tooltip_text(Some(color));
            let (pop, apply) = (weak_pop.clone(), apply.clone());
            swatch.connect_clicked(move |_| {
                if let Some(p) = pop.upgrade() {
                    p.popdown();
                }
                apply(gtk::gdk::RGBA::parse(*color).ok());
            });
            grid.attach(&swatch, col as i32, row as i32, 1, 1);
        }
    }
    content.append(&grid);
    let custom = gtk::Button::with_label(&i18n("Custom…"));
    custom.add_css_class("flat");
    {
        let (pop, weak, apply, chosen) = (
            weak_pop.clone(),
            webview.downgrade(),
            apply.clone(),
            chosen.clone(),
        );
        custom.connect_clicked(move |b| {
            if let Some(p) = pop.upgrade() {
                p.popdown();
            }
            let picker = gtk::ColorDialog::new();
            picker.set_with_alpha(false);
            let initial = (*chosen.borrow()).unwrap_or(gtk::gdk::RGBA::new(0.5, 0.5, 0.5, 1.0));
            let parent = b.root().and_downcast::<gtk::Window>();
            let (weak, apply) = (weak.clone(), apply.clone());
            picker.choose_rgba(
                parent.as_ref(),
                Some(&initial),
                None::<&gtk::gio::Cancellable>,
                move |res| match res {
                    Ok(rgba) => apply(Some(rgba)),
                    // Cancelled: the caret goes back where it was all the same.
                    Err(_) => run_restored(&weak, ""),
                },
            );
        });
    }
    content.append(&custom);
    btn
}

/// The face of a color button: an "A" over a bar in the color last chosen
/// (text color), or an "A" on it (highlight). With none chosen, the bar
/// takes the text's own color and the highlight is left out. Returns the
/// face and what repaints it.
pub(super) fn color_glyph(
    highlight: bool,
    chosen: std::rc::Rc<std::cell::RefCell<Option<gtk::gdk::RGBA>>>,
) -> (gtk::Widget, std::rc::Rc<dyn Fn()>) {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    let letter = gtk::Label::new(Some("A"));
    letter.add_css_class("heading");
    let face = gtk::Overlay::new();
    face.set_child(Some(&area));
    face.add_overlay(&letter);
    face.set_valign(gtk::Align::Center);
    if !highlight {
        letter.set_valign(gtk::Align::Start);
        letter.set_margin_top(-2);
    }
    {
        let chosen = chosen.clone();
        area.set_draw_func(move |area, cr, w, h| {
            let fg = area.color();
            let (w, h) = (w as f64, h as f64);
            let pick = *chosen.borrow();
            match (highlight, pick) {
                (true, Some(c)) => {
                    cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), 1.0);
                    cr.rectangle(0.0, 0.0, w, h);
                    let _ = cr.fill();
                }
                (true, None) => {
                    cr.set_source_rgba(fg.red().into(), fg.green().into(), fg.blue().into(), 0.5);
                    cr.set_line_width(1.0);
                    cr.rectangle(0.5, 0.5, w - 1.0, h - 1.0);
                    let _ = cr.stroke();
                }
                (false, c) => {
                    let c = c.unwrap_or(fg);
                    cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), 1.0);
                    cr.rectangle(2.0, h - 3.0, w - 4.0, 3.0);
                    let _ = cr.fill();
                }
            }
        });
    }
    // The letter is dark on a highlight, whatever the theme.
    let repaint: std::rc::Rc<dyn Fn()> = {
        let (area, letter) = (area.clone(), letter.clone());
        std::rc::Rc::new(move || {
            area.queue_draw();
            if highlight && chosen.borrow().is_some() {
                letter.add_css_class("on-highlight");
            } else {
                letter.remove_css_class("on-highlight");
            }
        })
    };
    (face.upcast(), repaint)
}
