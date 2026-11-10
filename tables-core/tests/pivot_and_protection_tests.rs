// pivot_and_protection_tests.rs — Unit and fixture tests for pivot tables and sheet protection (#114).
// SPDX-License-Identifier: GPL-3.0-or-later

use tables_core::controller::WorkbookController;
use tables_core::sheet::{PivotAggFunc, PivotDataField, PivotField, PivotTableSpec};

#[test]
fn test_pivot_table_evaluation_and_placement() {
    let mut controller = WorkbookController::new(20, 10).unwrap();

    // Populate source data (Region, Category, Sales)
    // Row 0: Headers
    controller.edit_cell(0, 0, "Region");
    controller.edit_cell(0, 1, "Category");
    controller.edit_cell(0, 2, "Sales");

    // Data rows
    controller.edit_cell(1, 0, "North"); controller.edit_cell(1, 1, "Electronics"); controller.edit_cell(1, 2, "100");
    controller.edit_cell(2, 0, "North"); controller.edit_cell(2, 1, "Apparel");     controller.edit_cell(2, 2, "50");
    controller.edit_cell(3, 0, "South"); controller.edit_cell(3, 1, "Electronics"); controller.edit_cell(3, 2, "200");
    controller.edit_cell(4, 0, "South"); controller.edit_cell(4, 1, "Apparel");     controller.edit_cell(4, 2, "150");

    let pivot = PivotTableSpec {
        name: "SalesSummary".to_string(),
        source_range: (0, 0, 4, 2),
        target_cell: (6, 0),
        row_fields: vec![PivotField { col_index: 0, name: "Region".to_string() }],
        col_fields: vec![],
        data_fields: vec![
            PivotDataField { col_index: 2, name: "Sales".to_string(), func: PivotAggFunc::Sum },
            PivotDataField { col_index: 2, name: "Sales".to_string(), func: PivotAggFunc::Count },
        ],
        extent: (0, 0),
    };

    controller.add_pivot_table(pivot);

    let state = controller.state.borrow();
    let sheet = state.sheet();

    // Target cell (6, 0): Header row [Region, Sum of Sales, Count of Sales]
    assert_eq!(sheet.data[6][0], "Region");
    assert_eq!(sheet.data[6][1], "Sum of Sales");
    assert_eq!(sheet.data[6][2], "Count of Sales");

    // Group rows: North (100 + 50 = 150, count 2)
    assert_eq!(sheet.data[7][0], "North");
    assert_eq!(sheet.data[7][1], "150");
    assert_eq!(sheet.data[7][2], "2");

    // Group rows: South (200 + 150 = 350, count 2)
    assert_eq!(sheet.data[8][0], "South");
    assert_eq!(sheet.data[8][1], "350");
    assert_eq!(sheet.data[8][2], "2");
}

#[test]
fn test_pivot_column_fields_create_cross_tab_intersections() {
    let mut controller = WorkbookController::new(20, 10).unwrap();
    for (row, values) in [
        ["Region", "Category", "Sales"],
        ["North", "Electronics", "100"],
        ["North", "Apparel", "50"],
        ["South", "Electronics", "200"],
    ]
    .iter()
    .enumerate()
    {
        for (column, value) in values.iter().enumerate() {
            controller.edit_cell(row, column, *value);
        }
    }

    let pivot = PivotTableSpec {
        name: "CrossTab".into(),
        source_range: (0, 0, 3, 2),
        target_cell: (6, 0),
        row_fields: vec![PivotField {
            col_index: 0,
            name: "Region".into(),
        }],
        col_fields: vec![PivotField {
            col_index: 1,
            name: "Category".into(),
        }],
        data_fields: vec![PivotDataField {
            col_index: 2,
            name: "Sales".into(),
            func: PivotAggFunc::Sum,
        }],
        extent: (0, 0),
    };
    let result = controller.evaluate_pivot_table(&pivot);
    controller.add_pivot_table(pivot);
    {
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert_eq!(sheet.data[6][0], "Region");
        assert_eq!(sheet.data[7][1], "50");
        assert_eq!(sheet.data[8][2], "200");
    }

    assert_eq!(
        result,
        vec![
            vec![
                "Region".to_string(),
                "Apparel - Sum of Sales".to_string(),
                "Electronics - Sum of Sales".to_string(),
            ],
            vec!["North".into(), "50".into(), "100".into()],
            vec!["South".into(), "0".into(), "200".into()],
        ]
    );
    let sheet = {
        let state = controller.state.borrow();
        let sheet = state.sheet().clone();
        sheet
    };
    let loss_report =
        tables_core::io::loss::content_a_save_drops(None, std::slice::from_ref(&sheet));
    assert!(loss_report.features.iter().any(|feature| feature.id == "pivot-tables"));

    let bytes = tables_core::io::save_sheets_to_xlsx_bytes(&[sheet], None).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cross-tab.xlsx");
    std::fs::write(&path, bytes).unwrap();
    let (_engine, loaded_sheets) =
        tables_core::io::load_xlsx_workbook(path.to_str().unwrap()).unwrap();
    assert_eq!(loaded_sheets[0].data[7][1], "50");
    assert_eq!(loaded_sheets[0].data[8][2], "200");
    assert!(loaded_sheets[0].pivot_tables.is_empty());

}

