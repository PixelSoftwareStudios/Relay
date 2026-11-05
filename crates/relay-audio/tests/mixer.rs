use relay_audio::mixer::{MAX_GAIN, Mixer};

fn mix_once(m: &mut Mixer, inputs: &[(relay_audio::mixer::CallSlot, Vec<f32>)]) -> Vec<f32> {
	m.begin();
	for (slot, data) in inputs {
		m.add(*slot, data).unwrap();
	}
	let mut out = vec![0.0; m.frame_len()];
	m.finish(&mut out).unwrap();
	out
}

#[test]
fn sums_calls_with_independent_gain() {
	let mut m = Mixer::new(480);
	let (a, b) = (m.open_slot().unwrap(), m.open_slot().unwrap());
	m.set_call_gain(b, 0.5).unwrap();
	// Let the gain ramp finish, then compare.
	mix_once(&mut m, &[(a, vec![0.1; 480]), (b, vec![0.2; 480])]);
	let out = mix_once(&mut m, &[(a, vec![0.1; 480]), (b, vec![0.2; 480])]);
	assert!(out.iter().all(|v| (v - 0.2).abs() < 1e-5));
}

#[test]
fn master_gain_and_cap() {
	let mut m = Mixer::new(8);
	let a = m.open_slot().unwrap();
	m.set_master_gain(100.0).unwrap();
	m.set_master_gain(0.5).unwrap();
	mix_once(&mut m, &[(a, vec![0.4; 8])]);
	let out = mix_once(&mut m, &[(a, vec![0.4; 8])]);
	assert!((out[0] - 0.2).abs() < 1e-6);
	m.set_call_gain(a, 100.0).unwrap();
	m.set_master_gain(1.0).unwrap();
	mix_once(&mut m, &[(a, vec![0.1; 8])]);
	let out = mix_once(&mut m, &[(a, vec![0.1; 8])]);
	assert!(
		(out[0] - 0.1 * MAX_GAIN).abs() < 1e-6,
		"gain must cap at MAX_GAIN"
	);
	assert!(m.set_call_gain(a, f32::NAN).is_err());
}

#[test]
fn limiter_bounds_output() {
	let mut m = Mixer::new(480);
	let (a, b) = (m.open_slot().unwrap(), m.open_slot().unwrap());
	let loud = vec![0.9; 480];
	for _ in 0..5 {
		let out = mix_once(&mut m, &[(a, loud.clone()), (b, loud.clone())]);
		assert!(out.iter().all(|v| v.abs() <= 1.0));
	}
	assert!(m.limiter().limited_samples() > 0);
}

#[test]
fn corrupt_input_does_not_poison_the_mix() {
	let mut m = Mixer::new(4);
	let (a, b) = (m.open_slot().unwrap(), m.open_slot().unwrap());
	let out = mix_once(
		&mut m,
		&[
			(a, vec![f32::NAN, f32::INFINITY, 0.0, 0.0]),
			(b, vec![0.25; 4]),
		],
	);
	assert!(out.iter().all(|v| v.is_finite() && (v - 0.25).abs() < 1e-6));
	assert_eq!(m.rejected_samples(), 2);
}

#[test]
fn slots_are_bounded_and_scratch_is_cleared() {
	let mut m = Mixer::new(4);
	let slots: Vec<_> = (0..relay_audio::MAX_CALLS)
		.map(|_| m.open_slot().unwrap())
		.collect();
	assert!(m.open_slot().is_none());
	m.close_slot(slots[0]);
	assert!(m.open_slot().is_some());
	let out = mix_once(&mut m, &[]);
	assert!(out.iter().all(|v| *v == 0.0));
	m.begin();
	assert!(m.add(slots[1], &[0.0; 3]).is_err());
}
