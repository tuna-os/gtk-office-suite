//! The packages Tables writes are consistent (#1274): every part has a
//! content type, and nothing relates to a part that isn't there.
//! `suite_common_core::carry::problems` is the check.

/// Each committed workbook is read, written as xlsx, read back and written
/// again: the second write is of a package Tables wrote itself, the case
/// every later save of a document is.
#[test]
fn rewriting_committed_workbooks_gives_consistent_packages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let dir = tempfile::tempdir().unwrap();
    let sources = [
        root.join("fixtures/two_sheets.ods"),
        root.join("fixtures/offset_start.ods"),
    ];
    for src in &sources {
        let mut from = src.clone();
        for round in 0..2 {
            let (engine, sheets) = tables_core::io::load_workbook(from.to_str().unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", from.display()));
            let out = dir.path().join(format!("{}-{round}.xlsx", src.file_stem().unwrap().to_string_lossy()));
            tables_core::io::save_sheets_to_xlsx_with_engine(out.to_str().unwrap(), &sheets, Some(&engine)).unwrap();
            assert_eq!(suite_common_core::carry::problems(&out), Vec::<String>::new(), "{} round {round}", src.display());
            from = out;
        }
    }
}

/// TBL-001: the safe opaque parts of a source (a thumbnail and custom XML)
/// ride the real save path byte-identical, and the saved package stays
/// consistent. The loss gate stays silent about them: they are retained,
/// not dropped — the package-level half of "retained fields warn about
/// nothing".
#[test]
fn carried_safe_parts_survive_a_tables_save() {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let mut engine = tables_core::TablesEngine::new(8, 4).expect("engine");
    engine.set_cell_text(1, 1, "1274").unwrap();
    let mut sheet = tables_core::sheet::SheetModel::new("Sheet1", 8, 4, engine.sheet_id_at(0).unwrap());
    sheet.sync_from_engine(&engine);
    let source = dir.path().join("source.xlsx");
    tables_core::io::save_sheets_to_xlsx_with_engine(source.to_str().unwrap(), &[sheet.clone()], Some(&engine)).unwrap();
    // Plant the safe parts no reader models, with the content types and
    // package relationship Office gives them.
    let thumbnail: &[u8] = b"\xff\xd8\xff\xe0 a thumbnail, only its bytes matter \xff\xd9";
    let custom: &[u8] = b"<?xml version=\"1.0\"?><project><code>GTK-1274</code></project>";
    let planted = dir.path().join("planted.xlsx");
    {
        let file = std::fs::File::open(&source).unwrap();
        let mut old = zip::ZipArchive::new(file).unwrap();
        let out = std::fs::File::create(&planted).unwrap();
        let mut new = zip::ZipWriter::new(out);
        let options = zip::write::SimpleFileOptions::default();
        for index in 0..old.len() {
            let mut entry = old.by_index(index).unwrap();
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().unwrap().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            let (name, bytes) = if name == "[Content_Types].xml" {
                let mut text = String::from_utf8(bytes).unwrap();
                text = text.replace("</Types>", "<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/></Types>");
                (name, text.into_bytes())
            } else if name == "_rels/.rels" {
                let mut text = String::from_utf8(bytes).unwrap();
                text = text.replace("</Relationships>", "<Relationship Id=\"rIdThumb\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail\" Target=\"docProps/thumbnail.jpeg\"/></Relationships>");
                (name, text.into_bytes())
            } else {
                (name, bytes)
            };
            new.start_file(&name, options).unwrap();
            new.write_all(&bytes).unwrap();
        }
        new.start_file("docProps/thumbnail.jpeg", options).unwrap();
        new.write_all(thumbnail).unwrap();
        new.start_file("customXml/item1.xml", options).unwrap();
        new.write_all(custom).unwrap();
        new.finish().unwrap();
    }
    // The gate is silent about retained parts.
    let gate = tables_core::io::loss::content_a_save_drops(Some(planted.to_str().unwrap()), &[sheet]);
    assert!(gate.features.is_empty(), "retained parts must not warn: {:?}", gate.features);
    // The real path carries them: capture plus write_with, as persistence does.
    let carried = suite_common_core::carry::capture(&planted);
    assert!(!carried.is_empty());
    let target = dir.path().join("carried.xlsx");
    let (engine2, sheets2) = tables_core::io::load_workbook(planted.to_str().unwrap()).expect("reopen");
    carried.write_with(&target, |p| {
        tables_core::io::save_sheets_to_xlsx_with_engine(p.to_str().unwrap(), &sheets2, Some(&engine2))
    }).unwrap();
    let file = std::fs::File::open(&target).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    for (name, want) in [("docProps/thumbnail.jpeg", thumbnail), ("customXml/item1.xml", custom)] {
        let mut got = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut got).unwrap();
        assert_eq!(got, want, "{name} changed in transit");
    }
    assert_eq!(suite_common_core::carry::problems(&target), Vec::<String>::new());
}