#[test]
fn test_sheet_protection_enforcement() {
    let mut controller = WorkbookController::new(10, 10).unwrap();

    // Set value in A1 before protection
    controller.edit_cell(0, 0, "Original");

    // Lock cell A1, unlock cell A2
    controller.set_cell_locked(0, 0, true);
    controller.set_cell_locked(1, 0, false);

    // Enable sheet protection
    controller.set_sheet_protection(true, Some("hash123".into()));

    // Try editing locked cell A1 -> should be rejected (remains "Original")
    controller.edit_cell(0, 0, "Modified");
    {
        let state = controller.state.borrow();
        assert_eq!(state.sheet().data[0][0], "Original");
    }

    // Try editing unlocked cell A2 -> should succeed
    controller.edit_cell(1, 0, "Allowed");
    {
        let state = controller.state.borrow();
        assert_eq!(state.sheet().data[1][0], "Allowed");
    }

    // Disable sheet protection
    controller.set_sheet_protection(false, None);

    // Editing cell A1 should now succeed
    controller.edit_cell(0, 0, "ModifiedNow");
    {
        let state = controller.state.borrow();
        assert_eq!(state.sheet().data[0][0], "ModifiedNow");
    }
}

fn xlsx_part(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// Inspect the actual cell XF independently of Tables' style importer.
fn assert_xml_cell_protection(sheet: &str, styles: &str, cell: &str, locked: bool, hidden: bool) {
    let cell_tag = sheet.split("<c ").skip(1)
        .map(|s| s.split('>').next().unwrap())
        .find(|s| s.contains(&format!("r=\"{cell}\""))).expect("cell must be written");
    let style = cell_tag.split("s=\"").nth(1).map(|s| {
        s.split('"').next().unwrap().parse::<usize>().unwrap()
    }).unwrap_or(0);
    let xfs = styles.split("<cellXfs").nth(1).unwrap().split("</cellXfs>").next().unwrap();
    let xf = xfs.split("<xf ").nth(style + 1).unwrap();
    assert_eq!(!xf.contains("locked=\"0\""), locked, "{cell}: {xf}");
    assert_eq!(xf.contains("hidden=\"1\""), hidden, "{cell}: {xf}");
    if !locked || hidden {
        assert!(xf.contains("applyProtection=\"1\""), "{cell}: {xf}");
    }
}

#[test]
fn imported_protection_survives_later_sheet_edits_and_undo_in_real_xlsx() {
    use rust_xlsxwriter::{Format, Formula, ProtectionOptions, Workbook};
    use std::cell::RefCell;
    use std::rc::Rc;
    use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("protected.xlsx");
    let mut fixture = Workbook::new();
    fixture.add_worksheet().set_name("First").unwrap().write_string(0, 0, "first").unwrap();
    let later = fixture.add_worksheet();
    later.set_name("Later").unwrap();
    later.merge_range(0, 0, 0, 1, "", &Format::new()).unwrap();
    later.write_formula_with_format(0, 0, Formula::new("1+1").set_result("2"),
        &Format::new().set_hidden().set_bold().set_num_format("0.00")).unwrap();
    later.write_string_with_format(1, 0, "before", &Format::new().set_unlocked().set_italic()).unwrap();
    later.write_number_with_format(2, 0, 12.0, &Format::new().set_unlocked().set_hidden()).unwrap();
    later.write_blank(8, 3, &Format::new().set_unlocked()).unwrap();
    later.protect_with_options(&ProtectionOptions {
        select_locked_cells: false,
        format_cells: true,
        insert_rows: true,
        ..Default::default()
    });
    fixture.save(&source).unwrap();
    let original = std::fs::read(&source).unwrap();
    let (engine, sheets) = load_workbook(source.to_str().unwrap()).unwrap();
    let expected_protection = sheets[1].protection.clone();
    let expected_formula_style = sheets[1].styles[0][0].clone();
    let expected_formula_format = sheets[1].formats[0][0].clone();
    let mut controller = WorkbookController::new(1, 1).unwrap();
    {
        let mut state = controller.state.borrow_mut();
        state.engine = engine;
        state.sheets = sheets.into_iter().map(|s| Rc::new(RefCell::new(s))).collect();
        state.switch_sheet(1).unwrap();
    }
    controller.edit_cell(1, 0, "after");
    controller.edit_cell(0, 0, "must be refused");
    for (round, expected_text) in ["after", "before"].into_iter().enumerate() {
        if round == 1 {
            assert!(controller.undo(), "undo the permitted edit, not the rejected one");
        }
        let bytes = {
            let state = controller.state.borrow();
            let sheets: Vec<_> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
            save_sheets_to_xlsx_bytes(&sheets, Some(&state.engine)).unwrap()
        };
        let sheet_xml = xlsx_part(&bytes, "xl/worksheets/sheet2.xml");
        let styles = xlsx_part(&bytes, "xl/styles.xml");
        assert!(sheet_xml.contains("<f>1+1</f><v>2</v>"), "{sheet_xml}");
        assert!(sheet_xml.contains("selectLockedCells=\"1\""), "{sheet_xml}");
        assert!(sheet_xml.contains("formatCells=\"0\""), "{sheet_xml}");
        assert!(sheet_xml.contains("insertRows=\"0\""), "{sheet_xml}");
        assert!(!sheet_xml.contains("selectUnlockedCells="), "{sheet_xml}");
        assert!(!sheet_xml.contains("deleteRows="), "{sheet_xml}");
        for (cell, locked, hidden) in [
            ("A1", true, true), ("B1", true, false), ("A2", false, false), ("A3", false, true), ("D9", false, false),
        ] {
            assert_xml_cell_protection(&sheet_xml, &styles, cell, locked, hidden);
        }
        let saved = dir.path().join(format!("saved-{round}.xlsx"));
        std::fs::write(&saved, bytes).unwrap();
        let (back_engine, back) = load_workbook(saved.to_str().unwrap()).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].data[0][0], "first");
        assert_eq!(back[1].data[1][0], expected_text);
        assert_eq!(back_engine.cell_at(1, 0, 0), "2");
        assert!(back_engine.formula_at(1, 0, 0).is_some());
        assert_eq!(back[1].protection, expected_protection);
        assert_eq!(back[1].styles[0][0], expected_formula_style);
        assert_eq!(back[1].formats[0][0], expected_formula_format);
        assert!(back[1].styles[1][0].italic);
        assert!(back[1].cell_protections[0][0].hidden_formula);
        assert!(!back[1].cell_protections[0][1].hidden_formula);
        assert_eq!(back[1].merges, [(0, 0, 1, 2)]);
        assert!(!back[1].cell_protections[1][0].locked);
        assert!(!back[1].cell_protections[2][0].locked);
        assert!(back[1].cell_protections[2][0].hidden_formula);
        assert!(!back[1].cell_protections[8][3].locked);
        assert!(back[1].data[8][3].is_empty());
    }
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn every_modeled_sheet_permission_combination_has_ooxml_disallow_semantics() {
    use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
    use tables_core::sheet::{SheetModel, SheetProtection};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.xlsx");
    for mask in 0..32 {
        let mut sheet = SheetModel::new("Permissions", 1, 1, 0);
        sheet.data[0][0] = "value".into();
        sheet.protection = SheetProtection {
            protected: true,
            allow_select_locked: mask & 1 != 0,
            allow_select_unlocked: mask & 2 != 0,
            allow_format_cells: mask & 4 != 0,
            allow_insert_rows: mask & 8 != 0,
            allow_delete_rows: mask & 16 != 0,
            ..Default::default()
        };
        let bytes = save_sheets_to_xlsx_bytes(&[sheet.clone()], None).unwrap();
        let xml = xlsx_part(&bytes, "xl/worksheets/sheet1.xml");
        let tag = xml.split("<sheetProtection ").nth(1).unwrap().split("/>").next().unwrap();
        for (attribute, allow, default_disallow) in [
            ("selectLockedCells", sheet.protection.allow_select_locked, false),
            ("selectUnlockedCells", sheet.protection.allow_select_unlocked, false),
            ("formatCells", sheet.protection.allow_format_cells, true),
            ("insertRows", sheet.protection.allow_insert_rows, true),
            ("deleteRows", sheet.protection.allow_delete_rows, true),
        ] {
            let prefix = format!("{attribute}=\"");
            let disallow = tag.split(&prefix).nth(1)
                .map_or(default_disallow, |s| s.starts_with("1\""));
            assert_eq!(disallow, !allow, "mask {mask}: {attribute}: {tag}");
        }
        std::fs::write(&path, bytes).unwrap();
        let (_, back) = load_workbook(path.to_str().unwrap()).unwrap();
        assert_eq!(back[0].protection, sheet.protection, "mask {mask}");
    }
}

