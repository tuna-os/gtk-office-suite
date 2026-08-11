#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!(
        "gtk-office-tables-{}-{}.xlsx",
        std::process::id(),
        data.len()
    ));
    if std::fs::write(&path, data).is_ok() {
        let _ = tables_core::io::load_xlsx_workbook(path.to_str().unwrap());
        let _ = std::fs::remove_file(path);
    }
});
