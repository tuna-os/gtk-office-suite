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

// ── What is known about the remaining intermittent failure ───────────
//
// One failure mode is still open, and this records what has been ruled out
// so the next occurrence does not start from nothing. Symptom: a single
// Letters widget test fails with GTK refusing to initialise while every
// other widget test in the same run initialises fine. Seven occurrences,
// seven *different* tests, always with `DISPLAY=:0` and a socket present —
// which
// is the case `describe_display_and_server` was added to name, and it names
// it: refused by a live display, not handed a missing one.
//
// That it is the environment and not the code is demonstrated, not
// inferred. Two runs of one commit — documentation-only, on a branch whose
// previous head had a clean suite — failed in different places, and each
// passed the test the other failed:
//
//     run 1:  FAIL (144/915) bridge::tests::document_round_trips_through_buffer
//     run 2:  FAIL (170/915) doc_tab::tests::header_and_footer_reach_the_page_view
//             PASS (148/915) bridge::tests::document_round_trips_through_buffer
//
// No defect in the code under test can do that.
//
// An earlier version of this note claimed the failures were all
// `letters::bridge` tests in a band of "118 to 145 of some 900", and
// reasoned from the band that the cause does not build up over a run. The
// sixth occurrence is `letters::doc_tab` at 170, and the band turned out to
// be an artifact of the sample: Letters' widget tests *occupy* roughly
// 118-170 of this workspace's run order, so a failure among them lands
// there whatever causes it. The band described where the candidates are,
// not when the failure happens, and the position argument built on it is
// withdrawn.
//
// Ruled out, with the evidence, because both are the obvious guesses:
//
//   * **Connection accumulation.** nextest runs a process per test, so each
//     widget test opens its own X connection, and a server that ran out of
//     client slots would explain one failure near the end of a long run.
//     Measured: eight iterations of the widget tests against one persistent
//     display, roughly 750 GTK inits, zero refusals. That is evidence about
//     a long-lived display, and it is the only evidence here that is — the
//     position argument that used to stand beside it is withdrawn above.
//     `Xvfb -maxclients n` is the lever if a client-slot limit is ever
//     shown to be the cause, and a nextest test group with
//     `max-threads = 1` over the Letters widget tests is the lever for
//     reducing how many X connections are open at once. The second is not
//     a masking fix — it changes scheduling, not assertions, so unlike the
//     retry it cannot turn a display-less run green — but it costs run
//     time and rests on a hypothesis nothing here has confirmed, so it
//     should be a deliberate decision rather than a reflex.
//
//   * **Retrying `gtk4::init()`.** This is the tempting fix and it is
//     actively unsafe. `gtk4::init()` is *not* idempotent after a failure:
//     called a second time it returns `Ok` without a usable display. A
//     retry was implemented, passed the unit tests, and passed two
//     mutations — and then a canary widget test run with `DISPLAY=:77`
//     (nothing serving it) went from `FAILED` to `ok. 1 passed`. That is
//     exactly the masking #241 exists to prevent, applied to every
//     display-less run rather than to one flake. Do not reintroduce
//     `init_with_retries`.
//
// Solved (#652, fixed in #1054). Xvfb resets the server whenever its last
// client disconnects, and refuses connections while it does. The test
// runner opens a fresh connection for each test process, so one widget test
// per run sometimes found the display mid-reset and had its connection
// refused. Measured locally: 13 of 400 connections refused without
// `-noreset`, 0 of 400 with it. Every Xvfb launcher now passes `-noreset`,
// and a test keeps the flag there. If this symptom returns, check first
// that the display in use was started with `-noreset`.

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
    describe_display_and_server(std::env::var("DISPLAY").ok().as_deref(), server_socket_exists)
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

/// Whether a local X server has a socket for this display number.
///
/// Not a connection attempt: opening one from inside a failing test would
/// add its own failure mode to the diagnosis. The socket's presence is
/// enough to answer the question the message could not previously answer.
fn server_socket_exists(number: &str) -> bool {
    std::path::Path::new(&format!("/tmp/.X11-unix/X{number}")).exists()
}

