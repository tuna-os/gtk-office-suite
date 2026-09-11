#![no_main]

use libfuzzer_sys::fuzz_target;

// `markdown::parse` takes `&str` and returns a `Document`, not a `Result`:
// it has no way to report a bad input, so a panic is its only failure mode.
// No tempfile either, which makes this the cheapest target here — it runs
// many more iterations per second than the package readers.
fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = letters_core::markdown::parse(text);
    }
});
