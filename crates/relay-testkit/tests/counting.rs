use relay_testkit::{CountingAlloc, allocations_in, total_allocations};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn counts_allocations_made_inside_the_closure() {
	let n = allocations_in(|| {
		std::hint::black_box(Vec::<u8>::with_capacity(64));
	});
	assert_eq!(n, 1);
}

#[test]
fn allocation_free_code_counts_zero() {
	let mut sum = 0u64;
	let n = allocations_in(|| {
		for i in 0..100u64 {
			sum += std::hint::black_box(i);
		}
	});
	assert_eq!(n, 0);
	assert_eq!(sum, 4950);
}

#[test]
fn other_threads_do_not_disturb_the_count() {
	let noisy = std::thread::spawn(|| {
		for _ in 0..2000 {
			std::hint::black_box(Box::new([0u8; 128]));
		}
	});
	let n = allocations_in(|| {
		std::hint::black_box(7u32.wrapping_mul(3));
		std::thread::yield_now();
	});
	noisy.join().unwrap();
	assert_eq!(n, 0);
}

#[test]
fn counting_is_disarmed_outside_the_closure() {
	let _ = allocations_in(|| {});
	drop(std::hint::black_box(Vec::<u8>::with_capacity(8)));
	assert_eq!(allocations_in(|| {}), 0);
}

#[test]
fn total_counts_every_allocation() {
	let before = total_allocations();
	drop(std::hint::black_box(Box::new(1u64)));
	assert!(total_allocations() > before);
}

#[test]
fn nested_measurements_preserve_the_outer_count() {
	let outer = allocations_in(|| {
		drop(std::hint::black_box(Box::new(1u64)));
		assert_eq!(
			allocations_in(|| {
				drop(std::hint::black_box(Box::new(2u64)));
			}),
			1
		);
		drop(std::hint::black_box(Box::new(3u64)));
	});
	assert_eq!(outer, 3);
}

#[test]
fn panicking_scope_restores_an_enclosing_measurement() {
	let outer = allocations_in(|| {
		let result = std::panic::catch_unwind(|| allocations_in(|| panic!("measurement probe")));
		assert!(result.is_err());
		let before = allocations_in(|| {
			drop(std::hint::black_box(Box::new(1u64)));
		});
		assert_eq!(before, 1);
	});
	assert!(outer >= 1);
	assert_eq!(allocations_in(|| {}), 0);
}
