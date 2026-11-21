// SPDX-License-Identifier: GPL-3.0-or-later
//
// crash_report.rs — a backtrace on stderr when the app dies of a signal,
// for the GUI journeys (#1192).
//
// The nightly stress campaign sees Tables vanish mid-journey about once in
// eighteen runs, with nothing on stderr: a native crash, not a Rust panic
// (a panic prints its message). The runner's core_pattern pipes cores to
// systemd-coredump, which a container cannot reach, and the crash has not
// reproduced locally. So the evidence has to come from the process itself:
// with GTK_OFFICE_CRASH_BACKTRACE set (the GUI harness sets it), a fatal
// signal prints its name and the faulting thread's backtrace, then the
// signal is re-raised with its default action, so the exit status and any
// core are what they would have been.
//
// Capturing a backtrace allocates, which is not async-signal-safe: if the
// crash is inside malloc it can deadlock instead. That is acceptable for a
// test-only diagnostic (the harness times out and kills it, the same
// failure it reports today), and it is why this is never on for users.

/// The variable that turns the report on.
pub const ENV: &str = "GTK_OFFICE_CRASH_BACKTRACE";

const SIGNALS: [libc::c_int; 5] = [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT];

/// Install the report if `GTK_OFFICE_CRASH_BACKTRACE` is set.
pub fn install() {
    if std::env::var_os(ENV).is_none() {
        return;
    }
    for sig in SIGNALS {
        // SAFETY: a plain sigaction with a handler of the right signature;
        // SA_RESETHAND restores the default action before the handler runs,
        // so the re-raise below terminates as the signal would have.
        // No SA_ONSTACK: symbolising the backtrace needs far more stack
        // than the alternate stack Rust's runtime installs (it overflowed
        // that and faulted again), so the report runs on the crashing
        // thread's own stack. A stack overflow therefore gets no report,
        // just the default action, as before.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_RESETHAND;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
}

fn name(sig: libc::c_int) -> &'static str {
    match sig {
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGBUS => "SIGBUS",
        libc::SIGILL => "SIGILL",
        libc::SIGFPE => "SIGFPE",
        libc::SIGABRT => "SIGABRT",
        _ => "signal",
    }
}

extern "C" fn handler(sig: libc::c_int, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
    use std::io::Write;
    // SAFETY: the kernel passes a valid siginfo_t to an SA_SIGINFO handler.
    let addr = unsafe { info.as_ref().map_or(std::ptr::null_mut(), |i| i.si_addr()) };
    let thread = std::thread::current();
    let report = format!(
        "\n=== fatal {} (address {:?}) in thread {} ===\n{}\n=== end of crash report ===\n",
        name(sig),
        addr,
        thread.name().unwrap_or("<unnamed>"),
        std::backtrace::Backtrace::force_capture(),
    );
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(report.as_bytes());
    let _ = err.flush();
    // SAFETY: re-raising with the default action restored (SA_RESETHAND).
    unsafe {
        libc::raise(sig);
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    /// A child that installs the report and then segfaults prints the
    /// report, and still dies of SIGSEGV (not a clean exit).
    #[test]
    fn a_segfault_prints_a_backtrace_and_still_dies_of_the_signal() {
        if std::env::var_os("CRASH_REPORT_CHILD").is_some() {
            super::install();
            // SAFETY: deliberately not safe — this child exists to crash.
            unsafe {
                libc::raise(libc::SIGSEGV);
            }
            unreachable!("SIGSEGV returned");
        }
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_report::tests::a_segfault_prints_a_backtrace_and_still_dies_of_the_signal", "--nocapture"])
            .env("CRASH_REPORT_CHILD", "1")
            .env(super::ENV, "1")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("=== fatal SIGSEGV"), "no report on stderr:\n{err}");
        assert!(err.contains("a_segfault_prints_a_backtrace"), "the backtrace names the crashing function:\n{err}");
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(out.status.signal(), Some(libc::SIGSEGV), "the process still died of the signal");
    }
}
