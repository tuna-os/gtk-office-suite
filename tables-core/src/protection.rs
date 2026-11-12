// protection.rs — what sheet protection means for selection and display
// (TBL-005).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// One place for the protected sheet's view-side rules, so the formula bar,
// keyboard, mouse, name box, clipboard and editing paths cannot disagree:
//
// - a protected sheet's hidden-formula cells show their calculated value,
//   never their input (formula bar, copy, fill sources);
// - `allow_select_locked`/`allow_select_unlocked` decide which cells the
//   user can select at all, and a cell that cannot be selected cannot be
//   edited either.
//
// Selection is session state: these helpers move it on the `SheetModel`
// directly, never through history. Edits are still authorized by
// `controller/protection.rs`, which calls the same predicates.

use crate::sheet::SheetModel;

impl SheetModel {
    fn cell_lock(&self, row: usize, col: usize) -> crate::sheet::CellProtection {
        self.cell_protections.get(row).and_then(|r| r.get(col)).cloned().unwrap_or_default()
    }

    /// Whether protection hides (`row`, `col`)'s input: the sheet is
    /// protected and the cell's hidden flag is set. Its value still shows.
    pub fn formula_hidden(&self, row: usize, col: usize) -> bool {
        self.protection.protected && self.cell_lock(row, col).hidden_formula
    }

    /// Whether any cell's selectability depends on protection.
    pub fn selection_restricted(&self) -> bool {
        self.protection.protected && !(self.protection.allow_select_locked && self.protection.allow_select_unlocked)
    }

    /// Whether the user may select (`row`, `col`): always on an unprotected
    /// sheet; otherwise as its locked or unlocked selection permission says.
    pub fn may_select(&self, row: usize, col: usize) -> bool {
        if row >= self.rows || col >= self.cols {
            return false;
        }
        if !self.selection_restricted() {
            return true;
        }
        if self.cell_lock(row, col).locked {
            self.protection.allow_select_locked
        } else {
            self.protection.allow_select_unlocked
        }
    }

    /// Whether every cell of the inclusive block may be selected.
    pub fn may_select_block(&self, top: usize, left: usize, bottom: usize, right: usize) -> bool {
        let (top, bottom) = (top.min(bottom), top.max(bottom));
        let (left, right) = (left.min(right), left.max(right));
        if bottom >= self.rows || right >= self.cols {
            return false;
        }
        if !self.selection_restricted() {
            return true;
        }
        (top..=bottom).all(|r| (left..=right).all(|c| self.may_select(r, c)))
    }

    /// Select the single cell (`row`, `col`) if protection allows it.
    /// False — and the selection unchanged — when it does not.
    pub fn try_select_cell(&mut self, row: usize, col: usize) -> bool {
        if !self.may_select(row, col) {
            return false;
        }
        self.select_cell(row, col);
        true
    }

    /// Select the block from the anchor (`row`, `col`) to (`end_row`,
    /// `end_col`) if protection allows every cell in it.
    pub fn try_select_block(&mut self, row: usize, col: usize, end_row: usize, end_col: usize) -> bool {
        if !self.may_select_block(row, col, end_row, end_col) {
            return false;
        }
        self.select_cell(row, col);
        self.extend_selection(end_row, end_col);
        true
    }

    /// Extend the selection's moving end to (`row`, `col`) if protection
    /// allows every cell of the resulting block.
    pub fn try_extend_selection(&mut self, row: usize, col: usize) -> bool {
        let (anchor_row, anchor_col) = (self.selected_row, self.selected_col);
        self.try_select_block(anchor_row, anchor_col, row, col)
    }

    /// One arrow-key step by (`dr`, `dc`): move the active cell, or with
    /// `extend` the selection's moving end. On a sheet whose protection
    /// restricts selection, a move skips cells that cannot be selected
    /// along its line and stays put when none can; an extension that would
    /// take in such a cell does not happen. True when the selection moved.
    pub fn try_step_selection(&mut self, dr: i64, dc: i64, extend: bool) -> bool {
        if self.rows == 0 || self.cols == 0 || (dr == 0 && dc == 0) {
            return false;
        }
        let (row, col) = if extend { (self.sel_end_row, self.sel_end_col) } else { (self.selected_row, self.selected_col) };
        let clamp = |v: i64, n: usize| v.clamp(0, n as i64 - 1) as usize;
        if extend {
            let target = (clamp(row as i64 + dr, self.rows), clamp(col as i64 + dc, self.cols));
            return target != (row, col) && self.try_extend_selection(target.0, target.1);
        }
        let mut k = 1i64;
        loop {
            let (r, c) = (row as i64 + dr * k, col as i64 + dc * k);
            if r < 0 || c < 0 || r >= self.rows as i64 || c >= self.cols as i64 {
                // The edge: an unrestricted step there clamps onto the
                // current cell, as it always has; a restricted one stays.
                return false;
            }
            if self.try_select_cell(r as usize, c as usize) {
                return true;
            }
            k += 1;
        }
    }

