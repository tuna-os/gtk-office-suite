#![no_main]

use libfuzzer_sys::fuzz_target;

// `integrity::check_package` walks a ZIP central directory by hand (offsets,
// lengths, local headers) before collabd publishes a version, and the same
// rule guards what a peer published. Hand-written offset arithmetic over
// untrusted bytes is exactly what should never panic (#1995).
//
// The first byte picks the format the bytes claim to be, so both the ODF
// and the OOXML required-entry paths are reached.
fuzz_target!(|data: &[u8]| {
    let Some((&pick, bytes)) = data.split_first() else { return };
    let mime = match pick % 3 {
        0 => "application/vnd.oasis.opendocument.text",
        1 => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    };
    let _ = dcp_core::integrity::check_package(mime, bytes);
});
