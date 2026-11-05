//! Counting global allocator shared by allocation tests.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
	static ARMED: Cell<bool> = const { Cell::new(false) };
	static COUNT: Cell<u64> = const { Cell::new(0) };
}

fn note() {
	ARMED.with(|armed| {
		if armed.get() {
			COUNT.with(|c| c.set(c.get() + 1));
		}
	});
}

// SAFETY: every method forwards to the system allocator unchanged; the
// bookkeeping only touches const-initialised, destructor-free thread locals.
unsafe impl GlobalAlloc for Counting {
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

#[global_allocator]
static ALLOC: Counting = Counting;

/// Allocations made on this thread while `f` runs.
pub fn allocations_in(f: impl FnOnce()) -> u64 {
	COUNT.with(|c| c.set(0));
	ARMED.with(|a| a.set(true));
	f();
	ARMED.with(|a| a.set(false));
	COUNT.with(Cell::get)
}
