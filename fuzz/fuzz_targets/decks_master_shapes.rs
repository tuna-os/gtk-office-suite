#![no_main]

use libfuzzer_sys::fuzz_target;

// Raw slide-master XML straight in, no `Result` out, no tempfile.
fuzz_target!(|data: &[u8]| {
    if let Ok(xml) = std::str::from_utf8(data) {
        let _ = decks_core::engine::parse_master_shapes(xml);
    }
});