/// TBL-002: a two-sheet workbook with charts on both sheets rewrites to a
/// consistent package twice over, and every chart still resolves to its
/// owning sheet after the second write.
#[test]
fn chart_relationships_stay_consistent_across_rewrites() {
    use tables_core::sheet::{ChartKind, ChartSeries, ChartSpec, LegendPosition};
    let dir = tempfile::tempdir().unwrap();
    let mut engine = tables_core::TablesEngine::new(10, 4).expect("engine");
    engine.add_sheet("Later").unwrap();
    let mut first = tables_core::sheet::SheetModel::new("First", 10, 4, engine.sheet_id_at(0).unwrap());
    first.data[1][0] = "A".into();
    first.data[1][1] = "3".into();
    first.charts.push(ChartSpec {
        kind: ChartKind::Bar, title: "First chart".into(),
        x_axis_title: None, y_axis_title: None, legend_position: LegendPosition::Right,
        series: vec![ChartSeries { name: "One".into(), cat: (1, 0, 1), val: (1, 1, 1), color: None }],
        cat: (1, 0, 1), val: (1, 1, 1), anchor: (3, 2),
        width_px: 480.0, height_px: 280.0,
    });
    let mut later = tables_core::sheet::SheetModel::new("Later", 10, 4, engine.sheet_id_at(1).unwrap());
    later.data[1][0] = "B".into();
    later.data[1][1] = "7".into();
    later.charts.push(ChartSpec {
        kind: ChartKind::Line, title: "Later chart".into(),
        x_axis_title: None, y_axis_title: None, legend_position: LegendPosition::None,
        series: vec![ChartSeries { name: String::new(), cat: (1, 0, 1), val: (1, 1, 1), color: None }],
        cat: (1, 0, 1), val: (1, 1, 1), anchor: (0, 2),
        width_px: 320.0, height_px: 200.0,
    });
    let once = dir.path().join("once.xlsx");
    tables_core::io::save_sheets_to_xlsx_with_engine(once.to_str().unwrap(), &[first, later], Some(&engine)).unwrap();
    assert_eq!(suite_common_core::carry::problems(&once), Vec::<String>::new());
    let (engine2, sheets2) = tables_core::io::load_workbook(once.to_str().unwrap()).expect("reopen");
    assert_eq!(sheets2[0].charts.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["First chart"]);
    assert_eq!(sheets2[1].charts.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["Later chart"]);
    let twice = dir.path().join("twice.xlsx");
    tables_core::io::save_sheets_to_xlsx_with_engine(twice.to_str().unwrap(), &sheets2, Some(&engine2)).unwrap();
    assert_eq!(suite_common_core::carry::problems(&twice), Vec::<String>::new());
    let (_, sheets3) = tables_core::io::load_workbook(twice.to_str().unwrap()).expect("reopen twice");
    assert_eq!(sheets3[0].charts.len(), 1);
    assert_eq!(sheets3[1].charts.len(), 1);
    assert_eq!(sheets3[1].charts[0].legend_position, LegendPosition::None);
}
