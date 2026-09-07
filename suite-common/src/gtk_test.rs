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

use std::panic;
use std::sync::mpsc;
use std::sync::OnceLock;

/// Shared worker. `None` once GTK has been found uninitialisable (headless
/// with no usable display), so every later call skips instead of retrying an
/// init that cannot succeed.
static GTK_THREAD: OnceLock<Option<gtk4::glib::ThreadPool>> = OnceLock::new();

fn gtk_thread() -> Option<&'static gtk4::glib::ThreadPool> {
    GTK_THREAD
        .get_or_init(|| {
            let pool = gtk4::glib::ThreadPool::exclusive(1).ok()?;
            let (tx, rx) = mpsc::channel();
            pool.push(move || {
                let _ = tx.send(gtk4::init().is_ok());
            })
            .ok()?;
            match rx.recv().ok()? {
                true => Some(pool),
                false => None,
            }
        })
        .as_ref()
}

/// Run `f` on the process's single GTK thread, propagating a panic inside `f`
/// as a failure of the calling test.
///
/// When GTK cannot initialise the closure is skipped and the test passes. That
/// is deliberate: these are widget tests, a machine with no display cannot run
/// them, and the alternative — every GTK test failing on such a machine — is
/// noise that trains people to ignore the suite. Coverage of the *logic* under
/// these widgets belongs in the GTK-free core crates, where it runs everywhere.
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
    let Some(pool) = gtk_thread() else {
        eprintln!("SKIP: GTK could not be initialised (no display)");
        return;
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
    gtk_thread().is_some()
}

#[cfg(test)]
mod tests {
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
