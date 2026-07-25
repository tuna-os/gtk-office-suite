#!/usr/bin/env python3
"""Tables GUI tests — every test exercises a feature and verifies the result."""

import time
from framework import BaseGUITestCase

class TablesTest(BaseGUITestCase):
    app_name = "tables"

    def _new(self):
        try:
            self.app.child(name="New Document", roleName="push button").do_action(0)
        except Exception:
            self.app.keyCombo("<Control>n")
        time.sleep(1.0)

    def _fx(self):
        return self.app.child(roleName="text")

    def _click(self, name, role="toggle button"):
        try:
            self.app.child(name=name, roleName=role).do_action(0)
        except Exception:
            try:
                self.app.child(label=name, roleName=role).do_action(0)
            except Exception:
                pass

    def test_empty_state_shows_tables_branding(self):
        self.take_screenshot("empty")
        self.assertVision([
            "Window title is 'Tables'",
            "Status page shows heading 'Tables'",
            "An 'Open File...' button is shown",
        ], screenshot_path=self.last_screenshot)

    def test_new_spreadsheet_shows_grid_and_formula_bar(self):
        self._new()
        self.take_screenshot("grid")
        self.assertVision([
            "A spreadsheet grid with column headers A, B, C, D and row headers 1, 2, 3 is visible",
            "A formula bar (text entry field) is visible above the grid",
            "Sheet tabs at the bottom show 'Sheet1' with an add button",
        ], screenshot_path=self.last_screenshot)

    def test_enter_number_in_cell(self):
        self._new()
        fx = self._fx()
        fx.typeText("123")
        fx.keyCombo("Return")
        time.sleep(0.5)
        self.take_screenshot("number")
        self.assertVision([
            "Cell A1 displays the number '123'",
        ], screenshot_path=self.last_screenshot)

    def test_enter_text_in_cell(self):
        self._new()
        fx = self._fx()
        fx.typeText("Hello Tables")
        fx.keyCombo("Return")
        time.sleep(0.5)
        self.take_screenshot("text")
        self.assertVision([
            "Cell A1 displays the text 'Hello Tables'",
        ], screenshot_path=self.last_screenshot)

    def test_simple_formula_evaluates(self):
        self._new()
        fx = self._fx()
        fx.typeText("10")
        fx.keyCombo("Return")
        time.sleep(0.3)
        self.app.keyCombo("Down")
        time.sleep(0.15)
        fx.typeText("=A1*2")
        fx.keyCombo("Return")
        time.sleep(0.5)
        self.take_screenshot("formula")
        self.assertVision([
            "Cell A1 shows '10'",
            "Cell A2 shows the computed result '20' from the formula =A1*2",
        ], screenshot_path=self.last_screenshot)

    def test_sum_formula_aggregates_range(self):
        self._new()
        fx = self._fx()
        for val in ["5", "10", "15"]:
            fx.typeText(val)
            fx.keyCombo("Return")
            time.sleep(0.2)
            self.app.keyCombo("Down")
            time.sleep(0.1)
        time.sleep(0.2)
        fx.typeText("=SUM(A1:A3)")
        fx.keyCombo("Return")
        time.sleep(0.5)
        self.take_screenshot("sum")
        self.assertVision([
            "Cells A1, A2, A3 show values 5, 10, 15",
            "Cell A4 shows the sum result '30' from =SUM(A1:A3)",
        ], screenshot_path=self.last_screenshot)

    def test_formula_bar_visible_above_grid(self):
        self._new()
        self.take_screenshot("fxbar")
        self.assertVision([
            "A text entry formula bar is positioned above the spreadsheet grid",
        ], screenshot_path=self.last_screenshot)

    def test_column_and_row_headers_displayed(self):
        self._new()
        self.take_screenshot("headers")
        self.assertVision([
            "Column headers A, B, C, D are visible at the top of the grid area",
            "Row headers 1, 2, 3, 4, 5 are visible on the left side",
        ], screenshot_path=self.last_screenshot)

    def test_add_sheet_creates_sheet2_tab(self):
        self._new()
        self._click("Add sheet", "push button")
        time.sleep(0.5)
        self.take_screenshot("addsheet")
        self.assertVision([
            "Sheet tabs show both 'Sheet1' and 'Sheet2'",
        ], screenshot_path=self.last_screenshot)

    def test_add_multiple_sheets_shows_all_tabs(self):
        self._new()
        for _ in range(2):
            self._click("Add sheet", "push button")
            time.sleep(0.3)
        self.take_screenshot("threesheets")
        self.assertVision([
            "Sheet tabs show 'Sheet1', 'Sheet2', and 'Sheet3'",
        ], screenshot_path=self.last_screenshot)

    def test_sheet_switcher_dropdown_lists_sheets(self):
        self._new()
        self._click("Add sheet", "push button")
        time.sleep(0.3)
        self.take_screenshot("switcher")
        self.assertVision([
            "Sheet tabs or switcher shows Sheet1 and Sheet2 names",
        ], screenshot_path=self.last_screenshot)

    def test_gridlines_visible_between_cells(self):
        self._new()
        self.take_screenshot("gridlines")
        self.assertVision([
            "Light gridlines are drawn between all cells in the spreadsheet grid",
        ], screenshot_path=self.last_screenshot)

    def test_column_header_dividers_for_resize(self):
        self._new()
        self.take_screenshot("resize")
        self.assertVision([
            "Column headers A, B, C, D have visible divider lines between them for column resizing",
        ], screenshot_path=self.last_screenshot)

    def test_undo_removes_cell_value(self):
        self._new()
        fx = self._fx()
        fx.typeText("Undo this value")
        fx.keyCombo("Return")
        time.sleep(0.3)
        self.app.keyCombo("<Control>z")
        time.sleep(0.5)
        self.take_screenshot("undo")
        self.assertVision([
            "Cell A1 is empty or no longer shows the previously entered text",
        ], screenshot_path=self.last_screenshot)

    def test_chart_button_triggers_chart_dialog(self):
        """Enter data then click chart to see dialog."""
        self._new()
        fx = self._fx()
        fx.typeText("Sales")
        fx.keyCombo("Right")
        time.sleep(0.1)
        fx.typeText("100")
        fx.keyCombo("Return")
        time.sleep(0.3)
        self.app.keyCombo("Down")
        time.sleep(0.1)
        fx.typeText("Costs")
        fx.keyCombo("Right")
        time.sleep(0.1)
        fx.typeText("50")
        fx.keyCombo("Return")
        time.sleep(0.3)
        self._click("Chart", "push button")
        time.sleep(0.5)
        self.take_screenshot("chart")
        self.assertVision([
            "A chart dialog or chart preview is visible with data from the spreadsheet",
        ], screenshot_path=self.last_screenshot)

    def test_toggle_border_changes_cell_style(self):
        self._new()
        fx = self._fx()
        fx.typeText("Test")
        fx.keyCombo("Return")
        time.sleep(0.3)
        self._click("Toggle Cell Border", "push button")
        time.sleep(0.4)
        self.take_screenshot("border")
        self.assertVision([
            "The selected cell shows a border or outline around it",
        ], screenshot_path=self.last_screenshot)

    def test_toggle_number_format_cycles(self):
        self._new()
        fx = self._fx()
        fx.typeText("42")
        fx.keyCombo("Return")
        time.sleep(0.3)
        self._click("Toggle Number Format", "push button")
        time.sleep(0.4)
        self.take_screenshot("format")
        self.assertVision([
            "The cell value '42' shows a different number format (currency, percent, or decimal) after toggling",
        ], screenshot_path=self.last_screenshot)

    def test_merge_cells_button_toggles(self):
        self._new()
        fx = self._fx()
        fx.typeText("Merged")
        fx.keyCombo("Return")
        time.sleep(0.3)
        self.app.keyCombo("Tab")  # move to B1
        time.sleep(0.1)
        self._click("Merge Cells", "push button")
        time.sleep(0.4)
        self.take_screenshot("merge")
        self.assertVision([
            "The two adjacent cells (A1 and B1) appear merged into a single wider cell",
        ], screenshot_path=self.last_screenshot)

    def test_export_pdf_button_triggers_save_dialog(self):
        self._new()
        self._click("Export PDF", "push button")
        time.sleep(0.5)
        self.take_screenshot("export")
        self.assertVision([
            "A file save dialog for exporting a PDF is visible on screen",
        ], screenshot_path=self.last_screenshot)

    def test_preferences_shows_show_gridlines(self):
        self._new()
        try:
            self.app.keyCombo("<Control>comma")
        except Exception:
            pass
        time.sleep(0.5)
        self.take_screenshot("prefs")
        self.assertVision([
            "A Preferences dialog with a 'Show Gridlines' toggle switch is visible",
        ], screenshot_path=self.last_screenshot)
        try:
            self.app.keyCombo("Escape")
        except Exception:
            pass

    def test_full_spreadsheet_with_data_and_formula(self):
        """Full workflow: data entry, formulas, multiple sheets."""
        self._new()
        fx = self._fx()
        for val in ["100", "200", "300"]:
            fx.typeText(val)
            fx.keyCombo("Return")
            time.sleep(0.15)
            self.app.keyCombo("Down")
            time.sleep(0.1)
        time.sleep(0.15)
        fx.typeText("=SUM(A1:A3)")
        fx.keyCombo("Return")
        time.sleep(0.4)
        self._click("Add sheet", "push button")
        time.sleep(0.3)
        self.take_screenshot("final")
        self.assertVision([
            "Cell A1 shows '100'",
            "Cell A2 shows '200'",
            "Cell A3 shows '300'",
            "Cell A4 shows the sum result '600' from =SUM(A1:A3)",
            "Sheet tabs show 'Sheet1' and 'Sheet2'",
        ], screenshot_path=self.last_screenshot)


