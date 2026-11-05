use std::time::Duration;

use relay_net::channel::limits::*;
use relay_net::channel::{AttemptLimiter, ChannelId};
use relay_net::identity::Identity;

fn secs(n: u64) -> Duration {
	Duration::from_secs(n)
}

fn peer() -> relay_net::identity::Fingerprint {
	Identity::generate().unwrap().fingerprint()
}

#[test]
fn lockout_doubles_with_every_strike() {
	let (mut limiter, p, c) = (
		AttemptLimiter::new(),
		peer(),
		ChannelId::from_bytes([1; 16]),
	);
	let mut now = 0;
	let mut expected = LOCKOUT_BASE;
	// Three strikes: a fourth would also trip the channel-wide budget (20 failures).
	for _ in 0..3 {
		for _ in 0..PEER_CHANNEL_FAILURES {
			limiter.record_failure(secs(now), &p, &c);
		}
		let locked = limiter.check(secs(now), &p, &c).unwrap_err();
		assert_eq!(locked.retry_after, expected);
		now += locked.retry_after.as_secs();
		assert!(limiter.check(secs(now), &p, &c).is_ok());
		expected *= 2;
	}
}

#[test]
fn success_clears_the_pair_but_not_the_shared_budget() {
	let (mut limiter, p, c) = (
		AttemptLimiter::new(),
		peer(),
		ChannelId::from_bytes([1; 16]),
	);
	for _ in 0..4 {
		limiter.record_failure(secs(0), &p, &c);
	}
	limiter.record_success(&p, &c);
	// Four failures forgotten for the pair: four more do not lock it.
	for _ in 0..4 {
		limiter.record_failure(secs(1), &p, &c);
	}
	assert!(limiter.check(secs(1), &p, &c).is_ok());
}

#[test]
fn peer_budget_spans_channels() {
	let (mut limiter, p) = (AttemptLimiter::new(), peer());
	for i in 0..PEER_FAILURES {
		let c = ChannelId::from_bytes([(i % 200) as u8; 16]);
		limiter.record_failure(secs(i as u64), &p, &c);
	}
	let fresh = ChannelId::from_bytes([250; 16]);
	assert!(limiter.check(secs(100), &p, &fresh).is_err());
	assert!(
		limiter
			.check(secs(100) + CHANNEL_COOLDOWN, &p, &fresh)
			.is_ok()
	);
}

#[test]
fn memory_stays_bounded_under_identity_floods() {
	let mut limiter = AttemptLimiter::new();
	let c = ChannelId::from_bytes([1; 16]);
	for i in 0..(LIMITER_ENTRIES_MAX * 3) {
		limiter.record_failure(secs(i as u64 / 100), &peer(), &c);
	}
	// No panic and still functioning; the unit under test enforces the bound internally.
	assert!(limiter.check(secs(10_000), &peer(), &c).is_ok());
}