/// The display *and* whether anything is serving it.
///
/// `describe_display` alone could not tell those apart, and the difference
/// decides where to look. A widget test failing with `DISPLAY=:0` reads like
/// a job that forgot Xvfb — but it is also what a job whose Xvfb is running
/// and refused one connection looks like, and that happened: a lane where
/// 140 of 141 widget tests initialised GTK on `:0` and one did not. The
/// first is a missing display, the second is not, and the message used to
/// describe both the same way.
///
/// The probe is injected so this stays a pure function: a test that created
/// or removed a socket under `/tmp/.X11-unix` would be doing it to every
/// other test in the process.
fn describe_display_and_server(
    value: Option<&str>,
    socket_exists: impl Fn(&str) -> bool,
) -> String {
    let described = describe_display(value);
    let Some(display) = value.filter(|v| !v.is_empty()) else {
        return described;
    };
    // "host:0.0" — the part before the colon is a host (empty for a local
    // display), and the screen suffix after the dot is not part of the
    // server's socket name.
    let Some((host, rest)) = display.split_once(':') else {
        return described;
    };
    if !host.is_empty() && host != "localhost" {
        return format!("{described}, a remote display this check does not probe");
    }
    let number = rest.split('.').next().unwrap_or(rest);
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
        return described;
    }
    if socket_exists(number) {
        format!(
            "{described}, which an X server *is* serving — so GTK was refused by a \
             live display rather than handed a missing one"
        )
    } else {
        format!("{described}, which nothing is serving")
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
                 {reason} ({display}). Give the run a display \
                 (`scripts/with-display.sh cargo test ...`, which waits for the \
                 server to accept connections), or set {SKIP_VARIABLE}=skip to \
                 declare this run display-less and skip the GTK widget tests \
                 deliberately.",
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
             Give the run a display (`scripts/with-display.sh ...`) or opt out \
             explicitly.",
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

    /// The message has to separate "no display" from "a display that
    /// refused us", because a lane where 140 of 141 widget tests
    /// initialised GTK on `:0` and one did not reads exactly like a job
    /// that forgot Xvfb, and is not one.
    #[test]
    fn the_message_says_whether_anything_is_serving_the_display() {
        let serving = |_: &str| true;
        let empty = |_: &str| false;

        let live = super::describe_display_and_server(Some(":0"), serving);
        assert!(live.starts_with("DISPLAY=:0,"), "{live}");
        assert!(live.contains("refused"), "{live}");

        let dead = super::describe_display_and_server(Some(":0"), empty);
        assert_eq!(dead, "DISPLAY=:0, which nothing is serving");
    }

    /// The screen suffix is not part of the socket name, and the host part
    /// means there is no local socket to look for at all. Getting either
    /// wrong would report "nothing is serving" for a display that is.
    #[test]
    fn the_probe_reads_the_display_the_way_x_does() {
        // RefCell because the probe is taken as `Fn`: a closure that pushed
        // straight into a local Vec would only be `FnMut`.
        let asked = std::cell::RefCell::new(Vec::new());
        super::describe_display_and_server(Some(":12.0"), |number: &str| {
            asked.borrow_mut().push(number.to_string());
            true
        });
        assert_eq!(
            asked.into_inner(),
            vec!["12".to_string()],
            "the screen suffix is not part of the display number",
        );

        let remote = super::describe_display_and_server(Some("otherhost:0"), |_| false);
        assert!(remote.contains("remote"), "{remote}");
        assert!(!remote.contains("nothing is serving"), "{remote}");
    }

    /// The states with nothing to probe keep their old wording rather than
    /// gaining a claim about a server.
    #[test]
    fn a_display_with_no_number_is_described_but_not_probed() {
        for value in [None, Some(""), Some("bogus"), Some(":")] {
            let described = super::describe_display_and_server(value, |_| true);
            assert!(
                !described.contains("serving") && !described.contains("refused"),
                "{value:?} -> {described}",
            );
        }
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
