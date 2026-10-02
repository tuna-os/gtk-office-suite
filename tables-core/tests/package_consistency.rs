//! The packages Tables writes are consistent (#1274): every part has a
//! content type, and nothing relates to a part that isn't there.
//! `suite_common_core::carry::problems` is the check.

#[test]
fn rewriting_the_fuzz_seed_workbooks_gives_consistent_packages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus/tables_xlsx");
    let dir = tempfile::tempdir().unwrap();
    let mut checked = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let src = entry.unwrap().path();
        if src.extension().and_then(|e| e.to_str()) != Some("xlsx") {
            continue;
        }
        let Ok((engine, sheets)) = tables_core::io::load_workbook(src.to_str().unwrap()) else { continue };
        let out = dir.path().join(src.file_name().unwrap());
        tables_core::io::save_sheets_to_xlsx_with_engine(out.to_str().unwrap(), &sheets, Some(&engine)).unwrap();
        assert_eq!(suite_common_core::carry::problems(&out), Vec::<String>::new(), "{}", src.display());
        checked += 1;
    }
    assert!(checked >= 3, "only {checked} seed workbooks read");
}
