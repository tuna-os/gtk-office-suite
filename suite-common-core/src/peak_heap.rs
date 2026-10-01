//! Peak heap measurement for the memory budgets (#1208).
//!
//! performance-accessibility.md row 2 asks for peak memory beside p50/p95
//! latency. Resident set size is the wrong instrument for a test: it counts
//! whatever the allocator kept from earlier work, and the harness's own
//! threads share it. This counts what the program asks for instead. A test
//! binary installs [`CountingAlloc`] as its global allocator and wraps each
//! operation in [`peak_during`], which reports the most heap the operation
//! held at once above what was live when it started.
//!
//! The counters are process-wide, so a binary using this should hold one
//! test: the harness runs tests on parallel threads, and a neighbour's
//! allocations would land in the measurement.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// The system allocator, counting live and peak bytes.
pub struct CountingAlloc;

fn grew(bytes: usize) {
    let now = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

// SAFETY: every call is forwarded unchanged to `System`; the counters only
// observe sizes and never touch the memory.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                grew(new_size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}

/// Run `operation` and return its result with the most heap it held at
/// once, in bytes, above what was live when it started. Only meaningful in
/// a binary whose global allocator is [`CountingAlloc`]; elsewhere it
/// reports 0.
pub fn peak_during<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let out = operation();
    (out, PEAK.load(Ordering::Relaxed).saturating_sub(base))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The library's own test binary does not install the allocator, so
    // only the arithmetic of `peak_during` is checked here; the counting
    // itself is exercised by the crates' peak_memory tests.
    #[test]
    fn without_the_allocator_installed_nothing_is_counted() {
        let (v, peak) = peak_during(|| vec![0u8; 1 << 20]);
        assert_eq!(v.len(), 1 << 20);
        assert_eq!(peak, 0);
    }
}
