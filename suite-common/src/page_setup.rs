//! Page Setup as a libadwaita dialog: paper size, orientation and margins
//! as rows, Cancel and Apply in its header bar.
//!
//! Letters used GtkPageSetupUnixDialog, a GTK 3-era dialog with no
//! libadwaita styling: a separate window with its own title bar, a printer
//! chooser a page setup has no use for, and a margins view behind a further
//! "Manage custom sizes" dialog.

use crate::i18n;
use gtk4::{self as gtk, prelude::*};
use libadwaita::{self as adw, prelude::*};

/// Points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// The paper sizes offered, portrait (name, width pt, height pt).
pub const PAPERS: [(&str, f64, f64); 5] = [
    ("A4", 595.28, 841.89),
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("A5", 419.53, 595.28),
    ("A3", 841.89, 1190.55),
];

/// A page: size and margins, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    pub width_pt: f64,
    pub height_pt: f64,
    pub margin_top_pt: f64,
    pub margin_bottom_pt: f64,
    pub margin_left_pt: f64,
    pub margin_right_pt: f64,
}

/// The index in `PAPERS` of the size closest to `page` (either way round),
/// and whether it is landscape.
pub fn paper_of(page: &Page) -> (usize, bool) {
    let landscape = page.width_pt > page.height_pt;
    let (short, long) = if landscape { (page.height_pt, page.width_pt) } else { (page.width_pt, page.height_pt) };
    let index = PAPERS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let d = |p: &(&str, f64, f64)| (p.1 - short).abs() + (p.2 - long).abs();
            d(a).total_cmp(&d(b))
        })
        .map(|(i, _)| i)
        .unwrap_or(0);
    (index, landscape)
}

/// `page` resized to paper `index` in the given orientation, margins kept.
pub fn with_paper(page: Page, index: usize, landscape: bool) -> Page {
    let (_, w, h) = PAPERS[index.min(PAPERS.len() - 1)];
    let (width_pt, height_pt) = if landscape { (h, w) } else { (w, h) };
    Page { width_pt, height_pt, ..page }
}

/// Show Page Setup over `parent`, starting from `current`; `apply` gets the
/// chosen page when Apply is pressed. Cancel, Escape or closing changes
/// nothing.
pub fn show(parent: &impl IsA<gtk::Widget>, current: Page, apply: impl Fn(Page) + 'static) {
    let (paper, landscape) = paper_of(&current);
    let papers = gtk::StringList::new(&PAPERS.map(|p| p.0));
    let paper_row = adw::ComboRow::builder().title(i18n("Paper size")).model(&papers).selected(paper as u32).build();
    let orientations = gtk::StringList::new(&[&i18n("Portrait"), &i18n("Landscape")]);
    let orientation_row = adw::ComboRow::builder()
        .title(i18n("Orientation"))
        .model(&orientations)
        .selected(u32::from(landscape))
        .build();
    let size = adw::PreferencesGroup::new();
    size.add(&paper_row);
    size.add(&orientation_row);

    let margin = |title: String, pt: f64| {
        adw::SpinRow::builder()
            .title(title)
            .subtitle(i18n("Millimetres"))
            .digits(1)
            .adjustment(&gtk::Adjustment::new((pt / PT_PER_MM * 10.0).round() / 10.0, 0.0, 100.0, 1.0, 5.0, 0.0))
            .build()
    };
    let top = margin(i18n("Top"), current.margin_top_pt);
    let bottom = margin(i18n("Bottom"), current.margin_bottom_pt);
    let left = margin(i18n("Left"), current.margin_left_pt);
    let right = margin(i18n("Right"), current.margin_right_pt);
    let margins = adw::PreferencesGroup::builder().title(i18n("Margins")).build();
    for row in [&top, &bottom, &left, &right] {
        margins.add(row);
    }

    let crate::dialogs::ActionDialog { dialog, action: apply_button } = crate::dialogs::action_dialog(
        &i18n("Page Setup"),
        &i18n("_Apply"),
        400,
        &crate::dialogs::form_body(&[size.upcast_ref(), margins.upcast_ref()]),
    );

    {
        let d = dialog.clone();
        apply_button.connect_clicked(move |_| {
            let mm = |row: &adw::SpinRow| row.value() * PT_PER_MM;
            let page = Page {
                margin_top_pt: mm(&top),
                margin_bottom_pt: mm(&bottom),
                margin_left_pt: mm(&left),
                margin_right_pt: mm(&right),
                ..current
            };
            apply(with_paper(page, paper_row.selected() as usize, orientation_row.selected() == 1));
            d.close();
        });
    }
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(w: f64, h: f64) -> Page {
        Page { width_pt: w, height_pt: h, margin_top_pt: 72.0, margin_bottom_pt: 72.0, margin_left_pt: 36.0, margin_right_pt: 36.0 }
    }

    /// A page is matched to its paper either way round, and a landscape
    /// one is reported as landscape.
    #[test]
    fn a_page_is_recognised_as_its_paper_and_orientation() {
        assert_eq!(paper_of(&page(612.0, 792.0)), (1, false));
        assert_eq!(paper_of(&page(792.0, 612.0)), (1, true));
        assert_eq!(paper_of(&page(595.3, 841.9)), (0, false));
    }

    /// Choosing a paper and orientation sets the size and keeps the margins.
    #[test]
    fn a_chosen_paper_turns_for_landscape_and_keeps_the_margins() {
        let p = with_paper(page(612.0, 792.0), 0, true);
        assert_eq!((p.width_pt, p.height_pt), (841.89, 595.28));
        assert_eq!((p.margin_left_pt, p.margin_top_pt), (36.0, 72.0));
    }
}