#[test]
fn inactive_cell_protection_survives_until_a_sheet_is_protected() {
    use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
    use tables_core::sheet::SheetModel;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inactive.xlsx");
    let mut sheet = SheetModel::new("Inactive", 4, 2, 0);
    sheet.data[0][0] = "value".into();
    sheet.cell_protections[0][0].locked = false;
    sheet.merges.push((0, 0, 1, 2));
    sheet.cell_protections[3][1].locked = false;
    sheet.cell_protections[3][1].hidden_formula = true;
    let bytes = save_sheets_to_xlsx_bytes(&[sheet], None).unwrap();
    let xml = xlsx_part(&bytes, "xl/worksheets/sheet1.xml");
    assert!(!xml.contains("<sheetProtection"));
    assert_xml_cell_protection(&xml, &xlsx_part(&bytes, "xl/styles.xml"), "B4", false, true);
    std::fs::write(&path, bytes).unwrap();
    let (_, back) = load_workbook(path.to_str().unwrap()).unwrap();
    assert!(!back[0].protection.protected);
    assert!(!back[0].cell_protections[0][0].locked);
    assert!(back[0].cell_protections[0][1].locked);
    assert!(!back[0].cell_protections[3][1].locked);
    assert!(back[0].cell_protections[3][1].hidden_formula);
}

