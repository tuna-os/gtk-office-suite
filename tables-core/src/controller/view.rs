// SPDX-License-Identifier: GPL-3.0-or-later
//! View-level commands: protection, named ranges, filters, row/col visibility,
//! print setup, and sorting.
//! Split out of the former controller.rs (issue #227).

use std::collections::HashSet;

use crate::sheet::{col_label, SortDirection};

use super::state::*;

use super::core::WorkbookController;

impl WorkbookController {
    pub fn set_sheet_protection(&mut self, protected: bool, password_hash: Option<String>) {
        self.mutate_sheet("Toggle Sheet Protection", move |sheet| {
            sheet.protection.protected = protected;
            sheet.protection.password_hash = password_hash;
        });
    }

    pub fn set_cell_locked(&mut self, row: usize, col: usize, locked: bool) {
        self.mutate_sheet("Toggle Cell Lock", move |sheet| {
            sheet.cell_protections[row][col].locked = locked;
        });
    }

    pub fn define_name(
        &mut self,
        name: &str,
        sel: (usize, usize, usize, usize),
    ) -> Result<(), String> {
        let (top, left, bottom, right) = sel;
        let state = self.state.borrow();
        let sheet_name = state.sheet().name.clone();
        let formula = if top == bottom && left == right {
            format!("{}!${}${}", sheet_name, col_label(left), top + 1)
        } else {
            format!(
                "{}!${}${}:${}${}",
                sheet_name,
                col_label(left),
                top + 1,
                col_label(right),
                bottom + 1
            )
        };
        drop(state);
        // Validate before creating an undo entry — an invalid name/formula
        // must never reach the undo stack. is_valid_defined_name takes
        // &mut self (it's read-only in effect, but the upstream signature
        // requires it), so this needs its own borrow_mut, separate from
        // and after the read-only borrow above.
        self.state
            .borrow_mut()
            .engine
            .model
            .is_valid_defined_name(name, None, &formula)?;
        self.execute(Box::new(DefinedNameCommand {
            name: name.to_string(),
            formula,
        }));
        Ok(())
    }

