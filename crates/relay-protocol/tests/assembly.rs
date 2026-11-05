use proptest::prelude::*;
use relay_protocol::AssemblyError;
use relay_protocol::assembly::FrameAssembler;

#[test]
fn eight_ms_into_ten_ms_has_no_common_multiple_batching() {
	let mut asm = FrameAssembler::<i16>::new(480, 384);
	let mut out = [0i16; 480];
	let mut counter = 0i16;
	let mut frames_per_push = Vec::new();
	for _ in 0..10 {
		let chunk: Vec<i16> = (0..384)
			.map(|_| {
				counter = counter.wrapping_add(1);
				counter
			})
			.collect();
		asm.push(&chunk).unwrap();
		let mut frames = 0;
		while asm.pop_frame(&mut out) {
			frames += 1;
		}
		frames_per_push.push(frames);
		assert!(asm.buffered() < 480);
	}
	// 5 pushes of 8 ms == 4 frames of 10 ms, delivered as early as possible.
	assert_eq!(frames_per_push, [0, 1, 1, 1, 1, 0, 1, 1, 1, 1]);
}

#[test]
fn preserves_sample_order_across_wrap() {
	let mut asm = FrameAssembler::<u32>::new(5, 4);
	let mut out = [0; 5];
	let mut next = 0;
	let mut expected = 0;
	for _ in 0..50 {
		let chunk: Vec<u32> = (next..next + 4).collect();
		next += 4;
		asm.push(&chunk).unwrap();
		while asm.pop_frame(&mut out) {
			for v in out {
				assert_eq!(v, expected);
				expected += 1;
			}
		}
	}
}

#[test]
fn overflow_is_an_error() {
	let mut asm = FrameAssembler::<u8>::new(4, 2);
	asm.push(&[1, 2]).unwrap();
	assert_eq!(
		asm.push(&[1, 2, 3, 4]),
		Err(AssemblyError::Overflow {
			pushed: 4,
			capacity: 5
		})
	);
}

proptest! {
	#[test]
	fn arbitrary_chunking_is_lossless(
		frame in 1usize..64,
		chunks in proptest::collection::vec(0usize..64, 1..60),
	) {
		let mut asm = FrameAssembler::<u32>::new(frame, 63);
		let mut out = vec![0; frame];
		let (mut next, mut expected) = (0u32, 0u32);
		for len in chunks {
			let data: Vec<u32> = (next..next + len as u32).collect();
			next += len as u32;
			asm.push(&data).unwrap();
			while asm.pop_frame(&mut out) {
				for v in &out {
					prop_assert_eq!(*v, expected);
					expected += 1;
				}
			}
			prop_assert!(asm.buffered() < frame);
		}
		prop_assert_eq!(expected as usize + asm.buffered(), next as usize);
	}
}
