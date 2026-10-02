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
