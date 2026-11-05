//! Allocations per 10 ms frame for each candidate, counted by a global
//! allocator. Needs the local weights:
//! `cargo test --release -p relay-benches --features neural,sonora,nnnoiseless --test neural_allocs -- --ignored --nocapture`
#![cfg(feature = "neural")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use relay_audio::dsp::DspLimits;
use relay_audio::format::{BlockTiming, FRAME_SAMPLES};
use relay_benches::audio::runner::{default_backends, make_candidate};

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards every call to the system allocator unchanged.
unsafe impl GlobalAlloc for Counting {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
		// SAFETY: same arguments, same contract as the caller's.
		unsafe { System.alloc(layout) }
	}
	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		// SAFETY: same arguments, same contract as the caller's.
		unsafe { System.dealloc(ptr, layout) }
	}
	unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
		ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
		// SAFETY: same arguments, same contract as the caller's.
		unsafe { System.realloc(ptr, layout, new) }
	}
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
#[ignore = "needs the local weights"]
fn allocations_per_frame() {
	const FRAMES: u64 = 1_000;
	for name in default_backends() {
		let mut candidate = make_candidate(&name).unwrap();
		let limits = DspLimits {
			max_block_samples: FRAME_SAMPLES,
			max_scratch_bytes: candidate.ns.descriptor().max_scratch_bytes,
		};
		candidate.ns.prepare(&limits).unwrap();
		let mut out = vec![0.0_f32; 2 * FRAME_SAMPLES];
		let silence = vec![0.0_f32; FRAME_SAMPLES];
		let mut state = 7_u32;
		let mut frame = |buf: &mut [f32]| {
			for s in buf {
				state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
				*s = (state >> 8) as f32 / (1_u32 << 24) as f32 * 0.2 - 0.1;
			}
		};
		let mut input = vec![0.0_f32; FRAME_SAMPLES];
		let mut step = |n: u64, candidate: &mut relay_benches::audio::runner::Candidate| {
			frame(&mut input);
			if candidate.feed_render {
				candidate.ns.push_render(&silence).unwrap();
			}
			let timing = BlockTiming {
				generation: 0,
				first_sample: n * 480,
			};
			candidate.ns.process_into(&input, &mut out, timing).unwrap();
		};
		for n in 0..100 {
			step(n, &mut candidate);
		}
		let before = ALLOCATIONS.load(Ordering::Relaxed);
		for n in 100..100 + FRAMES {
			step(n, &mut candidate);
		}
		let used = ALLOCATIONS.load(Ordering::Relaxed) - before;
		println!(
			"ALLOCS {name} {:.2} per frame ({used} over {FRAMES})",
			used as f64 / FRAMES as f64
		);
	}
}
