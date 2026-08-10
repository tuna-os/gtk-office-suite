//! Repo-level conformance gate: docs/PARITY.md must satisfy the structural
//! validator (tuna-os/gtk-office-suite #96).
//!
//! The parity scorecard is the suite's public trust signal — a green row
//! claims a feature is proven. This test shells out to the same Python
//! validator the nightly scorecard gates on, so a PR that breaks an
//! evidence link or cites a path that no longer exists fails in PR CI
//! (E1–E3), not just on the nightly schedule.
//!
//! python3 is already a developer/CI prerequisite (scorecard.py,
//! tests/gui/*, conformance/* all use it).

use std::path::Path;
use std::process::Command;

#[test]
fn parity_documentation_passes_conformance() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("suite-common-core sits directly under the repo root");
    let out = Command::new("python3")
        .arg("conformance/validate_parity.py")
        .current_dir(repo_root)
        .output()
        .expect("failed to run conformance/validate_parity.py (is python3 installed?)");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "docs/PARITY.md failed conformance validation (E1–E3).\
         \nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
}