    pub fn filter_by_value(&mut self, col: usize, needle: &str) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let before = sheet.hidden_rows.clone();
        let after: HashSet<usize> = if needle.is_empty() {
            HashSet::new()
        } else {
            let needle_lower = needle.to_lowercase();
            (0..sheet.rows)
                .filter(|&r| {
                    (0..sheet.cols).any(|c| !sheet.cell(r, c).is_empty())
                        && !sheet.cell(r, col).to_lowercase().contains(&needle_lower)
                })
                .collect()
        };
        drop(sheet);
        drop(state);
        if before != after {
            self.execute(Box::new(FilterCommand {
                sheet_id,
                before,
                after,
            }));
        }
    }

    pub fn clear_filter(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().hidden_rows.clone();
        drop(state);
        if !before.is_empty() {
            self.execute(Box::new(FilterCommand {
                sheet_id,
                before,
                after: HashSet::new(),
            }));
        }
    }

    pub fn hide_selected_rows(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let before = sheet.hidden_rows_manual.clone();
        let (r0, _, r1, _) = sheet.selection_rect();
        let mut after = before.clone();
        after.extend(r0..=r1);
        drop(sheet);
        drop(state);
        if before != after {
            self.execute(Box::new(HideRowsCommand {
                sheet_id,
                before,
                after,
            }));
        }
    }

    pub fn unhide_all_rows(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().hidden_rows_manual.clone();
        drop(state);
        if !before.is_empty() {
            self.execute(Box::new(HideRowsCommand {
                sheet_id,
                before,
                after: HashSet::new(),
            }));
        }
    }

    pub fn hide_selected_cols(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let before = sheet.hidden_cols.clone();
        let (_, c0, _, c1) = sheet.selection_rect();
        let mut after = before.clone();
        after.extend(c0..=c1);
        drop(sheet);
        drop(state);
        if before != after {
            self.execute(Box::new(HideColsCommand {
                sheet_id,
                before,
                after,
            }));
        }
    }

    pub fn unhide_all_cols(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().hidden_cols.clone();
        drop(state);
        if !before.is_empty() {
            self.execute(Box::new(HideColsCommand {
                sheet_id,
                before,
                after: HashSet::new(),
            }));
        }
    }

    pub fn set_print_area(&mut self, sel: (usize, usize, usize, usize)) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().print_area;
        drop(state);
        let after = Some(sel);
        if before != after {
            self.execute(Box::new(PrintAreaCommand {
                sheet_id,
                before,
                after,
            }));
        }
    }

    pub fn clear_print_area(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().print_area;
        drop(state);
        if before.is_some() {
            self.execute(Box::new(PrintAreaCommand {
                sheet_id,
                before,
                after: None,
            }));
        }
    }

    pub fn set_page_setup(&mut self, setup: suite_common_core::print::PageSetup) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().page_setup.clone();
        drop(state);
        if before != setup {
            self.execute(Box::new(PageSetupCommand {
                sheet_id,
                before,
                after: setup,
            }));
        }
    }

    pub fn toggle_sort(&mut self, col: usize) {
        use SortDirection::{Ascending, Descending};

        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before_sheet = state.sheet().clone();
        if col >= before_sheet.cols {
            return;
        }
        let new_direction = match before_sheet.sorted_col {
            Some((current, Ascending)) if current == col => Descending,
            Some((current, Descending)) if current == col => {
                drop(state);
                self.mutate_sheet("Clear Sort Indicator", |sheet| sheet.sorted_col = None);
                return;
            }
            _ => Ascending,
        };
        let before_inputs: Vec<Vec<String>> = (0..before_sheet.rows)
            .map(|row| {
                (0..before_sheet.cols)
                    .map(|column| state.cell_input(row, column))
                    .collect()
            })
            .collect();
        // Numeric compare when both cells parse as numbers; otherwise a
        // case-insensitive string compare. The previous version always
        // parsed-with-fallback-to-0.0, so any non-numeric column (the
        // common case — sorting names, categories, anything text) had
        // every row compare equal and silently never actually reorder,
        // even though sorted_col (and the header arrow) still updated
        // as if it had. Caught via #113's visual-validation screenshots
        // (a text-column sort test) showing unsorted data despite a
        // passing sorted_col assertion — a real bug, not a rendering gap.
        //
        // Blank cells always sort last regardless of direction (Excel/
        // Sheets convention) — without this, sorting a lightly-used
        // sheet pushes real data to the bottom on ascending sort, since
        // "" < any non-empty string. Caught the same way: a GUI test on
        // the app's real 100-row default sheet (only 3 rows filled)
        // found the 3 values sorted correctly among themselves but
        // shoved to rows 97-99 by the 97 blank rows sorting ahead of
        // them.
        let mut order: Vec<usize> = (0..before_sheet.rows).collect();
        order.sort_by(|&left, &right| {
            let left_text = &before_sheet.data[left][col];
            let right_text = &before_sheet.data[right][col];
            let left_blank = left_text.trim().is_empty();
            let right_blank = right_text.trim().is_empty();
            match (left_blank, right_blank) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => {
                    let cmp = match (left_text.parse::<f64>(), right_text.parse::<f64>()) {
                        (Ok(l), Ok(r)) => l.partial_cmp(&r).unwrap_or(std::cmp::Ordering::Equal),
                        _ => left_text.to_lowercase().cmp(&right_text.to_lowercase()),
                    };
                    if new_direction == Ascending {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                }
            }
        });
        drop(state);

        let mut after_inputs = vec![vec![String::new(); before_sheet.cols]; before_sheet.rows];
        {
            let mut state = self.state.borrow_mut();
            for (new_row, &old_row) in order.iter().enumerate() {
                for column in 0..before_sheet.cols {
                    after_inputs[new_row][column] = state.engine.move_input(
                        &before_inputs[old_row][column],
                        (old_row, column),
                        (new_row, column),
                    );
                }
            }
        }
        let mut after_sheet = before_sheet.clone();
        after_sheet.sorted_col = Some((col, new_direction));
        after_sheet.formats = order
            .iter()
            .map(|&row| before_sheet.formats[row].clone())
            .collect();
        after_sheet.borders = order
            .iter()
            .map(|&row| before_sheet.borders[row].clone())
            .collect();
        after_sheet.validations = order
            .iter()
            .map(|&row| before_sheet.validations[row].clone())
            .collect();

        self.execute(Box::new(SortCommand {
            sheet_id,
            before_inputs,
            after_inputs,
            before_sheet,
            after_sheet,
        }));
    }
}
