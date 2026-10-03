//! Window dialogs — format cells, conditional format, define name, page
//! setup, and filter dialogs for the spreadsheet window.
//!
//! Extracted from `window.rs` (gtk-office-suite#168): self-contained modal
//! dialogs that only need the workbook controller and drawing area; the
//! main window implementation stays in `window.rs`.

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use suite_common::format::{NumberFormat, NumberFormatKind};
use tables_core::controller::WorkbookController;

/// Format Cells dialog: number-format kind + decimals + currency
/// symbol, applied to the whole selection.
pub(crate) fn show_format_cells_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    da: &gtk4::DrawingArea,
    refresh: &Rc<dyn Fn()>,
    parent: Option<adw::ApplicationWindow>,
) {
    let kinds = ["General", "Number", "Currency", "Percent", "Date", "Scientific"];
    let dropdown = gtk4::DropDown::from_strings(&kinds);
    dropdown.update_property(&[gtk4::accessible::Property::Label("Format kind")]);
    // Preselect from the active cell's current format.
    {
        let state = controller.borrow().state.clone();
        let st = state.borrow();
        let sh = st.sheet();
        let idx = match sh.formats[sh.selected_row][sh.selected_col].kind {
            NumberFormatKind::General | NumberFormatKind::Text => 0,
            NumberFormatKind::Number(_) => 1,
            NumberFormatKind::Currency(_, _) => 2,
            NumberFormatKind::Percent(_) => 3,
            NumberFormatKind::Date(_) | NumberFormatKind::DateTime(_) => 4,
            NumberFormatKind::Scientific(_) => 5,
            // No dropdown entry of its own yet; "Number" is the nearest.
            NumberFormatKind::Fraction(_) => 1,
            // A code of its own: the inspector's Number group edits it.
            NumberFormatKind::Custom(_) => 0,
        };
        dropdown.set_selected(idx);
    }

    let decimals = gtk4::SpinButton::with_range(0.0, 6.0, 1.0);
    decimals.set_value(2.0);
    decimals.update_property(&[gtk4::accessible::Property::Label("Decimal places")]);
    let symbol = gtk4::Entry::new();
    symbol.set_text("$");
    symbol.set_max_width_chars(4);
    symbol.update_property(&[gtk4::accessible::Property::Label("Currency symbol")]);

    let grid = suite_common::dialogs::form_rows(&[
        (&suite_common::i18n("Format"), dropdown.upcast_ref()),
        (&suite_common::i18n("Decimals"), decimals.upcast_ref()),
        (&suite_common::i18n("Symbol"), symbol.upcast_ref()),
    ]);
    let apply = gtk4::Button::with_label("Apply");
    apply.add_css_class("suggested-action");

    let dialog = adw::Dialog::builder()
        .title("Format Cells")
        .content_width(320)
        .build();
    suite_common::dialogs::form_dialog(&dialog, &grid, &apply);

    {
        let ctl = controller.clone();
        let da = da.clone();
        let refresh = refresh.clone();
        let dialog = dialog.clone();
        let dropdown = dropdown.clone();
        apply.connect_clicked(move |_| {
            let dp = decimals.value() as u8;
            let sym = symbol.text().to_string();
            let kind = match dropdown.selected() {
                1 => NumberFormatKind::Number(dp),
                2 => NumberFormatKind::Currency(sym, dp),
                3 => NumberFormatKind::Percent(dp),
                4 => NumberFormatKind::Date("%Y-%m-%d".into()),
                5 => NumberFormatKind::Scientific(dp),
                _ => NumberFormatKind::General,
            };
            ctl.borrow_mut().mutate_sheet("Format Cells", move |sh| {
                let (r0, c0, r1, c1) = sh.selection_rect();
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        sh.formats[r][c] = NumberFormat::new(kind.clone());
                    }
                }
            });
            refresh();
            da.queue_draw();
            dialog.close();
        });
    }
    dialog.present(parent.as_ref());
}

