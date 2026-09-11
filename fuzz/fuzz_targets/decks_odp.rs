#![no_main]

use libfuzzer_sys::fuzz_target;

// The reader takes a path, not bytes — there is no `from_bytes` variant
// anywhere in the core crates — so the input is written to a uniquely named
// temporary file. `NamedTempFile` rather than a name built from the pid and
// the input length: that scheme collides between libFuzzer workers handling
// equal-length inputs in one process, and a collision looks like a crash
// that will not reproduce.

fuzz_target!(|data: &[u8]| {
    let Ok(file) = tempfile::Builder::new().suffix(".odp").tempfile() else { return };
    if std::fs::write(file.path(), data).is_err() {
        return;
    }
    let Some(path) = file.path().to_str() else { return };
    let _ = decks_core::odp::read(path);
});
