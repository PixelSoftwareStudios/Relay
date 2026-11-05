mod common;

use common::{Arrival, FRAME, Sim, steady};
use relay_protocol::jitter::{ConcealCause, InsertOutcome, JitterConfig, Playout, SkipReason};

const CFG: JitterConfig = JitterConfig::BALANCED;

#[test]
fn steady_stream_plays_in_order_after_prefill() {
	let mut sim = Sim::new(CFG);
	sim.run(&steady(100), 120);
	assert_eq!(sim.played(), (0..100).collect::<Vec<_>>());
	assert_eq!(sim.log[0], Playout::Skip(SkipReason::Prefill));
	let stats = sim.buf.stats();
	assert_eq!(
		(
			sim.concealed_in_body(),
			stats.late_drop,
			stats.network_missing
		),
		(0, 0, 0)
	);
}

#[test]
fn survives_sequence_and_timestamp_wrap() {
	let mut sim = Sim::with_bases(CFG, 65_500, u32::MAX - 480 * 40);
	sim.run(&steady(200), 220);
	assert_eq!(sim.played(), (0..200).collect::<Vec<_>>());
	assert_eq!(sim.buf.stats().network_missing, 0);
}

#[test]
fn reordered_packets_within_margin_play_in_order() {
	let mut arrivals = steady(60);
	arrivals[10].tick = 11;
	arrivals[11].tick = 10;
	arrivals[30].tick = 32;
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 80);
	assert_eq!(sim.played(), (0..60).collect::<Vec<_>>());
	assert_eq!(sim.buf.stats().late_drop, 0);
}

#[test]
fn duplicates_are_counted_once_and_not_replayed() {
	let mut arrivals = steady(30);
	arrivals.push(Arrival {
		tick: 12,
		index: 12,
		ts_skip: 0,
	});
	arrivals.push(Arrival {
		tick: 13,
		index: 3,
		ts_skip: 0,
	});
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 50);
	assert_eq!(sim.played(), (0..30).collect::<Vec<_>>());
	assert_eq!(sim.buf.stats().duplicate, 2);
}

#[test]
fn single_loss_is_concealed_then_resumes() {
	let arrivals: Vec<_> = steady(40).into_iter().filter(|a| a.index != 10).collect();
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 60);
	let concealed = sim
		.body()
		.iter()
		.filter(|p| matches!(p, Playout::Conceal(c) if c.cause == ConcealCause::Loss))
		.count();
	assert_eq!(concealed, 1);
	let lost: Vec<_> = sim
		.log
		.iter()
		.filter_map(|p| match p {
			Playout::Play(p) if p.lost_before > 0 => Some((p.payload, p.lost_before)),
			_ => None,
		})
		.collect();
	assert_eq!(lost, [(11, 1)]);
	assert_eq!(sim.buf.stats().network_missing, 1);
}

#[test]
fn burst_loss_fades_then_interrupts_and_never_plays_backlog() {
	let arrivals: Vec<_> = steady(60)
		.into_iter()
		.filter(|a| !(10..18).contains(&a.index))
		.collect();
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 80);
	let runs: Vec<_> = sim
		.log
		.iter()
		.filter_map(|p| match p {
			Playout::Conceal(c) => Some((c.run_frames, c.fade)),
			_ => None,
		})
		.collect();
	assert_eq!(&runs[..4], [(1, false), (2, false), (3, false), (4, true)]);
	assert!(sim.log.contains(&Playout::Skip(SkipReason::Interrupted)));
	let played = sim.played();
	assert!(played.windows(2).all(|w| w[0] < w[1]));
	assert_eq!(*played.last().unwrap(), 59);
	assert_eq!(sim.buf.stats().network_missing, 8);
}

#[test]
fn late_packet_is_dropped_and_does_not_grow_delay() {
	let mut arrivals = steady(60);
	// Packet 20 shows up 12 frames late, long after its deadline.
	arrivals[20].tick = 32;
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 80);
	assert!(!sim.played().contains(&20));
	assert_eq!(sim.buf.stats().late_drop, 1);
	assert!(sim.buf.margin_frames() <= CFG.max_margin_frames);
	let played = sim.played();
	assert!(played.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn deliberate_gap_is_silence_not_loss() {
	// Contiguous sequence numbers, timestamp jumps by 3 frames at packet 20,
	// and the packet is already queued when the gap is reached.
	let mut arrivals = steady(40);
	for a in arrivals.iter_mut().filter(|a| a.index >= 20) {
		a.ts_skip = 3;
		a.tick = a.index;
	}
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 70);
	let gaps = sim
		.log
		.iter()
		.filter(|p| **p == Playout::Skip(SkipReason::Gap))
		.count();
	assert_eq!(gaps, 3);
	let stats = sim.buf.stats();
	assert_eq!((stats.network_missing, sim.concealed_in_body()), (0, 0));
	assert_eq!(sim.played().len(), 40);
}

#[test]
fn silence_longer_than_plc_resumes_as_intentional() {
	let mut arrivals: Vec<_> = steady(10).into_iter().collect();
	arrivals.extend((10..30).map(|i| Arrival {
		tick: i + 20,
		index: i,
		ts_skip: 20,
	}));
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 70);
	let stats = sim.buf.stats();
	assert_eq!(stats.intentional_silence, 1);
	assert_eq!(stats.network_missing, 0);
	assert_eq!(sim.played().len(), 30);
}

#[test]
fn stall_burst_keeps_delay_bounded() {
	// 80 ms stall: eight packets arrive together.
	let mut arrivals = steady(80);
	for a in arrivals.iter_mut().filter(|a| (30..38).contains(&a.index)) {
		a.tick = 38;
	}
	let mut sim = Sim::new(CFG);
	sim.run(&arrivals, 100);
	let played = sim.played();
	assert!(played.windows(2).all(|w| w[0] < w[1]));
	assert!(sim.buf.margin_frames() <= CFG.max_margin_frames);
	// After recovery the playhead is at most `max margin` frames behind the sender.
	let last_tick = sim
		.log
		.iter()
		.rposition(|p| matches!(p, Playout::Play(_)))
		.unwrap() as u32;
	let delay = last_tick - *played.last().unwrap();
	assert!(delay <= CFG.max_margin_frames + 1, "delay {delay} frames");
}

#[test]
fn far_future_packet_is_rejected_without_disturbing_state() {
	let mut sim = Sim::new(CFG);
	sim.run(&steady(5), 5);
	assert_eq!(sim.deliver(5000, 0), InsertOutcome::TooFar);
	sim.run(&steady(0), 0);
	assert_eq!(sim.buf.stats().rejected_far, 1);
}

#[test]
fn margin_grows_after_misses_and_shrinks_after_stability() {
	let mut sim = Sim::new(CFG);
	// Repeated late arrivals every ~25 frames.
	let mut arrivals = steady(200);
	for i in [20u32, 45, 70, 95] {
		arrivals[i as usize].tick = i + 6;
	}
	sim.run(&arrivals, 210);
	assert!(sim.buf.stats().stretches > 0);
	assert!(sim.buf.margin_frames() <= CFG.max_margin_frames);
	assert!(sim.buf.target_margin_frames() > CFG.start_margin_frames);
	// Long clean stretch: target comes back down.
	let more: Vec<_> = (200..2400)
		.map(|i| Arrival {
			tick: i + 210 - 200,
			index: i,
			ts_skip: 0,
		})
		.collect();
	sim.run(&more, 2200);
	assert_eq!(sim.buf.target_margin_frames(), CFG.min_margin_frames);
	let _ = FRAME;
}