/// Conditional Formatting dialog: operator + threshold(s) + fill color,
/// applied to the current selection (ADR 0003 §4 — cell-value rules).
pub(crate) fn show_conditional_format_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    da: &gtk4::DrawingArea,
    parent: Option<&adw::ApplicationWindow>,
) {
    use tables_core::sheet::{CondOp, CondRule};
    let dialog = adw::Dialog::builder()
        .title(suite_common::i18n("Conditional Formatting"))
        .content_width(440)
        .build();

    let op_combo = gtk::DropDown::from_strings(&["Greater than", "Less than", "Equal to", "Between"]);
    let value_entry = gtk::Entry::builder().placeholder_text("Value").build();
    let value2_entry = gtk::Entry::builder().placeholder_text("Upper bound").build();
    value2_entry.set_sensitive(false);
    {
        let v2 = value2_entry.clone();
        op_combo.connect_selected_notify(move |dd| v2.set_sensitive(dd.selected() == 3));
    }
    let color_btn = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    color_btn.set_rgba(&gtk4::gdk::RGBA::new(1.0, 0.75, 0.75, 1.0));

    let grid = suite_common::dialogs::form_rows(&[
        ("Condition", op_combo.upcast_ref()),
        ("Value", value_entry.upcast_ref()),
        ("And", value2_entry.upcast_ref()),
        ("Fill", color_btn.upcast_ref()),
    ]);

    let apply = gtk::Button::with_label(&suite_common::i18n("Apply"));
    apply.add_css_class("suggested-action");

    {
        let ctl = controller.clone();
        let da = da.clone();
        let dlg = dialog.clone();
        let op_combo = op_combo.clone();
        let value_entry = value_entry.clone();
        let value2_entry = value2_entry.clone();
        let color_btn = color_btn.clone();
        apply.connect_clicked(move |_| {
            let Ok(value) = value_entry.text().trim().parse::<f64>() else { return };
            let value2 = value2_entry.text().trim().parse::<f64>().unwrap_or(value);
            let op = match op_combo.selected() {
                0 => CondOp::Greater,
                1 => CondOp::Less,
                2 => CondOp::Equal,
                _ => CondOp::Between,
            };
            let rgba = color_btn.rgba();
            let fill = format!(
                "{:02X}{:02X}{:02X}",
                (rgba.red() * 255.0) as u8,
                (rgba.green() * 255.0) as u8,
                (rgba.blue() * 255.0) as u8
            );
            ctl.borrow_mut().mutate_sheet("Conditional Formatting", move |sheet| {
                let (r0, c0, r1, c1) = sheet.selection_rect();
                sheet.cond_rules.push(CondRule {
                    range: (r0, c0, r1, c1),
                    op,
                    value,
                    value2,
                    fill,
                });
            });
            da.queue_draw();
            dlg.close();
        });
    }

    suite_common::dialogs::form_dialog(&dialog, &grid, &apply);
    dialog.present(parent);
}

/// Define a workbook-scoped named range covering the current selection
/// (#113). Jump back to a defined name via the name box (typing its
/// name, not just a cell reference — see the name box's connect_activate
/// handler) rather than a separate management UI; deleting a name is
/// deferred until there's a concrete need for it.
pub(crate) fn show_define_name_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    parent: Option<&adw::ApplicationWindow>,
) {
    let sel = controller.borrow().state.borrow().sheet().selection_rect();

    let dialog = adw::Dialog::builder()
        .title(suite_common::i18n("Define Name"))
        .content_width(320)
        .build();

    let name_entry = gtk::Entry::builder().placeholder_text("e.g. TaxRate").build();
    name_entry.update_property(&[gtk4::accessible::Property::Label("Name")]);
    let error_label = gtk::Label::new(None);
    error_label.add_css_class("error");
    error_label.set_halign(gtk::Align::Start);
    let grid = suite_common::dialogs::form_rows(&[("Name", name_entry.upcast_ref())]);
    grid.append(&error_label);

    let apply = gtk::Button::with_label(&suite_common::i18n("Define"));
    apply.add_css_class("suggested-action");

    {
        let ctl = controller.clone();
        let dlg = dialog.clone();
        let name_entry = name_entry.clone();
        let error_label = error_label.clone();
        apply.connect_clicked(move |_| {
            let name = name_entry.text().to_string();
            match ctl.borrow_mut().define_name(&name, sel) {
                Ok(()) => { dlg.close(); }
                Err(e) => error_label.set_text(&e),
            }
        });
    }

    suite_common::dialogs::form_dialog(&dialog, &grid, &apply);
    dialog.present(parent);
}

/// Page setup for PDF export (#113): the suite's Page Setup dialog, the one
/// Letters uses, with a margin for each side. It used to be a bare grid in
/// a dialog with no title and no Cancel, and one margin for all four sides.
pub(crate) fn show_page_setup_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    parent: Option<&adw::ApplicationWindow>,
) {
    let Some(parent) = parent else { return };
    let current = controller.borrow().state.borrow().sheet().page_setup.clone();
    let ctl = controller.clone();
    suite_common::page_setup::show(parent, page_of(&current), move |page| {
        ctl.borrow_mut().set_page_setup(setup_of(&page, current.scale));
    });
}

const PT_PER_MM: f64 = 72.0 / 25.4;

