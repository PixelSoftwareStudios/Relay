//! Deterministic 24 h simulation of review F-12: attacker identities retry whenever the limiter
//! allows while a fresh legitimate joiner probes every minute. A manual clock drives it.

use std::time::Duration;

use relay_net::channel::limits::*;
use relay_net::channel::{AttemptLimiter, ChannelId};
use relay_net::identity::{Fingerprint, Identity};

const DAY: u64 = 24 * 3600;

struct Outcome {
	locked_percent: f64,
	attacker_guesses: u64,
}

fn fingerprint() -> Fingerprint {
	Identity::generate().unwrap().fingerprint()
}

fn simulate(attackers: usize) -> Outcome {
	let channel = ChannelId::from_bytes([7; 16]);
	let mut limiter = AttemptLimiter::new();
	let ids: Vec<Fingerprint> = (0..attackers).map(|_| fingerprint()).collect();
	let (mut probes, mut locked, mut guesses) = (0u64, 0u64, 0u64);
	for tick in 0..DAY / 10 {
		let now = Duration::from_secs(tick * 10);
		for id in &ids {
			if limiter.check(now, id, &channel).is_ok() {
				limiter.record_failure(now, id, &channel);
				guesses += 1;
			}
		}
		if tick % 6 == 0 {
			probes += 1;
			if limiter.check(now, &fingerprint(), &channel).is_err() {
				locked += 1;
			}
		}
	}
	Outcome {
		locked_percent: locked as f64 * 100.0 / probes as f64,
		attacker_guesses: guesses,
	}
}

#[test]
fn few_attackers_rarely_lock_out_a_fresh_joiner() {
	assert!(simulate(1).locked_percent < 1.5);
	assert!(simulate(2).locked_percent < 3.5);
}

#[test]
fn lockout_rate_drops_with_the_reserved_lane() {
	// Before the reserved lane: 29.2 % (4), 47.9 % (8), 84.4 % (16) of probes locked out.
	// Now: 0 % up to 8 identities and about 2 % at 16 (the lane is spent only by identities
	// with a clean record, one use each per window). 64 pinned identities still lock the
	// channel out most of the time; that residual DoS needs 64 owner-pinned devices.
	for attackers in [4, 8] {
		assert!(simulate(attackers).locked_percent < 1.0);
	}
	assert!(simulate(16).locked_percent < 5.0);
	assert!(simulate(64).locked_percent < 100.0);
}

#[test]
fn guessing_ceiling_stays_bounded() {
	// Per 15 min cool-down cycle: the channel budget plus the reserved lane, never more.
	let cycles = DAY / CHANNEL_COOLDOWN.as_secs() + 1;
	let ceiling = cycles * (CHANNEL_FAILURES + RESERVE_FAILURES) as u64;
	for attackers in [1, 4, 16, 64] {
		let out = simulate(attackers);
		assert!(
			out.attacker_guesses <= ceiling,
			"{attackers}: {} > {ceiling}",
			out.attacker_guesses
		);
	}
}

#[test]
fn reserved_lane_needs_a_clean_record() {
	let channel = ChannelId::from_bytes([7; 16]);
	let mut limiter = AttemptLimiter::new();
	let now = Duration::from_secs(0);
	for _ in 0..CHANNEL_FAILURES {
		limiter.record_failure(now, &fingerprint(), &channel);
	}
	let dirty = fingerprint();
	// A failure elsewhere makes the identity ineligible for the lane.
	limiter.record_failure(now, &dirty, &ChannelId::from_bytes([8; 16]));
	assert!(limiter.check(now, &dirty, &channel).is_err());
	let clean = fingerprint();
	assert!(limiter.check(now, &clean, &channel).is_ok());
	// An unknown channel never gets a lane.
	assert!(
		limiter
			.check(now, &clean, &ChannelId::from_bytes([0; 16]))
			.is_ok()
	);
}

#[test]
fn reserved_lane_is_itself_bounded_and_refunds_on_success() {
	let channel = ChannelId::from_bytes([7; 16]);
	let mut limiter = AttemptLimiter::new();
	let now = Duration::from_secs(0);
	for _ in 0..CHANNEL_FAILURES {
		limiter.record_failure(now, &fingerprint(), &channel);
	}
	let joiner = fingerprint();
	assert!(limiter.check(now, &joiner, &channel).is_ok());
	limiter.record_failure(now, &joiner, &channel);
	limiter.record_success(&joiner, &channel);
	// The refund went to the lane, so the channel budget is still cooling down for others.
	let dirty = fingerprint();
	limiter.record_failure(now, &dirty, &ChannelId::from_bytes([9; 16]));
	assert!(limiter.check(now, &dirty, &channel).is_err());
	for _ in 0..RESERVE_FAILURES {
		let id = fingerprint();
		assert!(limiter.check(now, &id, &channel).is_ok());
		limiter.record_failure(now, &id, &channel);
	}
	assert!(limiter.check(now, &fingerprint(), &channel).is_err());
}
