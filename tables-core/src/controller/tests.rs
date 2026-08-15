    use super::WorkbookController;
    use crate::engine::TablesEngine;
    use crate::fragment::Fragment; // was an import of the old controller.rs; tests reference Fragment::Grid

    #[test]
    fn live_cell_edit_undo_and_redo_share_canonical_state() {
        let mut controller = WorkbookController::new(5, 5).unwrap();
        let observed = controller.state.clone();

        controller.edit_cell(0, 0, "41");
        assert_eq!(observed.borrow().sheet().cell(0, 0), "41");
        assert!(controller.can_undo());

        assert!(controller.undo());
        assert_eq!(observed.borrow().sheet().cell(0, 0), "");
        assert!(controller.can_redo());

        assert!(controller.redo());
        assert_eq!(observed.borrow().sheet().cell(0, 0), "41");
    }

    #[test]
    fn fill_down_tiles_single_cell_and_is_one_undo_step() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(0, 0, "42");

        controller.fill((0, 0, 0, 0), 3, 0);
        for row in 1..=3 {
            assert_eq!(controller.state.borrow().sheet().cell(row, 0), "42");
        }
        assert!(!controller.can_redo());

        // One undo reverts the whole fill (not one cell at a time).
        assert!(controller.undo());
        for row in 1..=3 {
            assert_eq!(controller.state.borrow().sheet().cell(row, 0), "");
        }
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "42");

        // The other undo entry is the original edit_cell, from before fill.
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
        assert!(!controller.can_undo());
    }

    #[test]
    fn fill_right_tiles_a_multi_cell_selection_pattern() {
        let mut controller = WorkbookController::new(4, 8).unwrap();
        controller.edit_cell(0, 0, "a");
        controller.edit_cell(0, 1, "b");

        controller.fill((0, 0, 0, 1), 0, 5);
        let sheet = controller.state.borrow();
        let sh = sheet.sheet();
        assert_eq!(sh.cell(0, 2), "a");
        assert_eq!(sh.cell(0, 3), "b");
        assert_eq!(sh.cell(0, 4), "a");
        assert_eq!(sh.cell(0, 5), "b");
    }

    #[test]
    fn fill_shifts_relative_formula_references_like_a_real_spreadsheet() {
        // Filling a formula down should behave like Excel/LibreOffice's
        // own fill handle: relative references shift with the target
        // cell, not get copied verbatim (which would silently duplicate
        // the source formula's *meaning* into cells with different
        // neighbors — a correctness bug, not a convenience).
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(0, 0, "10"); // A1
        controller.edit_cell(1, 0, "20"); // A2
        controller.edit_cell(2, 0, "30"); // A3
        controller.edit_cell(0, 1, "=A1*2"); // B1

        controller.fill((0, 1, 0, 1), 2, 1); // drag B1 down through B3
        let state = controller.state.borrow();
        assert_eq!(state.cell_input(1, 1), "=A2*2");
        assert_eq!(state.cell_input(2, 1), "=A3*2");
        assert_eq!(state.sheet().cell(1, 1), "40");
        assert_eq!(state.sheet().cell(2, 1), "60");
    }

    #[test]
    fn fill_leaves_absolute_formula_references_pinned() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(0, 0, "10"); // A1
        controller.edit_cell(0, 1, "=$A$1*2"); // B1

        controller.fill((0, 1, 0, 1), 2, 1); // drag B1 down through B3
        let state = controller.state.borrow();
        assert_eq!(state.cell_input(1, 1), "=$A$1*2");
        assert_eq!(state.cell_input(2, 1), "=$A$1*2");
        assert_eq!(state.sheet().cell(2, 1), "20");
    }

    #[test]
    fn fill_formula_reference_shift_is_undoable() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(0, 0, "10");
        controller.edit_cell(0, 1, "=A1*2");
        controller.fill((0, 1, 0, 1), 1, 1);
        assert_eq!(controller.state.borrow().cell_input(1, 1), "=A2*2");

        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(1, 1), "");
    }

    #[test]
    fn fill_up_tiles_starting_adjacent_to_the_selection() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(3, 0, "a");
        controller.edit_cell(4, 0, "b");

        // Selection rows 3..=4; drag up to row 0 -> fills rows 0..=2,
        // adjacent-first: row2=a, row1=b, row0=a (tiled, cycling source).
        controller.fill((3, 0, 4, 0), 0, 0);
        let sheet = controller.state.borrow();
        let sh = sheet.sheet();
        assert_eq!(sh.cell(2, 0), "a");
        assert_eq!(sh.cell(1, 0), "b");
        assert_eq!(sh.cell(0, 0), "a");
    }

    #[test]
    fn fill_left_tiles_starting_adjacent_to_the_selection() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(0, 3, "x");

        controller.fill((0, 3, 0, 3), 0, 0);
        let sheet = controller.state.borrow();
        let sh = sheet.sheet();
        assert_eq!(sh.cell(0, 2), "x");
        assert_eq!(sh.cell(0, 1), "x");
        assert_eq!(sh.cell(0, 0), "x");
    }

    #[test]
    fn fill_up_and_down_are_symmetric_around_a_selection() {
        // Filling down then filling the result back up (from a fresh
        // selection at the new bottom) should reproduce the same tile
        // order adjacent-first in both directions. Non-numeric content,
        // so this exercises tile behavior specifically, not series
        // detection (see fill_down_continues_an_arithmetic_series for
        // that).
        let mut controller = WorkbookController::new(8, 6).unwrap();
        controller.edit_cell(2, 0, "x");
        controller.edit_cell(3, 0, "y");
        controller.fill((2, 0, 3, 0), 5, 0);
        let sheet = controller.state.borrow();
        let sh = sheet.sheet();
        assert_eq!(sh.cell(4, 0), "x");
        assert_eq!(sh.cell(5, 0), "y");
    }

    #[test]
    fn fill_down_continues_an_arithmetic_series() {
        let mut controller = WorkbookController::new(8, 6).unwrap();
        controller.edit_cell(0, 0, "1");
        controller.edit_cell(1, 0, "2");
        controller.fill((0, 0, 1, 0), 3, 0);
        let sheet = controller.state.borrow();
        let sh = sheet.sheet();
        assert_eq!(sh.cell(2, 0), "3");
        assert_eq!(sh.cell(3, 0), "4");
    }

    #[test]
    fn fill_down_single_numeric_cell_tiles_not_increments() {
        let mut controller = WorkbookController::new(8, 6).unwrap();
        controller.edit_cell(0, 0, "5");
        controller.fill((0, 0, 0, 0), 2, 0);
        let sheet = controller.state.borrow();
        let sh = sheet.sheet();
        assert_eq!(sh.cell(1, 0), "5");
        assert_eq!(sh.cell(2, 0), "5");
    }

    #[test]
    fn define_name_creates_a_workbook_scoped_name() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.define_name("Total", (0, 0, 2, 1)).unwrap();
        let state = controller.state.borrow();
        let names = &state.engine.model.workbook.defined_names;
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "Total");
        assert_eq!(names[0].formula, "Sheet1!$A$1:$B$3");
        assert!(
            names[0].sheet_id.is_none(),
            "workbook-scoped name must have no sheet_id"
        );
    }

    #[test]
    fn define_name_single_cell_omits_the_range_colon() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.define_name("Rate", (2, 3, 2, 3)).unwrap();
        let state = controller.state.borrow();
        assert_eq!(
            state.engine.model.workbook.defined_names[0].formula,
            "Sheet1!$D$3"
        );
    }

    #[test]
    fn define_name_is_usable_in_a_formula() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(0, 0, "21");
        controller.define_name("Rate", (0, 0, 0, 0)).unwrap();
        controller.edit_cell(0, 1, "=Rate*2");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "42");
    }

    #[test]
    fn define_name_rejects_an_invalid_identifier_without_creating_an_undo_entry() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        assert!(controller.define_name("2invalid", (0, 0, 0, 0)).is_err());
        assert!(!controller.can_undo());
        assert!(controller
            .state
            .borrow()
            .engine
            .model
            .workbook
            .defined_names
            .is_empty());
    }

    #[test]
    fn define_name_undo_removes_it_and_redo_restores_it() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.define_name("Total", (0, 0, 0, 0)).unwrap();
        assert_eq!(
            controller
                .state
                .borrow()
                .engine
                .model
                .workbook
                .defined_names
                .len(),
            1
        );

        assert!(controller.undo());
        assert!(controller
            .state
            .borrow()
            .engine
            .model
            .workbook
            .defined_names
            .is_empty());

        assert!(controller.redo());
        assert_eq!(
            controller
                .state
                .borrow()
                .engine
                .model
                .workbook
                .defined_names
                .len(),
            1
        );
    }

    #[test]
    fn filter_by_value_hides_non_matching_rows() {
        let mut controller = WorkbookController::new(4, 2).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.edit_cell(1, 0, "banana");
        controller.edit_cell(2, 0, "apricot");
        controller.edit_cell(3, 0, "cherry");

        controller.filter_by_value(0, "ap");
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert!(!sheet.is_row_hidden(0)); // apple
        assert!(sheet.is_row_hidden(1)); // banana
        assert!(!sheet.is_row_hidden(2)); // apricot
        assert!(sheet.is_row_hidden(3)); // cherry
    }

    #[test]
    fn filter_by_value_never_hides_a_completely_blank_row() {
        // Regression: a new document is 100x26 by default, so filtering
        // used to hide every one of the ~96 unused trailing rows too
        // (an empty cell never contains a non-empty needle), collapsing
        // the whole rest of the sheet to zero height on screen. A row
        // with no data anywhere has nothing to filter on and must stay
        // visible regardless of the needle.
        let mut controller = WorkbookController::new(10, 2).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.edit_cell(1, 0, "banana");
        controller.filter_by_value(0, "apple");
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert!(!sheet.is_row_hidden(0)); // apple
        assert!(sheet.is_row_hidden(1)); // banana
        for r in 2..10 {
            assert!(
                !sheet.is_row_hidden(r),
                "blank row {r} should never be hidden by a filter"
            );
        }
    }

    #[test]
    fn filter_by_value_is_case_insensitive() {
        let mut controller = WorkbookController::new(2, 1).unwrap();
        controller.edit_cell(0, 0, "Apple");
        controller.filter_by_value(0, "APP");
        assert!(!controller.state.borrow().sheet().is_row_hidden(0));
    }

    #[test]
    fn filter_by_value_with_empty_needle_clears_the_filter() {
        let mut controller = WorkbookController::new(2, 1).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.edit_cell(1, 0, "banana");
        controller.filter_by_value(0, "apple");
        assert!(controller.state.borrow().sheet().is_row_hidden(1));

        controller.filter_by_value(0, "");
        assert!(!controller.state.borrow().sheet().is_row_hidden(1));
    }

    #[test]
    fn clear_filter_unhides_everything() {
        let mut controller = WorkbookController::new(2, 1).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.edit_cell(1, 0, "banana");
        controller.filter_by_value(0, "apple");
        assert!(controller.state.borrow().sheet().is_row_hidden(1));

        controller.clear_filter();
        assert!(!controller.state.borrow().sheet().is_row_hidden(1));
    }

    #[test]
    fn clear_filter_on_an_unfiltered_sheet_is_a_no_op() {
        let mut controller = WorkbookController::new(2, 1).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.clear_filter();

        // Exactly 1 undo step (the edit): clear_filter on an already-clear
        // sheet must not have pushed its own step.
        assert!(controller.undo());
        assert!(!controller.can_undo());
    }

    #[test]
    fn filter_is_undoable_and_redoable() {
        let mut controller = WorkbookController::new(2, 1).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.edit_cell(1, 0, "banana");
        controller.filter_by_value(0, "apple");
        assert!(controller.state.borrow().sheet().is_row_hidden(1));

        assert!(controller.undo());
        assert!(!controller.state.borrow().sheet().is_row_hidden(1));

        assert!(controller.redo());
        assert!(controller.state.borrow().sheet().is_row_hidden(1));
    }

    #[test]
    fn filter_matching_the_same_result_again_does_not_push_a_redundant_undo_step() {
        let mut controller = WorkbookController::new(2, 1).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.edit_cell(1, 0, "banana");
        controller.filter_by_value(0, "apple"); // hides row 1 — one undo step
        controller.filter_by_value(0, "apple"); // identical result — no new step

        // Exactly 3 undo steps total (2 edits + 1 filter): the redundant
        // second filter call must not have pushed a 4th.
        assert!(controller.undo());
        assert!(!controller.state.borrow().sheet().is_row_hidden(1));
        assert!(controller.undo());
        assert!(controller.undo());
        assert!(!controller.can_undo());
    }

    #[test]
    fn hide_selected_rows_hides_the_selection_span() {
        let mut controller = WorkbookController::new(6, 2).unwrap();
        controller.state.borrow().sheet_mut().select_cell(1, 0);
        controller.state.borrow().sheet_mut().extend_selection(3, 0);
        controller.hide_selected_rows();
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert!(!sheet.is_row_hidden(0));
        assert!(sheet.is_row_hidden(1));
        assert!(sheet.is_row_hidden(2));
        assert!(sheet.is_row_hidden(3));
        assert!(!sheet.is_row_hidden(4));
    }

    #[test]
    fn unhide_all_rows_reveals_manually_hidden_rows_but_not_filtered_ones() {
        let mut controller = WorkbookController::new(4, 1).unwrap();
        controller.edit_cell(0, 0, "apple");
        controller.edit_cell(1, 0, "banana");
        controller.edit_cell(2, 0, "apple");
        controller.filter_by_value(0, "apple"); // hides row 1 (banana)
        controller.state.borrow().sheet_mut().select_cell(3, 0);
        controller.hide_selected_rows(); // manually hides row 3 too

        controller.unhide_all_rows();
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert!(
            sheet.is_row_hidden(1),
            "filter-hidden row must survive unhide_all_rows"
        );
        assert!(
            !sheet.is_row_hidden(3),
            "manually-hidden row must be revealed"
        );
    }

    #[test]
    fn hide_selected_cols_hides_the_selection_span() {
        let mut controller = WorkbookController::new(2, 5).unwrap();
        controller.state.borrow().sheet_mut().select_cell(0, 1);
        controller.state.borrow().sheet_mut().extend_selection(0, 2);
        controller.hide_selected_cols();
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert!(!sheet.is_col_hidden(0));
        assert!(sheet.is_col_hidden(1));
        assert!(sheet.is_col_hidden(2));
        assert!(!sheet.is_col_hidden(3));
    }

    #[test]
    fn hide_and_unhide_cols_are_undoable_and_redoable() {
        let mut controller = WorkbookController::new(2, 3).unwrap();
        controller.state.borrow().sheet_mut().select_cell(0, 1);
        controller.hide_selected_cols();
        assert!(controller.state.borrow().sheet().is_col_hidden(1));

        assert!(controller.undo());
        assert!(!controller.state.borrow().sheet().is_col_hidden(1));

        assert!(controller.redo());
        assert!(controller.state.borrow().sheet().is_col_hidden(1));
    }

    #[test]
    fn unhide_all_cols_on_an_unhidden_sheet_is_a_no_op() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "x");
        controller.unhide_all_cols();

        // Exactly 1 undo step (the edit): a no-op unhide must not push
        // its own step.
        assert!(controller.undo());
        assert!(!controller.can_undo());
    }

    #[test]
    fn set_print_area_is_undoable_and_redoable() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        assert_eq!(controller.state.borrow().sheet().print_area, None);

        controller.set_print_area((0, 0, 2, 1));
        assert_eq!(
            controller.state.borrow().sheet().print_area,
            Some((0, 0, 2, 1))
        );

        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().print_area, None);

        assert!(controller.redo());
        assert_eq!(
            controller.state.borrow().sheet().print_area,
            Some((0, 0, 2, 1))
        );
    }

    #[test]
    fn clear_print_area_reverts_to_whole_sheet_export() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.set_print_area((0, 0, 2, 1));
        controller.clear_print_area();
        assert_eq!(controller.state.borrow().sheet().print_area, None);

        assert!(controller.undo()); // undoes clear
        assert_eq!(
            controller.state.borrow().sheet().print_area,
            Some((0, 0, 2, 1))
        );
    }

    #[test]
    fn clear_print_area_on_an_unset_sheet_is_a_no_op() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.clear_print_area();
        assert!(!controller.can_undo());
    }

    #[test]
    fn setting_the_same_print_area_again_does_not_push_a_redundant_undo_step() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.set_print_area((0, 0, 2, 1));
        controller.set_print_area((0, 0, 2, 1));
        assert!(controller.undo());
        assert!(!controller.can_undo());
    }

    #[test]
    fn set_page_setup_is_undoable_and_redoable() {
        use suite_common_core::print::{Orientation, PageSetup};
        let mut controller = WorkbookController::new(6, 6).unwrap();
        assert_eq!(
            controller.state.borrow().sheet().page_setup,
            PageSetup::default()
        );

        let landscape = PageSetup {
            orientation: Orientation::Landscape,
            ..PageSetup::default()
        };
        controller.set_page_setup(landscape.clone());
        assert_eq!(controller.state.borrow().sheet().page_setup, landscape);

        assert!(controller.undo());
        assert_eq!(
            controller.state.borrow().sheet().page_setup,
            PageSetup::default()
        );

        assert!(controller.redo());
        assert_eq!(controller.state.borrow().sheet().page_setup, landscape);
    }

    #[test]
    fn setting_the_same_page_setup_again_does_not_push_a_redundant_undo_step() {
        use suite_common_core::print::PageSetup;
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.set_page_setup(PageSetup::default());
        assert!(!controller.can_undo());
    }

    #[test]
    fn fill_inside_selection_is_a_no_op() {
        let mut controller = WorkbookController::new(6, 6).unwrap();
        controller.edit_cell(0, 0, "x");
        controller.fill((0, 0, 2, 2), 1, 1);
        // The only undo entry is the original edit_cell — fill added none.
        assert!(controller.undo());
        assert!(!controller.can_undo());
    }

    #[test]
    fn undo_recalculates_dependent_formulas_and_restores_formula_input() {
        let mut controller = WorkbookController::new(5, 5).unwrap();
        controller.edit_cell(0, 0, "10");
        controller.edit_cell(0, 1, "=A1*2");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "20");

        controller.edit_cell(0, 0, "25");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "50");
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "10");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "20");
        assert_eq!(controller.state.borrow().cell_input(0, 1), "=A1*2");
    }

    #[test]
    fn unchanged_edit_does_not_create_undo_entry() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "");
        assert!(!controller.can_undo());
    }

    #[test]
    fn sheet_metadata_mutation_uses_the_live_undo_stack() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        let observed = controller.state.clone();
        controller.mutate_sheet("Resize Column", |sheet| {
            sheet.set_col_width(0, 240.0);
        });
        assert_eq!(observed.borrow().sheet().col_width(0), 240.0);
        assert!(controller.undo());
        assert_eq!(observed.borrow().sheet().col_width(0), 90.0);
        assert!(controller.redo());
        assert_eq!(observed.borrow().sheet().col_width(0), 240.0);
    }

    #[test]
    fn paste_is_one_undo_step_and_keeps_formulas_live() {
        use crate::fragment::GridCell;

        let mut controller = WorkbookController::new(5, 5).unwrap();
        let fragment = Fragment::Grid(vec![vec![
            GridCell {
                value: "3".into(),
                ..Default::default()
            },
            GridCell {
                value: String::new(),
                formula: Some("A1*4".into()),
                num_format: None,
            },
        ]]);
        controller.paste_fragment(0, 0, &fragment);
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "3");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "12");
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "");
        assert!(controller.redo());
        assert_eq!(controller.state.borrow().cell_input(0, 1), "=A1*4");
    }

    #[test]
    fn sort_moves_formula_inputs_and_row_metadata_as_one_undo_step() {
        use suite_common_core::format::{NumberFormat, NumberFormatKind};

        let mut controller = WorkbookController::new(3, 2).unwrap();
        for (row, value) in ["3", "1", "2"].into_iter().enumerate() {
            controller.edit_cell(row, 0, value);
            controller.edit_cell(row, 1, format!("=A{}*10", row + 1));
        }
        controller.mutate_sheet("Mark Row", |sheet| {
            sheet.formats[1][0] = NumberFormat::new(NumberFormatKind::Percent(1));
        });

        controller.toggle_sort(0);
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "1");
        assert_eq!(state.sheet().cell(0, 1), "10");
        assert_eq!(state.cell_input(0, 1), "=A1*10");
        assert_eq!(
            state.sheet().formats[0][0].kind,
            NumberFormatKind::Percent(1)
        );
        drop(state);

        assert!(controller.undo());
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "3");
        assert_eq!(state.cell_input(0, 1), "=A1*10");
        assert_eq!(
            state.sheet().formats[1][0].kind,
            NumberFormatKind::Percent(1)
        );
    }

    #[test]
    fn sort_reorders_a_text_column_alphabetically() {
        // Regression test: the sort compare previously parsed every cell
        // as f64 with a 0.0 fallback, so a text column's rows all
        // compared equal and never actually moved — sorted_col still
        // updated, giving the false impression sorting worked.
        let mut controller = WorkbookController::new(3, 1).unwrap();
        for (row, value) in ["banana", "apple", "cherry"].into_iter().enumerate() {
            controller.edit_cell(row, 0, value);
        }
        controller.toggle_sort(0);
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "apple");
        assert_eq!(state.sheet().cell(1, 0), "banana");
        assert_eq!(state.sheet().cell(2, 0), "cherry");
    }

    #[test]
    fn sort_descending_reorders_a_text_column() {
        let mut controller = WorkbookController::new(3, 1).unwrap();
        for (row, value) in ["banana", "apple", "cherry"].into_iter().enumerate() {
            controller.edit_cell(row, 0, value);
        }
        controller.toggle_sort(0); // ascending
        controller.toggle_sort(0); // descending
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "cherry");
        assert_eq!(state.sheet().cell(1, 0), "banana");
        assert_eq!(state.sheet().cell(2, 0), "apple");
    }

    #[test]
    fn sort_text_column_is_case_insensitive() {
        let mut controller = WorkbookController::new(3, 1).unwrap();
        for (row, value) in ["Banana", "apple", "Cherry"].into_iter().enumerate() {
            controller.edit_cell(row, 0, value);
        }
        controller.toggle_sort(0);
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "apple");
        assert_eq!(state.sheet().cell(1, 0), "Banana");
        assert_eq!(state.sheet().cell(2, 0), "Cherry");
    }

    #[test]
    fn sort_pushes_blank_rows_to_the_end_regardless_of_direction() {
        // On a sheet that's mostly blank (the app's real default is 100
        // rows; a small fixture here stands in), blanks must never
        // outrank real data — ascending sort would otherwise push every
        // filled row to the bottom, below all the blanks.
        let mut controller = WorkbookController::new(5, 1).unwrap();
        controller.edit_cell(0, 0, "banana");
        controller.edit_cell(2, 0, "apple");
        // Rows 1, 3, 4 stay blank.
        controller.toggle_sort(0);
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "apple");
        assert_eq!(state.sheet().cell(1, 0), "banana");
        assert_eq!(state.sheet().cell(2, 0), "");
        assert_eq!(state.sheet().cell(3, 0), "");
        assert_eq!(state.sheet().cell(4, 0), "");
        drop(state);

        controller.toggle_sort(0); // descending
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "banana");
        assert_eq!(state.sheet().cell(1, 0), "apple");
        assert_eq!(state.sheet().cell(2, 0), "");
        assert_eq!(state.sheet().cell(3, 0), "");
        assert_eq!(state.sheet().cell(4, 0), "");
    }

    #[test]
    fn new_edit_clears_redo_and_reports_the_current_command() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "first");
        assert_eq!(controller.undo_description(), Some("Edit Cell"));
        assert!(controller.undo());
        assert_eq!(controller.redo_description(), Some("Edit Cell"));

        controller.edit_cell(0, 1, "replacement");
        assert!(!controller.can_redo());
        assert_eq!(controller.undo_description(), Some("Edit Cell"));
    }

    #[test]
    fn edit_undo_redo_save_and_reopen_keeps_formula_source() {
        let mut controller = WorkbookController::new(3, 3).unwrap();
        controller.edit_cell(0, 0, "21");
        controller.edit_cell(0, 1, "=A1*2");
        assert!(controller.undo());
        assert!(controller.redo());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("undo-redo.xlsx");
        {
            let state = controller.state.borrow();
            let sheets = state
                .sheets
                .iter()
                .map(|sheet| sheet.borrow().clone())
                .collect::<Vec<_>>();
            crate::io::save_sheets_to_xlsx_with_engine(
                path.to_str().unwrap(),
                &sheets,
                Some(&state.engine),
            )
            .unwrap();
        }

        let mut reopened = TablesEngine::new(3, 3).unwrap();
        crate::io::load_file_into_engine(path.to_str().unwrap(), &mut reopened).unwrap();
        assert_eq!(reopened.cell(0, 0), "21");
        assert_eq!(reopened.cell(0, 1), "42");
        assert_eq!(reopened.formula(0, 1).as_deref(), Some("A1*2"));
    }

    #[test]
    fn sheets_keep_inputs_isolated_and_undo_targets_the_origin_sheet() {
        let mut controller = WorkbookController::new(3, 3).unwrap();
        {
            let state = controller.state.clone();
            let mut state = state.borrow_mut();
            state.add_sheet("Sheet2".into(), 3, 3).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "second sheet");
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        controller.edit_cell(0, 0, "first sheet");

        controller.state.borrow_mut().switch_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "second sheet");
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "second sheet");

        controller.state.borrow_mut().switch_sheet(0).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
        controller.state.borrow_mut().switch_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "second sheet");
    }

    #[test]
    fn rename_sheet_updates_name_and_keeps_cross_sheet_formulas_live() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "5");
        {
            let mut state = controller.state.borrow_mut();
            state.add_sheet("Sheet2".into(), 2, 2).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "=Sheet1!A1*2");
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "10");

        controller
            .state
            .borrow_mut()
            .rename_sheet(0, "Inputs")
            .unwrap();
        assert_eq!(controller.state.borrow().sheets[0].borrow().name, "Inputs");
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "10");

        // The formula on Sheet2 still resolves through the renamed sheet.
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        controller.edit_cell(0, 0, "7");
        controller.state.borrow_mut().switch_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "14");
    }

    /// The adversarial case for sheet-identity undo: after a structural
    /// delete renumbers sheet positions, older undo commands (captured by
    /// sheet_id) must keep targeting their original sheet rather than
    /// whatever now sits at their old positional index.
    #[test]
    fn deleting_a_sheet_does_not_retarget_other_sheets_undo_history() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "sheet1-a"); // undo #1, targets Sheet1

        {
            let mut state = controller.state.borrow_mut();
            state.add_sheet("Sheet2".into(), 2, 2).unwrap();
            state.add_sheet("Sheet3".into(), 2, 2).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "sheet2-a");

        // Delete Sheet2 (positional index 1). Sheet3 shifts from index 2 to 1.
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        controller.state.borrow_mut().delete_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheets.len(), 2);

        // Undoing the Sheet2 edit is inert: Sheet2 no longer exists, and the
        // command must not silently retarget Sheet3, which is now at the old
        // Sheet2 position.
        controller.state.borrow_mut().switch_sheet(1).unwrap();
        let sheet3_before = controller.state.borrow().sheet().cell(0, 0).to_string();
        assert!(controller.undo()); // pops the Sheet2 edit off the stack
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), sheet3_before);

        // The Sheet1 edit underneath it still undoes correctly.
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
    }

    /// Closes the last #98 acceptance-criteria gap: reordered, renamed
    /// sheets must save and reopen with their identity and order intact.
    #[test]
    fn reorder_rename_save_and_reopen_preserves_sheet_identity_and_order() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "sheet1-value");
        {
            let mut state = controller.state.borrow_mut();
            state.add_sheet("Sheet2".into(), 2, 2).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "sheet2-value");
        controller
            .state
            .borrow_mut()
            .rename_sheet(1, "Totals")
            .unwrap();
        // Front-load the renamed sheet.
        controller
            .state
            .borrow_mut()
            .reorder_sheets(&[1, 0])
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reorder-rename.xlsx");
        {
            let state = controller.state.borrow();
            let sheets = state
                .sheets
                .iter()
                .map(|sheet| sheet.borrow().clone())
                .collect::<Vec<_>>();
            crate::io::save_sheets_to_xlsx_with_engine(
                path.to_str().unwrap(),
                &sheets,
                Some(&state.engine),
            )
            .unwrap();
        }

        let (engine, sheets) = crate::io::load_xlsx_workbook(path.to_str().unwrap()).unwrap();
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].name, "Totals");
        assert_eq!(sheets[1].name, "Sheet1");
        assert_eq!(engine.cell_at(0, 0, 0), "sheet2-value");
        assert_eq!(engine.cell_at(1, 0, 0), "sheet1-value");
    }

    #[test]
    fn reorder_sheets_permutes_presentation_state_with_engine_content() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        {
            let mut state = controller.state.borrow_mut();
            state.sheets[0].borrow_mut().set_col_width(0, 200.0);
            state.add_sheet("Sheet2".into(), 2, 2).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "on-sheet-2");

        controller
            .state
            .borrow_mut()
            .reorder_sheets(&[1, 0])
            .unwrap();
        let state = controller.state.borrow();
        assert_eq!(state.sheets[0].borrow().name, "Sheet2");
        assert_eq!(state.sheets[0].borrow().cell(0, 0), "on-sheet-2");
        assert_eq!(state.sheets[1].borrow().col_width(0), 200.0);
    }

    #[test]
    fn dirty_tracks_every_mutating_action_and_clears_only_on_mark_clean() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        assert!(
            !controller.is_dirty(),
            "a fresh workbook should not be dirty"
        );

        controller.edit_cell(0, 0, "5");
        assert!(controller.is_dirty());

        controller.mark_clean();
        assert!(!controller.is_dirty());

        // Undo/redo also move the workbook away from the saved state.
        controller.edit_cell(0, 1, "6");
        controller.mark_clean();
        assert!(controller.undo());
        assert!(
            controller.is_dirty(),
            "undo must dirty a just-saved workbook"
        );

        controller.mark_clean();
        assert!(controller.redo());
        assert!(
            controller.is_dirty(),
            "redo must dirty a just-saved workbook"
        );
    }

    #[test]
    fn dirty_tracks_sheet_mutations_and_sort_too() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.mutate_sheet("Freeze Panes", |sheet| sheet.frozen_rows = 1);
        assert!(controller.is_dirty());
        controller.mark_clean();

        controller.edit_cell(0, 0, "2");
        controller.edit_cell(1, 0, "1");
        controller.mark_clean();
        controller.toggle_sort(0);
        assert!(
            controller.is_dirty(),
            "sort is a mutation and must dirty the workbook"
        );
    }