/// A sheet's page setup as the dialog's page, in points.
fn page_of(setup: &suite_common::print::PageSetup) -> suite_common::page_setup::Page {
    use suite_common::print::Orientation;
    let (w, h) = setup.size.dimensions_mm();
    let (w, h) = if setup.orientation == Orientation::Landscape { (h, w) } else { (w, h) };
    suite_common::page_setup::Page {
        width_pt: w * PT_PER_MM,
        height_pt: h * PT_PER_MM,
        margin_top_pt: setup.margin_top_mm * PT_PER_MM,
        margin_bottom_pt: setup.margin_bottom_mm * PT_PER_MM,
        margin_left_pt: setup.margin_left_mm * PT_PER_MM,
        margin_right_pt: setup.margin_right_mm * PT_PER_MM,
    }
}

/// The dialog's page as a sheet's page setup: a named size when it is one,
/// turned for its orientation.
fn setup_of(page: &suite_common::page_setup::Page, scale: f64) -> suite_common::print::PageSetup {
    use suite_common::print::{Orientation, PageSetup, PageSize};
    let landscape = page.width_pt > page.height_pt;
    let (short, long) = if landscape { (page.height_pt, page.width_pt) } else { (page.width_pt, page.height_pt) };
    let (short, long) = (short / PT_PER_MM, long / PT_PER_MM);
    let size = [PageSize::A4, PageSize::A3, PageSize::Letter, PageSize::Legal]
        .into_iter()
        .find(|s| {
            let (w, h) = s.dimensions_mm();
            (w - short).abs() < 1.0 && (h - long).abs() < 1.0
        })
        .unwrap_or(PageSize::Custom { width_mm: (short * 10.0).round() / 10.0, height_mm: (long * 10.0).round() / 10.0 });
    let mm = |pt: f64| (pt / PT_PER_MM * 10.0).round() / 10.0;
    PageSetup {
        size,
        orientation: if landscape { Orientation::Landscape } else { Orientation::Portrait },
        margin_top_mm: mm(page.margin_top_pt),
        margin_bottom_mm: mm(page.margin_bottom_pt),
        margin_left_mm: mm(page.margin_left_pt),
        margin_right_mm: mm(page.margin_right_pt),
        scale,
    }
}

/// Filter rows by a substring match against the currently selected
/// column (#113). Hiding non-matching rows, not deleting them — see
/// `WorkbookController::filter_by_value`.
pub(crate) fn show_filter_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    da: &gtk4::DrawingArea,
    parent: Option<&adw::ApplicationWindow>,
) {
    let col = controller.borrow().state.borrow().sheet().selected_col;
    let col_label = tables_core::sheet::col_label(col);

    let dialog = adw::Dialog::builder()
        .title(suite_common::i18n("Filter by Column"))
        .content_width(400)
        .build();

    let value_entry = gtk::Entry::builder()
        .placeholder_text("Value contains…")
        .build();
    value_entry.update_property(&[gtk4::accessible::Property::Label("Filter value")]);
    let grid = suite_common::dialogs::form_rows(&[(&format!("Column {col_label}"), value_entry.upcast_ref())]);

    let apply = gtk::Button::with_label(&suite_common::i18n("Filter"));
    apply.add_css_class("suggested-action");

    {
        let ctl = controller.clone();
        let da = da.clone();
        let dlg = dialog.clone();
        let value_entry = value_entry.clone();
        apply.connect_clicked(move |_| {
            let needle = value_entry.text().to_string();
            ctl.borrow_mut().filter_by_value(col, &needle);
            da.queue_draw();
            dlg.close();
        });
    }

    suite_common::dialogs::form_dialog(&dialog, &grid, &apply);
    dialog.present(parent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::print::{Orientation, PageSetup, PageSize};

    /// A sheet's page setup goes through the shared dialog's page and back
    /// unchanged: named sizes stay named, landscape stays landscape, and
    /// each side keeps its own margin.
    #[test]
    fn a_page_setup_round_trips_through_the_dialogs_page() {
        for (size, orientation) in [(PageSize::A4, Orientation::Portrait), (PageSize::Letter, Orientation::Landscape), (PageSize::Legal, Orientation::Portrait)] {
            let setup = PageSetup { size, orientation, margin_top_mm: 20.0, margin_bottom_mm: 15.0, margin_left_mm: 10.0, margin_right_mm: 12.5, scale: 0.9 };
            assert_eq!(setup_of(&page_of(&setup), 0.9), setup);
        }
        let a5 = suite_common::page_setup::Page { width_pt: 419.53, height_pt: 595.28, margin_top_pt: 72.0, margin_bottom_pt: 72.0, margin_left_pt: 72.0, margin_right_pt: 72.0 };
        assert!(matches!(setup_of(&a5, 1.0).size, PageSize::Custom { .. }), "a size with no name is kept as a custom one");
    }
}
