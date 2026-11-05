//! Counting global allocator for "this path must not allocate" assertions.
//!
//! A test binary installs it once:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: relay_testkit::CountingAlloc = relay_testkit::CountingAlloc;
//! ```
//!
//! Counting is per thread and armed only inside [`allocations_in`], so other test threads
//! running concurrently cannot disturb an assertion about the audio callback.
//! [`total_allocations`] is the process-wide counter for single-threaded measurements.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
	static ARMED: Cell<bool> = const { Cell::new(false) };
	static COUNT: Cell<u64> = const { Cell::new(0) };
}

static TOTAL: AtomicU64 = AtomicU64::new(0);

/// Forwards to the system allocator and counts allocations (not frees).
pub struct CountingAlloc;

fn note() {
	TOTAL.fetch_add(1, Ordering::Relaxed);
	ARMED.with(|armed| {
		if armed.get() {
			COUNT.with(|c| c.set(c.get() + 1));
		}
	});
}

// SAFETY: every method forwards to the system allocator unchanged; the bookkeeping only
// touches const-initialised, destructor-free thread locals and an atomic.
unsafe impl GlobalAlloc for CountingAlloc {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		note();
		// SAFETY: same contract as the caller's.
		unsafe { System.alloc(layout) }
	}

	unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
		note();
		// SAFETY: same contract as the caller's.
		unsafe { System.alloc_zeroed(layout) }
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		// SAFETY: ptr/layout come from this allocator's `alloc`.
		unsafe { System.dealloc(ptr, layout) }
	}

	unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
		note();
		// SAFETY: ptr/layout come from this allocator's `alloc`.
		unsafe { System.realloc(ptr, layout, new_size) }
	}
}

/// Allocations made on this thread while `f` runs.
pub fn allocations_in(f: impl FnOnce()) -> u64 {
	COUNT.with(|c| c.set(0));
	ARMED.with(|a| a.set(true));
	f();
	ARMED.with(|a| a.set(false));
	COUNT.with(Cell::get)
}

/// Allocations by every thread since the process started (take two readings and subtract).
pub fn total_allocations() -> u64 {
	TOTAL.load(Ordering::Relaxed)
}
