#![no_main]

use libfuzzer_sys::fuzz_target;

// `NamedTempFile` rather than a name built from the pid and the input
// length: that scheme collides between libFuzzer workers handling
// equal-length inputs in one process, and a collision looks like a crash
// that will not reproduce.
fuzz_target!(|data: &[u8]| {
    let Ok(file) = tempfile::Builder::new().suffix(".pptx").tempfile() else { return };
    if std::fs::write(file.path(), data).is_err() {
        return;
    }
    let Some(path) = file.path().to_str() else { return };
    let _ = decks_core::engine::read_pptx(path);
});
