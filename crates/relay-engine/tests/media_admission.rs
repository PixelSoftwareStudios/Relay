mod support_e2e;

use std::time::Duration;

use relay_core::*;
use relay_engine::media::{AdmissionLimits, LoadMeter, LoadSnapshot, PrepareError, Refusal};
use support_e2e::{Pair, active_call_with, run_for};

fn load(calls: usize, capture_us: u64, render_us: u64) -> LoadSnapshot {
	LoadSnapshot {
		calls,
		capture_ns: capture_us * 1_000,
		render_ns: render_us * 1_000,
	}
}

#[test]
fn the_call_cap_and_the_worker_budget_are_distinct_refusals() {
	let limits = AdmissionLimits {
		max_calls: 3,
		budget_permille: 500,
		assumed_call_cost: Duration::from_micros(400),
	};
	assert_eq!(limits.admit(load(0, 0, 0)), Ok(()));
	assert_eq!(limits.admit(load(3, 100, 100)), Err(Refusal::CallCap(3)));
	// 2 calls using 4 ms of a 5 ms budget: a third at the same cost does not fit.
	assert_eq!(
		limits.admit(load(2, 4_000, 100)),
		Err(Refusal::WorkerBudget { worker: "capture" })
	);
	assert_eq!(
		limits.admit(load(2, 100, 4_000)),
		Err(Refusal::WorkerBudget { worker: "render" })
	);
	// Light measured load admits more.
	assert_eq!(limits.admit(load(2, 600, 600)), Ok(()));
}

#[test]
fn an_already_overloaded_worker_admits_nothing_new() {
	let limits = AdmissionLimits::default();
	assert!(limits.admit(load(1, 9_000, 100)).is_err());
}

#[test]
fn load_meter_smooths_single_spikes() {
	let meter = LoadMeter::default();
	assert_eq!(meter.ns_per_frame(), 0);
	for _ in 0..50 {
		meter.record(1_000);
	}
	meter.record(100_000);
	assert!(meter.ns_per_frame() < 20_000, "{}", meter.ns_per_frame());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refusing_a_second_call_leaves_the_running_call_untouched() {
	let Pair {
		mut a,
		mut b,
		call,
		lease_a,
		..
	} = active_call_with(|config| config.limits.max_calls = 1).await;
	a.command(Command::SetTransmitMode {
		call,
		lease: lease_a,
		route: TransmitRoute::unmuted(),
	})
	.unwrap();
	run_for(&mut a, &mut b, Duration::from_millis(800)).await;
	let before = a.media.report(call).unwrap();
	assert!(before.sent > 20, "the first call is transmitting");

	// A second call to the same peer is exchanged, then its media is refused.
	let (second, session) = (a.ids.call(), a.ids.session());
	a.peers
		.send(
			b.id,
			PeerMessage::Invite {
				call: second,
				session,
				allow_parallel_same_peer: true,
			},
		)
		.unwrap();
	let refusal = a.media.prepare(second, session).unwrap_err();
	assert!(matches!(
		refusal,
		PrepareError::Refused(Refusal::CallCap(1))
	));
	assert!(a.media.report(second).is_none());

	run_for(&mut a, &mut b, Duration::from_millis(500)).await;
	let after = a.media.report(call).unwrap();
	assert!(after.sent > before.sent + 20, "still sending at full rate");
	assert_eq!(
		(after.stale, after.misbound),
		(before.stale, before.misbound)
	);
	assert!(a.gate_open(call));
}