/// Every `<sheetProtection>` permission, alone and on protected and
/// unprotected sheets, is written with its own attribute — read back here
/// independently of the importer, against ECMA-376's defaults — and reopens
/// identically (TBL-005 step 3).
#[test]
fn tbl005_every_permission_round_trips_on_protected_and_unprotected_sheets() {
    use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
    use tables_core::sheet::{SheetModel, SheetProtection};
    // (attribute, ECMA-376 default "disallowed", model field)
    type Field = fn(&mut SheetProtection) -> &mut bool;
    let permissions: [(&str, bool, Field); 15] = [
        ("objects", false, |p| &mut p.allow_edit_objects),
        ("scenarios", false, |p| &mut p.allow_edit_scenarios),
        ("formatCells", true, |p| &mut p.allow_format_cells),
        ("formatColumns", true, |p| &mut p.allow_format_columns),
        ("formatRows", true, |p| &mut p.allow_format_rows),
        ("insertColumns", true, |p| &mut p.allow_insert_columns),
        ("insertRows", true, |p| &mut p.allow_insert_rows),
        ("insertHyperlinks", true, |p| &mut p.allow_insert_hyperlinks),
        ("deleteColumns", true, |p| &mut p.allow_delete_columns),
        ("deleteRows", true, |p| &mut p.allow_delete_rows),
        ("selectLockedCells", false, |p| &mut p.allow_select_locked),
        ("sort", true, |p| &mut p.allow_sort),
        ("autoFilter", true, |p| &mut p.allow_autofilter),
        ("pivotTables", true, |p| &mut p.allow_pivot_tables),
        ("selectUnlockedCells", false, |p| &mut p.allow_select_unlocked),
    ];
    let attribute = |tag: &str, name: &str| -> Option<String> {
        let prefix = format!(" {name}=\"");
        tag.split(&prefix).nth(1).map(|s| s.split('"').next().unwrap().to_string())
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.xlsx");
    for protected in [true, false] {
        // One sheet per permission flipped from the model default, plus a
        // later sheet left at the defaults.
        let mut sheets: Vec<SheetModel> = permissions.iter().enumerate().map(|(i, (_, _, field))| {
            let mut sheet = SheetModel::new(&format!("P{i}"), 1, 1, i as u32);
            sheet.data[0][0] = "value".into();
            sheet.protection.protected = protected;
            let flipped = !*field(&mut sheet.protection);
            *field(&mut sheet.protection) = flipped;
            sheet
        }).collect();
        sheets.push(SheetModel::new("Defaults", 1, 1, 99));
        sheets.last_mut().unwrap().protection.protected = protected;
        let bytes = save_sheets_to_xlsx_bytes(&sheets, None).unwrap();
        for (i, sheet) in sheets.iter().enumerate() {
            let xml = xlsx_part(&bytes, &format!("xl/worksheets/sheet{}.xml", i + 1));
            let Some(tag) = xml.split("<sheetProtection").nth(1).map(|t| t.split("/>").next().unwrap().to_string()) else {
                assert!(!protected && sheet.protection == SheetProtection::default(), "sheet {i}: no element written: {xml}");
                continue;
            };
            assert_eq!(xml.matches("<sheetProtection").count(), 1, "sheet {i}");
            assert_eq!(attribute(&tag, "sheet").as_deref() == Some("1"), protected, "sheet {i}: {tag}");
            for (name, disallowed_by_default, field) in &permissions {
                let mut model = sheet.protection.clone();
                let allowed = *field(&mut model);
                let disallowed = attribute(&tag, name).map_or(*disallowed_by_default, |v| v == "1" || v == "true");
                assert_eq!(disallowed, !allowed, "protected={protected} sheet {i}: {name} in {tag}");
            }
        }
        std::fs::write(&path, &bytes).unwrap();
        let (_, back) = load_workbook(path.to_str().unwrap()).unwrap();
        for (sheet, reopened) in sheets.iter().zip(&back) {
            assert_eq!(reopened.protection, sheet.protection, "protected={protected}: {}", sheet.name);
        }
    }
}

/// The Insert Pivot Table dialog's path (#1657): fields named from the
/// selection's header row, a spec built from the picks, inserted and undone
/// as one step through the controller.
#[test]
fn a_pivot_built_from_dialog_choices_inserts_and_undoes_as_one_step() {
    let mut controller = WorkbookController::new(20, 10).unwrap();
    for (row, values) in [
        ["Region", "", "Sales"],
        ["North", "Q1", "100"],
        ["North", "Q2", "50"],
        ["South", "Q1", "200"],
        ["South", "Q2", "150"],
    ]
    .iter()
    .enumerate()
    {
        for (col, value) in values.iter().enumerate() {
            controller.edit_cell(row, col, *value);
        }
    }
    let range = (0, 0, 4, 2);
    let (fields, spec) = {
        let state = controller.state.borrow();
        let sheet = state.sheet();
        (
            PivotTableSpec::source_fields(&sheet, range),
            PivotTableSpec::from_choice(&sheet, range, 0, Some(1), 2, PivotAggFunc::Sum, None).unwrap(),
        )
    };
    assert_eq!(fields, ["Region", "Column B", "Sales"]);
    assert_eq!(spec.target_cell, (0, 4), "placed two columns right of the source");
    assert_eq!(spec.col_fields[0].col_index, 1);

    controller.add_pivot_table(spec);
    {
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert_eq!(sheet.pivot_tables.len(), 1);
        assert_eq!(sheet.data[0][4], "Region");
        assert_eq!(sheet.data[0][5], "Q1 - Sum of Sales");
        assert_eq!(sheet.data[1][4..7], ["North", "100", "50"]);
        assert_eq!(sheet.data[2][4..7], ["South", "200", "150"]);
        // The source is untouched.
        assert_eq!(sheet.data[0][..3], ["Region", "", "Sales"]);
    }

    controller.undo();
    let state = controller.state.borrow();
    let sheet = state.sheet();
    assert!(sheet.pivot_tables.is_empty());
    assert_eq!(sheet.data[0][4], "");
    assert_eq!(sheet.data[2][6], "");
}

#[test]
fn a_pivot_needs_a_data_row_and_fields_inside_the_selection() {
    let mut controller = WorkbookController::new(20, 10).unwrap();
    controller.edit_cell(0, 0, "Region");
    controller.edit_cell(0, 1, "Sales");
    controller.edit_cell(1, 0, "North");
    controller.edit_cell(1, 1, "1");
    let state = controller.state.borrow();
    let sheet = state.sheet();
    assert!(PivotTableSpec::source_fields(&sheet, (0, 0, 0, 1)).is_empty(), "a header with no rows under it");
    assert!(PivotTableSpec::from_choice(&sheet, (0, 0, 1, 1), 0, None, 2, PivotAggFunc::Sum, None).is_none());
    assert!(PivotTableSpec::from_choice(&sheet, (0, 0, 1, 1), 0, Some(5), 1, PivotAggFunc::Sum, None).is_none());
    assert!(PivotTableSpec::from_choice(&sheet, (0, 0, 1, 1), 0, None, 1, PivotAggFunc::Count, None).is_some());
}

/// A pivot keeps its definition, so refreshing it after the source is edited
/// rewrites its table: a changed figure, a new group and a group that is gone
/// all show, the stale row is cleared, and one undo puts the old table back.
#[test]
fn refreshing_a_pivot_follows_source_edits_and_undoes_as_one_step() {
    let mut controller = WorkbookController::new(20, 10).unwrap();
    for (row, values) in [["Region", "Sales"], ["North", "100"], ["North", "50"], ["South", "200"]]
        .iter()
        .enumerate()
    {
        for (col, value) in values.iter().enumerate() {
            controller.edit_cell(row, col, *value);
        }
    }
    let range = (0, 0, 3, 1);
    let spec = {
        let state = controller.state.borrow();
        let sheet = state.sheet();
        PivotTableSpec::from_choice(&sheet, range, 0, None, 1, PivotAggFunc::Sum, Some((10, 0))).unwrap()
    };
    assert_eq!(spec.target_cell, (10, 0), "placed where asked");
    controller.add_pivot_table(spec);
    let column = |c: &WorkbookController, col: usize| {
        let state = c.state.borrow();
        let sheet = state.sheet();
        (10..14).map(|r| sheet.data[r][col].clone()).collect::<Vec<_>>()
    };
    assert_eq!(column(&controller, 0), ["Region", "North", "South", ""]);
    assert_eq!(controller.state.borrow().sheet().pivot_tables[0].extent, (3, 2));

    // Edits alone leave the pivot as it was, as in Calc and Excel.
    controller.edit_cell(2, 1, "75");
    controller.edit_cell(3, 0, "East");
    assert_eq!(column(&controller, 1), ["Sum of Sales", "150", "200", ""]);

    assert_eq!(controller.refresh_pivot_tables(), 1);
    assert_eq!(column(&controller, 0), ["Region", "East", "North", ""]);
    assert_eq!(column(&controller, 1), ["Sum of Sales", "200", "175", ""]);

    // A source with fewer groups leaves no stale row under the table.
    controller.edit_cell(1, 0, "East");
    controller.edit_cell(2, 0, "East");
    assert_eq!(controller.refresh_pivot_tables(), 1);
    assert_eq!(column(&controller, 0), ["Region", "East", "", ""]);
    assert_eq!(column(&controller, 1), ["Sum of Sales", "375", "", ""]);
    assert_eq!(controller.state.borrow().sheet().pivot_tables[0].extent, (2, 2));

    controller.undo();
    assert_eq!(column(&controller, 0), ["Region", "East", "North", ""]);
    assert_eq!(column(&controller, 1), ["Sum of Sales", "200", "175", ""]);
    assert_eq!(controller.state.borrow().sheet().pivot_tables[0].extent, (3, 2));
}

#[test]
fn a_pivot_cannot_be_placed_inside_its_source_and_refresh_needs_one() {
    let mut controller = WorkbookController::new(20, 10).unwrap();
    for (row, values) in [["Region", "Sales"], ["North", "1"]].iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            controller.edit_cell(row, col, *value);
        }
    }
    {
        let state = controller.state.borrow();
        let sheet = state.sheet();
        assert!(PivotTableSpec::from_choice(&sheet, (0, 0, 1, 1), 0, None, 1, PivotAggFunc::Sum, Some((1, 1))).is_none());
        assert!(PivotTableSpec::from_choice(&sheet, (0, 0, 1, 1), 0, None, 1, PivotAggFunc::Sum, Some((2, 0))).is_some());
    }
    assert_eq!(controller.refresh_pivot_tables(), 0, "no pivot to refresh");
}
