//! Golden-file assertions for tests that pin an on-disk format.
//!
//! The expected text lives in a file beside the code (`testdata/`), so a
//! change to what Hylki writes shows up as a diff of that file. A missing
//! golden file is written from the actual output and the test fails once,
//! which is how a new one is made.

use std::path::PathBuf;

/// Compares `actual` with the file at `src/<path>`.
pub(crate) fn assert_golden(path: &str, actual: &str) {
    let file: PathBuf = [env!("CARGO_MANIFEST_DIR"), "src", path].iter().collect();
    match std::fs::read_to_string(&file) {
        Ok(expected) => assert!(
            expected == actual,
            "{} differs from the output:\n--- expected\n{expected}\n--- actual\n{actual}",
            file.display()
        ),
        Err(_) => {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).unwrap();
            }
            std::fs::write(&file, actual).unwrap();
            panic!(
                "wrote the new golden file {}; check it and run again",
                file.display()
            );
        }
    }
}
