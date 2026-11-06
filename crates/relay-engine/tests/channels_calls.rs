#![cfg(unix)]
//! Channel membership is metadata about peers. It never rings, accepts, grants
//! microphone consent or links calls: A in one channel with B and C, calling both
//! at once, keeps two independent calls and nobody hears anybody else's audio.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use relay_api::pb::{CallState, TransmitMode};
use service_support::*;
use support_channels::*;
use support_media::{power_at, rms};

const TONE_A: f32 = 440.0;
const TONE_B: f32 = 1_700.0;
const TONE_C: f32 = 3_100.0;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn channel_members_get_independent_calls_and_no_cross_audio() {
	let mut a = side(TONE_A).await;
	let mut b = side(TONE_B).await;
	let mut c = side(TONE_C).await;
	pair(&mut a, &mut b, 1).await;
	pair(&mut a, &mut c, 10).await;
	pair(&mut b, &mut c, 20).await;

	// Everyone is in one channel and every pair has proven it.
	let id = create_open(&mut a.cli, "Floor 2").await;
	assert_applied(&b.cli.join_channel(&id, "", "").await.expect("b joins"));
	assert_applied(&c.cli.join_channel(&id, "", "").await.expect("c joins"));
	let (a_id, b_id, c_id) = (id_of(&a), id_of(&b), id_of(&c));
	wait_shared(&mut a.cli, &b_id, &id).await;
	wait_shared(&mut a.cli, &c_id, &id).await;
	wait_shared(&mut b.cli, &c_id, &id).await;
	wait_shared(&mut c.cli, &b_id, &id).await;
	wait_shared(&mut b.cli, &a_id, &id).await;

	for (i, side) in [&mut a, &mut b, &mut c].into_iter().enumerate() {
		select_microphone(&mut side.cli, 100 + i as u8).await;
	}

	// A rings both at once.
	let to_b = ring(&mut a, &b, 30).await;
	let to_c = ring(&mut a, &c, 31).await;
	assert_ne!(to_b, to_c);

	// Membership does not accept for anyone: both calls keep ringing.
	settle(600).await;
	for (cli, call) in [(&mut b.cli, &to_b), (&mut c.cli, &to_c)] {
		let ringing = call_in(cli, call).await.expect("call");
		assert_eq!(ringing.state, CallState::IncomingRinging as i32);
	}

	// Each callee answers its own call.
	answer(&mut b.cli, &to_b, 40).await;
	answer(&mut c.cli, &to_c, 40).await;
	wait_state(&mut a.cli, &to_b, CallState::Connected).await;
	wait_state(&mut a.cli, &to_c, CallState::Connected).await;
	wait_state(&mut b.cli, &to_b, CallState::Connected).await;
	wait_state(&mut c.cli, &to_c, CallState::Connected).await;

	// Connected and silent: shared membership did not consent to any microphone.
	settle(500).await;
	for side in [&a, &b, &c] {
		assert!(rms(&tail(side, 300)) < 1e-4, "nobody consented yet");
	}
	for call in [&to_b, &to_c] {
		let view = call_in(&mut a.cli, call).await.expect("call");
		assert!(!view.transmit.expect("route").consent_granted);
	}

	// Everyone consents on their own call(s).
	route(&mut a.cli, 50, &to_b, TransmitMode::Unmuted).await;
	route(&mut a.cli, 51, &to_c, TransmitMode::Unmuted).await;
	route(&mut b.cli, 52, &to_b, TransmitMode::Unmuted).await;
	route(&mut c.cli, 53, &to_c, TransmitMode::Unmuted).await;

	// Canary tones: each callee hears A and nobody else; A hears both callees.
	eventually!({
		let heard = tail(&b, 300);
		(power_at(&heard, TONE_A) > 0.02).then_some(heard)
	});
	eventually!({
		let heard = tail(&c, 300);
		(power_at(&heard, TONE_A) > 0.02).then_some(heard)
	});
	eventually!({
		let heard = tail(&a, 300);
		(power_at(&heard, TONE_B) > 0.5 * power_at(&heard, TONE_A) && rms(&heard) > 0.02)
			.then_some(heard)
	});
	// Peak power of each canary tone at each speaker over a stretch of playback.
	let mut peaks = [[0.0f64; 3]; 3];
	for _ in 0..24 {
		settle(50).await;
		for (row, side) in [&a, &b, &c].into_iter().enumerate() {
			let heard = tail(side, 200);
			for (col, tone) in [TONE_A, TONE_B, TONE_C].into_iter().enumerate() {
				peaks[row][col] = peaks[row][col].max(power_at(&heard, tone));
			}
		}
	}
	let [at_a, at_b, at_c] = peaks;
	assert!(
		at_b[0] > 0.01 && at_c[0] > 0.01,
		"callees hear A: {at_b:?} {at_c:?}"
	);
	assert!(at_b[0] > 20.0 * at_b[2], "B never hears C: {at_b:?}");
	assert!(at_b[0] > 20.0 * at_b[1], "B never hears itself: {at_b:?}");
	assert!(at_c[0] > 20.0 * at_c[1], "C never hears B: {at_c:?}");
	assert!(at_c[0] > 20.0 * at_c[2], "C never hears itself: {at_c:?}");
	assert!(at_a[1] > 0.01 && at_a[2] > 0.01, "A hears both: {at_a:?}");
	assert!(at_a[1] > 20.0 * at_a[0] && at_a[2] > 20.0 * at_a[0]);

	// Hanging up one call leaves the other untouched.
	let generation = controller_of(&call_in(&mut a.cli, &to_b).await.expect("call")).generation;
	let hangup = mutation(&a.cli, 60, &to_b, generation);
	assert_applied(
		&a.cli
			.raw()
			.end_call(hangup)
			.await
			.expect("end")
			.into_inner(),
	);
	wait_state(&mut a.cli, &to_b, CallState::Ended).await;
	wait_state(&mut b.cli, &to_b, CallState::Ended).await;
	let still = call_in(&mut a.cli, &to_c).await.expect("call");
	assert_eq!(still.state, CallState::Connected as i32);

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
	c.daemon.shutdown().await.expect("c stops");
}
