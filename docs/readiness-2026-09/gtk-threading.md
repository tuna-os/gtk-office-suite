## September readiness dependency: GTK tests must run on one owning main thread

This remains a P0 testing prerequisite for #354, #313 and #441. Current `nightly.yml` on `e7e4df6` runs cargo llvm-cov --workspace without starting Xvfb. Related reports #304/#308/#332/#355/#376 describe the same failure family; use a single fix and attach results across them instead of creating more reports.

Separate GTK-free tests from widget tests. Run widget operations through a shared initialized GTK main-thread dispatcher (or a dedicated process per widget test suite). A serialized Rust test harness still creates worker threads, so --test-threads=1 is not sufficient evidence of main-thread correctness.

Acceptance: repeat all GTK widget and bridge tests under isolated Xvfb without thread panics; run the actual coverage command with the same execution policy; fail setup if the display is unavailable; retain stderr/backtraces and coverage artifacts. Do not mask GTK initialization failure with test skips.
