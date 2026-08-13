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

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    let mut row = 0;
    for (label, widget) in [
        (&suite_common::i18n("Format"), dropdown.clone().upcast::<gtk4::Widget>()),
        (&suite_common::i18n("Decimals"), decimals.clone().upcast()),
        (&suite_common::i18n("Symbol"), symbol.clone().upcast()),
    ] {
        let l = gtk4::Label::new(Some(label));
        l.add_css_class("dim-label");
        l.set_halign(gtk4::Align::Start);
        grid.attach(&l, 0, row, 1, 1);
        grid.attach(&widget, 1, row, 1, 1);
        row += 1;
    }
    let apply = gtk4::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    grid.attach(&apply, 1, row, 1, 1);

    let dialog = adw::Dialog::builder()
        .title("Format Cells")
        .content_width(320)
        .build();
    dialog.set_child(Some(&grid));

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
        .content_width(360)
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

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let lbl = |t: &str| {
        let l = gtk::Label::new(Some(t));
        l.set_halign(gtk::Align::Start);
        l
    };
    grid.attach(&lbl("Condition"), 0, 0, 1, 1);
    grid.attach(&op_combo, 1, 0, 1, 1);
    grid.attach(&lbl("Value"), 0, 1, 1, 1);
    grid.attach(&value_entry, 1, 1, 1, 1);
    grid.attach(&lbl("And"), 0, 2, 1, 1);
    grid.attach(&value2_entry, 1, 2, 1, 1);
    grid.attach(&lbl("Fill"), 0, 3, 1, 1);
    grid.attach(&color_btn, 1, 3, 1, 1);

    let apply = gtk::Button::with_label(&suite_common::i18n("Apply to Selection"));
    apply.add_css_class("suggested-action");
    grid.attach(&apply, 1, 4, 1, 1);

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

    dialog.set_child(Some(&grid));
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

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let lbl = |t: &str| {
        let l = gtk::Label::new(Some(t));
        l.set_halign(gtk::Align::Start);
        l
    };
    let name_entry = gtk::Entry::builder().placeholder_text("e.g. TaxRate").build();
    name_entry.update_property(&[gtk4::accessible::Property::Label("Name")]);
    let error_label = gtk::Label::new(None);
    error_label.add_css_class("error");
    error_label.set_halign(gtk::Align::Start);
    grid.attach(&lbl("Name"), 0, 0, 1, 1);
    grid.attach(&name_entry, 1, 0, 1, 1);
    grid.attach(&error_label, 0, 1, 2, 1);

    let apply = gtk::Button::with_label(&suite_common::i18n("Define"));
    apply.add_css_class("suggested-action");
    grid.attach(&apply, 1, 2, 1, 1);

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

    dialog.set_child(Some(&grid));
    dialog.present(parent);
}

/// Page setup for PDF export (#113): paper size, orientation, and a
/// single uniform margin (real apps allow per-side margins; this keeps
/// the dialog to one control per concern for a first slice).
pub(crate) fn show_page_setup_dialog(
    controller: &Rc<RefCell<WorkbookController>>,
    parent: Option<&adw::ApplicationWindow>,
) {
    use suite_common::print::{Orientation, PageSetup, PageSize};

    let current = controller.borrow().state.borrow().sheet().page_setup.clone();

    let dialog = adw::Dialog::builder()
        .title(suite_common::i18n("Page Setup"))
        .content_width(320)
        .build();

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let lbl = |t: &str| {
        let l = gtk::Label::new(Some(t));
        l.set_halign(gtk::Align::Start);
        l
    };

    let size_names = ["A4", "A3", "Letter", "Legal"];
    let size_combo = gtk::DropDown::from_strings(&size_names);
    let size_index = match current.size {
        PageSize::A4 => 0,
        PageSize::A3 => 1,
        PageSize::Letter => 2,
        PageSize::Legal => 3,
        PageSize::Custom { .. } => 0,
    };
    size_combo.set_selected(size_index);

    let orientation_combo = gtk::DropDown::from_strings(&["Portrait", "Landscape"]);
    orientation_combo.set_selected(if current.orientation == Orientation::Landscape { 1 } else { 0 });

    let margin_entry = gtk::Entry::builder().text(current.margin_top_mm.to_string()).build();
    margin_entry.update_property(&[gtk4::accessible::Property::Label("Margin (mm)")]);

    grid.attach(&lbl("Paper size"), 0, 0, 1, 1);
    grid.attach(&size_combo, 1, 0, 1, 1);
    grid.attach(&lbl("Orientation"), 0, 1, 1, 1);
    grid.attach(&orientation_combo, 1, 1, 1, 1);
    grid.attach(&lbl("Margin (mm)"), 0, 2, 1, 1);
    grid.attach(&margin_entry, 1, 2, 1, 1);

    let apply = gtk::Button::with_label(&suite_common::i18n("Apply"));
    apply.add_css_class("suggested-action");
    grid.attach(&apply, 1, 3, 1, 1);

    {
        let ctl = controller.clone();
        let dlg = dialog.clone();
        apply.connect_clicked(move |_| {
            let size = match size_combo.selected() {
                1 => PageSize::A3,
                2 => PageSize::Letter,
                3 => PageSize::Legal,
                _ => PageSize::A4,
            };
            let orientation =
                if orientation_combo.selected() == 1 { Orientation::Landscape } else { Orientation::Portrait };
            let margin = margin_entry.text().trim().parse::<f64>().unwrap_or(25.4).max(0.0);
            ctl.borrow_mut().set_page_setup(PageSetup {
                size,
                orientation,
                margin_top_mm: margin,
                margin_bottom_mm: margin,
                margin_left_mm: margin,
                margin_right_mm: margin,
                scale: 1.0,
            });
            dlg.close();
        });
    }

    dialog.set_child(Some(&grid));
    dialog.present(parent);
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
        .content_width(320)
        .build();

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(8);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let lbl = |t: &str| {
        let l = gtk::Label::new(Some(t));
        l.set_halign(gtk::Align::Start);
        l
    };
    let value_entry = gtk::Entry::builder()
        .placeholder_text("Value contains…")
        .build();
    value_entry.update_property(&[gtk4::accessible::Property::Label("Filter value")]);
    grid.attach(&lbl(&format!("Column {col_label}")), 0, 0, 1, 1);
    grid.attach(&value_entry, 1, 0, 1, 1);

    let apply = gtk::Button::with_label(&suite_common::i18n("Filter"));
    apply.add_css_class("suggested-action");
    grid.attach(&apply, 1, 1, 1, 1);

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

    dialog.set_child(Some(&grid));
    dialog.present(parent);
}
