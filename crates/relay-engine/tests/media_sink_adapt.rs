//! RT-06, RT-07 and RT-09: the playout target follows the device's callback
//! size, a stall's extra latency is shed quickly, and the drift thresholds mean
//! the same time at every device rate. Event-ordered simulation with exact
//! clocks: no hardware clock was measured.

use relay_audio::playback::playback_pair;
use relay_engine::media::RenderSink;

struct Run {
	/// Silence inserted by the device callback during the measured window.
	underrun_frames: u64,
	/// Fill just before a mixer write, averaged over the last `tail_s` seconds.
	tail_fill: f64,
	/// Fill target the sink ended with.
	target: usize,
	shed: u64,
	/// Drift correction applied at the end, ppm.
	final_ppm: f64,
	/// Mean pre-write fill per second of simulated time.
	per_second: Vec<f64>,
}

struct Scenario {
	rate: u32,
	callback_frames: usize,
	device_ppm: f64,
	seconds: usize,
	track_callbacks: bool,
	/// Extra 10 ms frames pushed in at once at the given second.
	burst: Option<(usize, usize)>,
}

fn simulate(s: &Scenario) -> Run {
	let ring = (s.rate as usize * 40 / 1000).max(4096 * 2 + s.rate as usize / 100);
	let (mut callback, writer, shared) = playback_pair(1, ring);
	let mut sink = RenderSink::new(writer, s.rate, 20).unwrap();
	if s.track_callbacks {
		sink = sink.track_callbacks(std::sync::Arc::clone(&shared));
	}
	let frame = [0.1_f32; 480];
	let mut out = vec![0.0_f32; s.callback_frames];
	let device_period =
		s.callback_frames as f64 / (f64::from(s.rate) * (1.0 + s.device_ppm * 1e-6));
	let (mut tick, mut pull) = (0_u64, 1_u64);
	let measure_from = (s.seconds as f64 - 60.0).max(0.0);
	let mut under_at_start = None;
	let mut fills = vec![(0.0_f64, 0_u32); s.seconds + 1];
	// The worker leads the device by the target fill.
	for _ in 0..2 {
		sink.write(&frame).unwrap();
	}
	loop {
		let (t_tick, t_pull) = (tick as f64 * 0.01, pull as f64 * device_period);
		if t_tick.min(t_pull) >= s.seconds as f64 {
			break;
		}
		if t_tick <= t_pull {
			if let Some((at, frames)) = s.burst
				&& tick == (at * 100) as u64
			{
				for _ in 0..frames {
					sink.write(&frame).unwrap();
				}
			}
			let fill = sink.occupancy();
			let second = t_tick as usize;
			fills[second].0 += fill as f64;
			fills[second].1 += 1;
			sink.write(&frame).unwrap();
			tick += 1;
		} else {
			if under_at_start.is_none() && t_pull >= measure_from {
				under_at_start = Some(shared.underrun_frames());
			}
			callback.process(&mut out);
			pull += 1;
		}
	}
	let per_second: Vec<f64> = fills
		.iter()
		.map(|(sum, n)| if *n == 0 { 0.0 } else { sum / f64::from(*n) })
		.collect();
	let tail = &per_second[per_second.len().saturating_sub(11)..per_second.len() - 1];
	Run {
		underrun_frames: shared.underrun_frames() - under_at_start.unwrap_or(0),
		tail_fill: tail.iter().sum::<f64>() / tail.len() as f64,
		target: sink.target(),
		shed: sink.shed_frames(),
		final_ppm: sink.drift_ppm(),
		per_second,
	}
}

fn scenario(rate: u32, callback_frames: usize) -> Scenario {
	Scenario {
		rate,
		callback_frames,
		device_ppm: 0.0,
		seconds: 100,
		track_callbacks: true,
		burst: None,
	}
}

#[test]
fn a_large_device_callback_raises_the_target_and_ends_the_underruns() {
	// 42.7 ms callbacks at 48 kHz: the review's structural-underrun case.
	let fixed = simulate(&Scenario {
		track_callbacks: false,
		seconds: 180,
		..scenario(48_000, 2_048)
	});
	assert!(
		fixed.underrun_frames > 0,
		"a fixed 20 ms target must underrun with 42.7 ms callbacks (baseline), got {}",
		fixed.underrun_frames
	);
	let adaptive = simulate(&Scenario {
		seconds: 180,
		..scenario(48_000, 2_048)
	});
	assert_eq!(adaptive.underrun_frames, 0, "target {}", adaptive.target);
	assert!(
		adaptive.target >= 2_048 * 3 / 2,
		"target {} must cover 1.5 callbacks",
		adaptive.target
	);
}

#[test]
fn small_callbacks_keep_the_configured_target() {
	let run = simulate(&scenario(48_000, 480));
	assert_eq!(run.target, 960, "20 ms at 48 kHz");
	assert_eq!(run.underrun_frames, 0);
}

#[test]
fn a_stalls_extra_latency_is_shed_in_seconds_not_minutes() {
	// The same run with and without 40 ms of extra queued audio at t = 40 s;
	// comparing the two removes the loop's own start-up transient.
	let calm = simulate(&scenario(48_000, 480));
	let stalled = simulate(&Scenario {
		burst: Some((40, 4)),
		..scenario(48_000, 480)
	});
	let extra = |second: usize| stalled.per_second[second] - calm.per_second[second];
	assert!(
		extra(40) > 400.0,
		"the burst must be visible: {}",
		extra(40)
	);
	// Three seconds later most of it is gone; before the fix it took ~2 minutes.
	assert!(extra(43) < 720.0, "still {} samples deeper", extra(43));
	assert!(stalled.shed > 0, "shedding is what removed it");
	// No overshoot into starvation afterwards, and no lingering excess.
	assert_eq!(stalled.underrun_frames, 0);
	assert!(extra(70).abs() < 720.0, "{} at 70 s", extra(70));
	assert!(extra(95).abs() < 720.0, "{} at 95 s", extra(95));
}

#[test]
fn drift_thresholds_hold_at_a_high_device_rate() {
	// 192 kHz, 21 ms callbacks, +300 ppm device: the loop settled at 195 ppm
	// and inserted 14.7 ms of silence before thresholds were scaled.
	let run = simulate(&Scenario {
		device_ppm: 300.0,
		..scenario(192_000, 4_032)
	});
	assert_eq!(run.underrun_frames, 0, "target {}", run.target);
	assert!(
		run.final_ppm > 250.0,
		"the loop settled at {} ppm for a +300 ppm device",
		run.final_ppm
	);
}
