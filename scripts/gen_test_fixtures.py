#!/usr/bin/env python3
"""Generate test fixture files for tables-core unit tests.

Requires: odfpy  (pip install odfpy  OR  dnf install python3-odfpy)

Run from the repository root:
    python3 scripts/gen_test_fixtures.py

Fixtures produced
-----------------
tables-core/tests/fixtures/two_sheets.ods
    Two-sheet ODS workbook used by the load_ods_workbook unit tests:
    - Sheet "Sales"  rows: [product, qty, price] / [widget, 10, 2.50]
    - Sheet "Config" rows: [key, value] / [tax_rate, 0.08]

tables-core/tests/fixtures/offset_start.ods
    Single-sheet ODS whose content starts at B2, not A1 — row 1 and column A
    are empty. Regression fixture for #324: a loader that treats a range's
    size as its extent, or writes range-relative indices straight into the
    engine, shifts this content up and left into A1.
    - Sheet "Offset"  B2 = "corner", C2 = "right", B3 = "below"
"""

import pathlib

# NOTE: odfpy stamps generation metadata into each file, so re-running this
# script produces byte-different but semantically identical fixtures. Only
# commit a regenerated fixture when its *content* changed — an incidental byte
# diff is noise in every review that touches this script.

FIXTURES = pathlib.Path(__file__).parent.parent / "tables-core" / "tests" / "fixtures"
FIXTURES.mkdir(parents=True, exist_ok=True)


def _str_cell(text: str):
    from odf.table import TableCell
    from odf.text import P
    cell = TableCell(valuetype="string")
    cell.addElement(P(text=text))
    return cell


def _empty_cells(row, count):
    """Append `count` empty cells to `row` (ODS has no implicit gap)."""
    from odf.table import TableCell
    for _ in range(count):
        row.addElement(TableCell())


def gen_offset_start_ods():
    """Content deliberately begins at B2 so A1 and row 1 stay empty."""
    from odf.opendocument import OpenDocumentSpreadsheet
    from odf.table import Table, TableRow

    doc = OpenDocumentSpreadsheet()
    sheet = Table(name="Offset")
    doc.spreadsheet.addElement(sheet)

    # Row 1: entirely empty.
    blank = TableRow()
    sheet.addElement(blank)
    _empty_cells(blank, 3)

    # Row 2: A empty, then B2/C2.
    row2 = TableRow()
    sheet.addElement(row2)
    _empty_cells(row2, 1)
    row2.addElement(_str_cell("corner"))
    row2.addElement(_str_cell("right"))

    # Row 3: A empty, then B3.
    row3 = TableRow()
    sheet.addElement(row3)
    _empty_cells(row3, 1)
    row3.addElement(_str_cell("below"))

    out = FIXTURES / "offset_start.ods"
    doc.save(str(out))
    print(f"wrote {out}")


def gen_two_sheets_ods():
    from odf.opendocument import OpenDocumentSpreadsheet
    from odf.table import Table, TableRow

    doc = OpenDocumentSpreadsheet()

    # Sheet 1: Sales
    sheet1 = Table(name="Sales")
    doc.spreadsheet.addElement(sheet1)
    row = TableRow()
    sheet1.addElement(row)
    for v in ["product", "qty", "price"]:
        row.addElement(_str_cell(v))
    row2 = TableRow()
    sheet1.addElement(row2)
    for v in ["widget", "10", "2.50"]:
        row2.addElement(_str_cell(v))

    # Sheet 2: Config
    sheet2 = Table(name="Config")
    doc.spreadsheet.addElement(sheet2)
    row3 = TableRow()
    sheet2.addElement(row3)
    for v in ["key", "value"]:
        row3.addElement(_str_cell(v))
    row4 = TableRow()
    sheet2.addElement(row4)
    for v in ["tax_rate", "0.08"]:
        row4.addElement(_str_cell(v))

    out = FIXTURES / "two_sheets.ods"
    doc.save(str(out))
    print(f"  wrote {out}")


if __name__ == "__main__":
    print("Generating test fixtures ...")
    gen_two_sheets_ods()
    gen_offset_start_ods()
    print("Done.")
