#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!(
        "gtk-office-decks-{}-{}.pptx",
        std::process::id(),
        data.len()
    ));
    if std::fs::write(&path, data).is_ok() {
        let _ = decks_core::engine::read_pptx(path.to_str().unwrap());
        let _ = std::fs::remove_file(path);
    }
});
