mod common;

use common::{Arrival, Sim};
use proptest::prelude::*;
use relay_protocol::jitter::{JitterConfig, Playout};

const CFG: JitterConfig = JitterConfig::BALANCED;
const MAX_JITTER: u32 = 6;
const PACKETS: u32 = 120;

#[derive(Debug, Clone)]
struct Fate {
	lost: bool,
	delay: u32,
	duplicate: Option<u32>,
}

fn fate() -> impl Strategy<Value = Fate> {
	(
		any::<bool>(),
		0..=MAX_JITTER,
		proptest::option::of(0..=MAX_JITTER),
	)
		.prop_map(|(lost_roll, delay, duplicate)| Fate {
			// Roughly one in five is lost; keep most packets alive.
			lost: lost_roll && delay % 2 == 0 && delay < 3,
			delay,
			duplicate,
		})
}

proptest! {
	/// Arbitrary loss / reordering / duplication keeps the buffer bounded,
	/// ordered and within the delay ceiling.
	#[test]
	fn invariants_hold_under_chaos(fates in proptest::collection::vec(fate(), PACKETS as usize)) {
		let mut arrivals = Vec::new();
		for (i, f) in fates.iter().enumerate() {
			let i = i as u32;
			if f.lost { continue; }
			arrivals.push(Arrival { tick: i + f.delay, index: i, ts_skip: 0 });
			if let Some(d) = f.duplicate {
				arrivals.push(Arrival { tick: i + d, index: i, ts_skip: 0 });
			}
		}
		let mut sim = Sim::with_bases(CFG, 65_520, u32::MAX - 4_000);
		let total = PACKETS + 40;
		for _ in 0..total {
			sim.run(&arrivals, 1);
			prop_assert!(sim.buf.buffered() <= CFG.slots);
			prop_assert!(sim.buf.margin_frames() <= CFG.max_margin_frames);
			prop_assert!(sim.buf.occupancy_samples() <= (CFG.horizon_frames() as u32 + 1) * 480 + 480);
		}

		let mut last = None;
		let mut max_delay = 0;
		for (tick, p) in sim.log.iter().enumerate() {
			if let Playout::Play(p) = p {
				prop_assert!(last.is_none_or(|l| p.sequence > l), "out of order play");
				last = Some(p.sequence);
				max_delay = max_delay.max(tick as u32 - p.payload.min(tick as u32));
			}
		}
		// Playout delay is bounded by arrival jitter plus the margin ceiling.
		prop_assert!(max_delay <= MAX_JITTER + CFG.max_margin_frames + 2, "delay {}", max_delay);

		let stats = sim.buf.stats();
		prop_assert_eq!(stats.played as usize, sim.played().len());
		prop_assert!(stats.played <= u64::from(PACKETS));
	}

	/// Arbitrary (even adversarial) sequence / timestamp values never panic.
	#[test]
	fn arbitrary_headers_never_panic(
		packets in proptest::collection::vec((any::<u16>(), any::<u32>(), 0u64..1_000_000), 0..300),
		polls in proptest::collection::vec(0u8..3, 0..300),
	) {
		let mut buf = relay_protocol::jitter::JitterBuffer::<u16>::new(CFG);
		let mut polls = polls.into_iter();
		for (sequence, timestamp, arrival_us) in packets {
			buf.insert(relay_protocol::jitter::PacketMeta { sequence, timestamp, arrival_us }, sequence);
			for _ in 0..polls.next().unwrap_or(0) { let _ = buf.poll(); }
			prop_assert!(buf.buffered() <= CFG.slots);
		}
	}
}
