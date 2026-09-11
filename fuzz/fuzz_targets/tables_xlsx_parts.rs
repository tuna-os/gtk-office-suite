#![no_main]

use libfuzzer_sys::fuzz_target;

// The reader takes a path, not bytes — there is no `from_bytes` variant
// anywhere in the core crates — so the input is written to a uniquely named
// temporary file. `NamedTempFile` rather than a name built from the pid and
// the input length: that scheme collides between libFuzzer workers handling
// equal-length inputs in one process, and a collision looks like a crash
// that will not reproduce.

// The three best-effort readers, which return no `Result` at all: a panic is
// their only possible failure mode.
fuzz_target!(|data: &[u8]| {
    let Ok(file) = tempfile::Builder::new().suffix(".xlsx").tempfile() else { return };
    if std::fs::write(file.path(), data).is_err() {
        return;
    }
    let Some(path) = file.path().to_str() else { return };
    let names = vec!["Sheet1".to_string()];
    tables_core::io::read_charts_from_xlsx(path);
    tables_core::io::read_cond_rules_from_xlsx(path);
    tables_core::io::read_sheet_props_from_xlsx(path, &names);
});