class Tables113FeatureVisuals(BaseGUITestCase):
    """Screenshot-only visual record of every #113 feature shipped in the
    2026-07-24/25 session (row filtering, sort indicator, row resize,
    named ranges, print area, page setup, formula reference
    highlighting). No assertVision calls (no VLM round trip needed) —
    each test just drives the feature via the same rawinput/AT-SPI
    mechanisms test_smoke.py's gating tests already prove reliable, and
    calls take_screenshot() so a human (or Claude) can eyeball the
    result. Re-run any single test here (`run_gui_tests.sh test_tables.py
    -k <name>`) any time a #113-area change needs a quick visual sanity
    check instead of re-deriving one from scratch.

    Canvas layout constants match TablesFillHandleSmoke/
    TablesSortIndicatorSmoke in test_smoke.py — empirically measured
    against tables-core::sheet's ROW_HEADER_WIDTH/COL_HEADER_HEIGHT/
    ROW_HEIGHT/COL_WIDTH under matchbox's fullscreen layout, not
    AT-SPI-reported position (this grid's virtual-cell nodes have their
    own known position bug, #132/#137)."""

    app_name = "tables"

    CANVAS_X = 0
    CANVAS_Y = 128
    ROW_HEADER_WIDTH = 50
    COL_HEADER_HEIGHT = 26
    ROW_HEIGHT = 28
    COL_WIDTH = 90

    def setUp(self):
        # Autosave isolation matters here specifically: without it, a
        # leftover crash-recovery snapshot from an unclean previous kill
        # (e.g. a manually pkill -9'd dev binary) gets auto-restored on
        # launch instead of a blank document, silently contaminating
        # every test in this class with stale cell data.
        self.isolate_autosave_state()
        self._snapshot_path = self.isolate_snapshot(prefix="tables-113-visual-")
        super().setUp()

    # ── shared helpers ────────────────────────────────────────────
    def _col_header_center(self, col):
        x = self.CANVAS_X + self.ROW_HEADER_WIDTH + col * self.COL_WIDTH + self.COL_WIDTH / 2
        y = self.CANVAS_Y + self.COL_HEADER_HEIGHT / 2
        return x, y

    def _row_divider_y(self, row):
        """Y of the divider *below* `row` (0-indexed), for drag-resize."""
        return self.CANVAS_Y + self.COL_HEADER_HEIGHT + (row + 1) * self.ROW_HEIGHT

    def _snapshot(self):
        import json
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "test-snapshot"])
        time.sleep(0.5)
        with open(self._snapshot_path) as f:
            return json.load(f)

    def _put(self, ref, value):
        from dogtail import rawinput

        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText(ref)
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText(value)
        rawinput.keyCombo("Return")
        time.sleep(0.3)

    def _new_document(self):
        import subprocess

        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "new-document"])
        time.sleep(1.5)

    # ── row filtering ─────────────────────────────────────────────
    def test_row_filtering(self):
        import subprocess

        self._new_document()
        self._put("A1", "apple")
        self._put("A2", "banana")
        self._put("A3", "apple")
        self._put("A4", "cherry")
        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "filter-by-column"])
        time.sleep(0.8)
        from dogtail import rawinput, tree
        rawinput.typeText("apple")
        self.take_screenshot("filter_dialog")
        # The dialog's entry has no connect_activate handler wired — only
        # the "Filter" button applies it, so Return here is a no-op.
        confirm = tree.root.findChild(lambda n: n.name == "Filter" and n.roleName == "push button")
        confirm.do_action(0)
        time.sleep(0.5)
        self.take_screenshot("filter_applied")
        snap = self._snapshot()
        hidden = set(snap["sheet"]["hidden_rows"])
        self.assertIn(1, hidden, f"row 2 (banana) should be hidden: {snap}")
        self.assertNotIn(0, hidden)
        self.assertNotIn(2, hidden)
        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "clear-filter"])
        time.sleep(0.5)
        self.take_screenshot("filter_cleared")
        self.assertIsNone(self.process.poll(), "tables crashed during filter visual check")

    # ── sort indicator ───────────────────────────────────────────
    def test_sort_indicator(self):
        from dogtail import rawinput

        self._new_document()
        self._put("A1", "b")
        self._put("A2", "a")
        self._put("A3", "c")
        # xdotool's `mousemove --sync` can hang if the pointer is already
        # at (or very near) the target pixel — confirmed multiple times
        # this session. Route every header click through the corner box
        # first (outside the header's click-to-sort zone, so it can't
        # itself trigger a sort) so each move is genuine, same workaround
        # TablesSortIndicatorSmoke in test_smoke.py already relies on.
        corner_x = self.CANVAS_X + self.ROW_HEADER_WIDTH / 2
        corner_y = self.CANVAS_Y + self.COL_HEADER_HEIGHT / 2
        x, y = self._col_header_center(0)
        rawinput.click(int(corner_x), int(corner_y))
        rawinput.click(int(x), int(y))
        time.sleep(0.5)
        self.take_screenshot("sort_ascending")
        snap = self._snapshot()
        self.assertEqual(snap["sheet"]["sorted_col"], {"col": 0, "ascending": True}, f"{snap}")
        cells = {(c["row"], c["col"]): c["value"] for c in snap["sheet"]["cells"]}
        # Blank rows (this sheet defaults to 100 rows; only 3 are filled)
        # must sort last, not first, so the filled rows stay at the top.
        self.assertEqual([cells.get((r, 0)) for r in range(3)], ["a", "b", "c"], f"data not reordered: {snap}")
        rawinput.click(int(corner_x), int(corner_y))
        rawinput.click(int(x), int(y))
        time.sleep(0.5)
        self.take_screenshot("sort_descending")
        snap = self._snapshot()
        self.assertEqual(snap["sheet"]["sorted_col"], {"col": 0, "ascending": False}, f"{snap}")
        cells = {(c["row"], c["col"]): c["value"] for c in snap["sheet"]["cells"]}
        self.assertEqual([cells.get((r, 0)) for r in range(3)], ["c", "b", "a"], f"data not reordered: {snap}")
        self.assertIsNone(self.process.poll(), "tables crashed during sort visual check")

    # ── row resize ───────────────────────────────────────────────
    def test_row_resize(self):
        from dogtail import rawinput

        self._new_document()
        self._put("A1", "resize me")
        divider_x = self.ROW_HEADER_WIDTH / 2
        divider_y = self._row_divider_y(0)
        self.drag(divider_x, divider_y, divider_x, divider_y + 40)
        time.sleep(0.5)
        self.take_screenshot("row_resized")
        self.assertIsNone(self.process.poll(), "tables crashed during row-resize visual check")

    # ── named ranges ─────────────────────────────────────────────
    def test_named_range_define_dialog(self):
        import subprocess
        from dogtail import rawinput, tree

        self._new_document()
        self._put("A1", "10")
        self._put("A2", "20")
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("<Shift>Down")
        time.sleep(0.3)
        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "define-name"])
        time.sleep(0.8)
        name_entry = tree.root.findChild(lambda n: n.name == "Name" and n.roleName == "text")
        name_entry.text = "MyRange"
        self.take_screenshot("define_name_dialog")
        confirm = tree.root.findChild(lambda n: n.name == "Define" and n.roleName == "push button")
        confirm.do_action(0)
        time.sleep(0.5)
        self.take_screenshot("define_name_done")
        self.assertIsNone(self.process.poll(), "tables crashed during named-range visual check")

    # ── print area ───────────────────────────────────────────────
    def test_print_area(self):
        import subprocess
        from dogtail import rawinput

        self._new_document()
        self._put("A1", "10")
        self._put("A2", "20")
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("A1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.keyCombo("<Shift>Down")
        time.sleep(0.3)
        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "set-print-area"])
        time.sleep(0.5)
        self.take_screenshot("print_area_set")
        # print_area isn't in the #104 state snapshot (only hidden_rows/
        # selection/sorted_col are) — this test is visual-only, relying
        # on the screenshot rather than an assertion on hidden state.
        self.assertIsNone(self.process.poll(), "tables crashed during print-area visual check")

    # ── page setup ───────────────────────────────────────────────
    def test_page_setup_dialog(self):
        import subprocess
        from dogtail import rawinput

        self._new_document()
        subprocess.run(["gapplication", "action", "org.tunaos.tables-rust", "page-setup"])
        time.sleep(0.8)
        self.take_screenshot("page_setup_dialog")
        rawinput.keyCombo("Escape")
        time.sleep(0.3)
        self.assertIsNone(self.process.poll(), "tables crashed during page-setup visual check")

    # ── formula reference highlighting ──────────────────────────
    def test_formula_reference_highlighting(self):
        from dogtail import rawinput

        self._new_document()
        self._put("A1", "10")
        self._put("A2", "20")
        self._put("A3", "30")
        rawinput.keyCombo("<Control>g")
        time.sleep(0.2)
        rawinput.typeText("C1")
        rawinput.keyCombo("Return")
        time.sleep(0.3)
        rawinput.typeText("=A1+A2+A3")
        time.sleep(0.5)
        self.take_screenshot("formula_highlighting")
        rawinput.keyCombo("Return")
        time.sleep(0.5)
        snap = self._snapshot()
        cells = {(c["row"], c["col"]): c["value"] for c in snap["sheet"]["cells"]}
        self.assertEqual(cells.get((0, 2)), "60", f"C1 should compute 60: {cells}")
        self.assertIsNone(self.process.poll(), "tables crashed during formula-highlight visual check")
