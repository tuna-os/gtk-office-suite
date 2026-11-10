// idle.rs — one pending idle callback per job, not one per change.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A buffer's `changed` fires once per keystroke (more for a paste). A job
// that reads the whole document in an idle (spell check, word count) and
// queues an idle per signal runs once per keystroke when the main loop
// catches up: a burst of typing in a long document queued that many
// whole-document scans (#1990). `IdleOnce` keeps the one pending idle and
// replaces it, as the relayout in doc_tab.rs does.

use gtk4::glib;
use std::cell::RefCell;
use std::rc::Rc;

/// A job run at most once per main-loop idle, however many times it was
/// scheduled before then.
#[derive(Clone, Default)]
pub struct IdleOnce {
    pending: Rc<RefCell<Option<glib::JoinHandle<()>>>>,
}

impl IdleOnce {
    /// Run `job` when this thread's main loop is next idle, in place of
    /// any job this already had waiting.
    pub fn schedule(&self, job: impl FnOnce() + 'static) {
        // Aborting a job that already ran does nothing, so no bookkeeping
        // has to follow the job's own end.
        if let Some(waiting) = self.pending.borrow_mut().take() {
            waiting.abort();
        }
        let handle = glib::MainContext::ref_thread_default().spawn_local_with_priority(glib::Priority::DEFAULT_IDLE, async move { job() });
        *self.pending.borrow_mut() = Some(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn drain(ctx: &glib::MainContext) {
        while ctx.iteration(false) {}
    }

    #[test]
    fn a_burst_of_schedules_runs_the_job_once_with_the_last_one() {
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let idle = IdleOnce::default();
            let runs = Rc::new(Cell::new(0));
            let last = Rc::new(Cell::new(0));
            for i in 1..=50 {
                let (runs, last) = (runs.clone(), last.clone());
                idle.schedule(move || {
                    runs.set(runs.get() + 1);
                    last.set(i);
                });
            }
            drain(&ctx);
            assert_eq!((runs.get(), last.get()), (1, 50));
            // Scheduling after it ran works again, and a job that schedules
            // the next one does not trip over its own finished source.
            let again = runs.clone();
            let (inner, idle2) = (runs.clone(), idle.clone());
            idle.schedule(move || {
                again.set(again.get() + 1);
                idle2.schedule(move || inner.set(inner.get() + 1));
            });
            drain(&ctx);
            assert_eq!(runs.get(), 3);
        })
        .unwrap();
    }
}
