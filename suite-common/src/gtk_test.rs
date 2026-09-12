// gtk_test.rs — one GTK thread for the whole test binary.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// GTK is thread-affine: widgets may only be created on the thread that called
// `gtk::init`, and `gtk::init` succeeds at most once per process. Rust's test
// harness spawns a fresh thread per test — even at `--test-threads=1` — so a
// test module that calls `gtk::init()` inline initialises GTK on whichever
// thread happened to run first, and every later test trips
// `assert_initialized_main_thread!` or faults inside GDK.
//
// The symptom is a whole test binary aborting rather than a named test
// failing, which is why it read as flaky for a long time (#304 and its
// duplicates, #241). Four hand-rolled variants of this had accumulated across
// the workspace; this is the one they should all call.
//
// This module is test support that ships in the library rather than under
// `#[cfg(test)]`, because the binaries' own test modules cannot see a
// dependency's test-only code.
//
// It used to skip — and *pass* — whenever GTK could not initialise, which
// read as reasonable and was not: the PR `test` job ran
// `cargo nextest run --workspace` with no display, so all 36 GTK widget
// tests in Letters skipped on every pull request while the harness reported
// "95 passed; 0 failed; 0 ignored". Nothing could see it, the ledger gate
// included: a skip that counts as a pass is invisible to a report that
// checks for skipped tests. A display-less run must now say so
// (`SUITE_GTK_TESTS=skip`); otherwise a widget test with no display fails,
// which is what #241 means by "do not mask GTK initialization failure with
// test skips".

use std::panic;
use std::sync::mpsc;
use std::sync::OnceLock;

/// Shared worker, or why there isn't one. `Err` once GTK has been found
/// uninitialisable (headless with no usable display), so every later call
/// skips instead of retrying an init that cannot succeed.
///
/// The reason is kept, not discarded. It used to be a bare `Option`, built
/// from `gtk4::init().is_ok()`, so a failure arrived as "GTK could not be
/// initialised" and nothing more — and when that turned up intermittently in
/// CI on one of two concurrent runs of the same commit, there was nothing in
/// the message to work from. A check that knows something and throws it away
/// makes the next occurrence cost as much as the first.
static GTK_THREAD: OnceLock<Result<gtk4::glib::ThreadPool, String>> = OnceLock::new();

/// Set to `skip` by a run that has no display and accepts not covering the
/// widget layer — a developer's laptop without an X server, say. CI sets up
/// Xvfb instead, so a missing display there is a bug in the workflow and
/// fails rather than passing quietly.
pub const SKIP_VARIABLE: &str = "SUITE_GTK_TESTS";

/// Split out from the environment read so the policy can be asserted
/// without writing a process-global variable — a test that set
/// `SUITE_GTK_TESTS` would be visible to every test running beside it.
fn value_opts_out(value: &str) -> bool {
    value.eq_ignore_ascii_case("skip")
}

fn skipping_is_allowed() -> bool {
    std::env::var(SKIP_VARIABLE).is_ok_and(|value| value_opts_out(&value))
}

/// What the display looked like when initialisation was attempted, for the
/// message. An unset `DISPLAY` and a set one that nothing is serving fail
/// identically inside GTK but need different fixes.
fn display_for_diagnosis() -> String {
    describe_display(std::env::var("DISPLAY").ok().as_deref())
}

/// Split out from the environment read for the same reason as
/// `value_opts_out`: a test that set `DISPLAY` would be visible to every
/// test running beside it, including the widget tests.
fn describe_display(value: Option<&str>) -> String {
    match value {
        Some("") => "DISPLAY set but empty".to_string(),
        Some(value) => format!("DISPLAY={value}"),
        None => "DISPLAY unset".to_string(),
    }
}

fn gtk_thread() -> Result<&'static gtk4::glib::ThreadPool, &'static String> {
    GTK_THREAD
        .get_or_init(|| {
            let pool = gtk4::glib::ThreadPool::exclusive(1)
                .map_err(|e| format!("could not start the GTK test thread: {e}"))?;
            let (tx, rx) = mpsc::channel();
            pool.push(move || {
                // The error, not a boolean: this is the one place that sees
                // why GTK refused.
                let _ = tx.send(gtk4::init().map_err(|e| e.to_string()));
            })
            .map_err(|e| format!("could not schedule GTK initialisation: {e}"))?;
            match rx.recv() {
                Ok(Ok(())) => Ok(pool),
                Ok(Err(reason)) => Err(format!("gtk::init failed: {reason}")),
                // The worker dropped the sender without reporting, which
                // means it died — a GTK abort rather than a refusal.
                Err(_) => Err(
                    "the GTK test thread exited without reporting; GTK aborted during \
                     initialisation rather than returning an error"
                        .to_string(),
                ),
            }
        })
        .as_ref()
}