    /// After protection changes, make the selection one the user could
    /// have made: unchanged when it already is; else the active cell alone
    /// if that may be selected; else the first selectable cell in reading
    /// order after it, wrapping. False when no cell can be selected (both
    /// selection permissions off): the selection then stays where it is,
    /// and every edit and the formula bar refuse it.
    pub fn settle_selection(&mut self) -> bool {
        let (top, left, bottom, right) = self.selection_block();
        if self.may_select_block(top, left, bottom, right) {
            return true;
        }
        let (row, col) = (self.selected_row, self.selected_col);
        if self.try_select_cell(row, col) {
            return true;
        }
        let cells = self.rows * self.cols;
        let start = row.min(self.rows.saturating_sub(1)) * self.cols + col.min(self.cols.saturating_sub(1));
        (1..cells).map(|i| (start + i) % cells).any(|i| self.try_select_cell(i / self.cols, i % self.cols))
    }
}

#[cfg(test)]
mod tests {
    use crate::sheet::SheetModel;

    /// A protected 4×4 sheet whose column B is unlocked.
    fn sheet(select_locked: bool, select_unlocked: bool) -> SheetModel {
        let mut s = SheetModel::new("S", 4, 4, 1);
        #[allow(clippy::needless_range_loop)]
        for row in 0..4 {
            s.cell_protections[row][1].locked = false;
        }
        s.protection.protected = true;
        s.protection.allow_select_locked = select_locked;
        s.protection.allow_select_unlocked = select_unlocked;
        s
    }

    #[test]
    fn hidden_formulas_hide_only_while_protected() {
        let mut s = sheet(true, true);
        s.cell_protections[0][0].hidden_formula = true;
        assert!(s.formula_hidden(0, 0));
        assert!(!s.formula_hidden(0, 1));
        s.protection.protected = false;
        assert!(!s.formula_hidden(0, 0), "an inactive hidden flag hides nothing");
    }

    #[test]
    fn locked_cells_cannot_be_selected_when_disallowed() {
        let mut s = sheet(false, true);
        s.select_cell(0, 1);
        assert!(!s.try_select_cell(0, 0));
        assert_eq!(s.selection_block(), (0, 1, 0, 1), "a refused click changes nothing");
        assert!(s.try_select_cell(3, 1));
        assert!(!s.try_extend_selection(3, 2), "an extension into a locked cell does not happen");
        assert!(s.try_extend_selection(1, 1));
        assert_eq!(s.selection_block(), (1, 1, 3, 1));
        assert!(!s.try_select_block(0, 0, 1, 1), "a name-box range with a locked cell is refused");
        s.protection.protected = false;
        assert!(s.try_select_cell(0, 0), "unprotected, every cell is selectable");
    }

    #[test]
    fn arrows_skip_cells_that_cannot_be_selected() {
        let mut s = sheet(true, false);
        s.select_cell(0, 0);
        assert!(s.try_step_selection(0, 1, false), "Right skips the unlocked column");
        assert_eq!((s.selected_row, s.selected_col), (0, 2));
        assert!(s.try_step_selection(0, -1, false));
        assert_eq!((s.selected_row, s.selected_col), (0, 0));
        assert!(!s.try_step_selection(0, -1, false), "the edge stays put");
        let mut s = sheet(false, true);
        s.select_cell(0, 1);
        assert!(!s.try_step_selection(0, 1, false), "no unlocked cell to the right: stay");
        assert!(s.try_step_selection(1, 0, false));
        assert_eq!((s.selected_row, s.selected_col), (1, 1));
    }

    #[test]
    fn nothing_moves_when_both_selection_permissions_are_off() {
        let mut s = sheet(false, false);
        s.select_cell(2, 2);
        assert!(!s.may_select(2, 1) && !s.may_select(2, 2));
        assert!(!s.try_step_selection(0, 1, false));
        assert!(!s.try_select_cell(0, 1));
        assert!(!s.settle_selection());
        assert_eq!(s.selection_block(), (2, 2, 2, 2));
    }

    #[test]
    fn settling_moves_a_disallowed_selection_to_the_next_selectable_cell() {
        let mut s = sheet(false, true);
        s.select_cell(2, 2);
        assert!(s.settle_selection());
        assert_eq!(s.selection_block(), (3, 1, 3, 1), "reading order after C3 reaches B4");
        s.select_cell(0, 1);
        s.extend_selection(0, 2);
        assert!(s.settle_selection());
        assert_eq!(s.selection_block(), (0, 1, 0, 1), "a block keeps its selectable active cell");
    }
}