/// Run `f` on the process's single GTK thread, propagating a panic inside `f`
/// as a failure of the calling test.
///
/// When GTK cannot initialise this **fails** the test, unless the run has
/// declared itself display-less by setting `SUITE_GTK_TESTS=skip`. A machine
/// with no display genuinely cannot run a widget test, but it has to be the
/// run that says so: defaulting to a silent pass is how these tests stopped
/// running in CI without anyone noticing. Coverage of the *logic* under
/// these widgets still belongs in the GTK-free core crates, where it runs
/// everywhere.
///
/// ```ignore
/// #[test]
/// fn widget_defaults() {
///     suite_common::gtk_test::run(|| {
///         let w = MyWidget::new();
///         assert_eq!(w.zoom(), 1.0);
///     });
/// }
/// ```
pub fn run<F>(f: F)
where
    F: FnOnce() + Send + panic::UnwindSafe + 'static,
{
    let pool = match gtk_thread() {
        Ok(pool) => pool,
        Err(reason) => {
            assert!(
                skipping_is_allowed(),
                "GTK could not be initialised, so this widget test could not run: \
                 {reason} ({display}). Give the run a display (`xvfb-run -a cargo \
                 test ...`), or set {SKIP_VARIABLE}=skip to declare this run \
                 display-less and skip the GTK widget tests deliberately.",
                display = display_for_diagnosis(),
            );
            eprintln!(
                "SKIP: GTK could not be initialised ({reason}, {}) and {SKIP_VARIABLE}=skip \
                 is set",
                display_for_diagnosis()
            );
            return;
        }
    };
    let (tx, rx) = mpsc::sync_channel(1);
    if pool.push(move || { let _ = tx.send(panic::catch_unwind(f)); }).is_err() {
        panic!("could not schedule work on the GTK test thread");
    }
    match rx.recv() {
        // Resume the panic on the *test's* thread so the harness reports this
        // test as failing, with the original message, instead of the worker
        // dying silently and the assertion being lost.
        Ok(Err(payload)) => panic::resume_unwind(payload),
        Ok(Ok(())) => {}
        Err(_) => panic!("the GTK test thread died without reporting a result"),
    }
}

/// Whether GTK is usable in this process, for tests that need to assert
/// something about the skip path itself.
pub fn is_available() -> bool {
    gtk_thread().is_ok()
}

/// Why GTK is unusable in this process, or `None` when it is usable. Exposed
/// so a diagnosis can be reported by whatever is in a position to report it,
/// rather than only inside a panic message.
pub fn unavailable_reason() -> Option<&'static str> {
    gtk_thread().err().map(String::as_str)
}

#[cfg(test)]
mod tests {
    /// The canary for #241: one clearly-named failure when a run has no
    /// display and did not say so, instead of every widget test quietly
    /// passing. This is what a CI job that forgets Xvfb now trips over.
    #[test]
    fn gtk_widget_tests_require_a_display_unless_the_run_opts_out() {
        assert!(
            super::is_available() || super::skipping_is_allowed(),
            "GTK could not be initialised and {}=skip is not set, so every GTK \
             widget test in this run would have been unable to run: {} ({}). \
             Give the run a display (`xvfb-run -a ...`) or opt out explicitly.",
            super::SKIP_VARIABLE,
            super::unavailable_reason().unwrap_or("no reason recorded"),
            super::display_for_diagnosis(),
        );
    }

    #[test]
    fn only_the_skip_value_opts_out() {
        assert!(super::value_opts_out("skip"));
        assert!(super::value_opts_out("SKIP"));
        // A truthy-looking value is not the opt-out: the variable names a
        // deliberate choice, and guessing at "1" or "true" would let a
        // stray environment quietly disable the widget layer again.
        for value in ["1", "true", "yes", "", "skipping"] {
            assert!(!super::value_opts_out(value), "value {value:?}");
        }
    }

    /// The three display states fail identically inside GTK and need
    /// different fixes, so the message has to tell them apart: an unset
    /// DISPLAY means "give the run a display", a set one means "the display
    /// you gave it is not answering".
    #[test]
    fn the_message_distinguishes_the_display_states() {
        assert_eq!(super::describe_display(None), "DISPLAY unset");
        assert_eq!(super::describe_display(Some("")), "DISPLAY set but empty");
        assert_eq!(super::describe_display(Some(":99")), "DISPLAY=:99");
    }

    /// When GTK is usable there is no reason to report, and when it is not
    /// there must be one — an empty diagnosis is the defect this replaced.
    #[test]
    fn unavailability_always_comes_with_a_reason() {
        match super::unavailable_reason() {
            None => assert!(super::is_available(), "no reason, but GTK is unavailable"),
            Some(reason) => {
                assert!(!super::is_available());
                assert!(!reason.trim().is_empty(), "unavailable with an empty reason");
            }
        }
    }

    /// Every call must land on the same thread, or widgets created by one test
    /// would be unusable from the next.
    #[test]
    fn all_work_runs_on_one_thread() {
        if !super::is_available() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        for _ in 0..4 {
            let tx = tx.clone();
            super::run(move || {
                let _ = tx.send(format!("{:?}", std::thread::current().id()));
            });
        }
        drop(tx);
        let ids: Vec<String> = rx.iter().collect();
        assert_eq!(ids.len(), 4, "every closure should have run");
        assert!(ids.windows(2).all(|w| w[0] == w[1]), "ran on several threads: {ids:?}");
    }

    /// A failing assertion inside the closure must fail the calling test —
    /// a harness that swallowed panics would report success for broken code.
    #[test]
    fn panics_propagate_to_the_calling_test() {
        if !super::is_available() {
            return;
        }
        let result = std::panic::catch_unwind(|| {
            super::run(|| panic!("boom"));
        });
        assert!(result.is_err(), "a panic inside run() must reach the caller");
    }
}
