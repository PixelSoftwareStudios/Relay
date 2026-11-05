use relay_net::channel::clock::{ClampedClock, ManualClock, SystemClock, UnixClock};
use relay_net::channel::{Authority, ids};
use relay_net::identity::Identity;

#[test]
fn system_clock_is_sampled_fresh_and_matches_wall_time() {
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_secs();
	let read = SystemClock.now();
	assert!(read.abs_diff(now) <= 2);
}

#[test]
fn a_clamped_clock_never_goes_backwards() {
	let manual = std::sync::Arc::new(ManualClock::new(1_000));
	struct Shared(std::sync::Arc<ManualClock>);
	impl UnixClock for Shared {
		fn now(&self) -> u64 {
			self.0.now()
		}
	}
	let clamped = ClampedClock::new(Shared(manual.clone()));
	assert_eq!(clamped.now(), 1_000);
	manual.set(500);
	assert_eq!(clamped.now(), 1_000, "a backward step is ignored");
	manual.set(1_200);
	assert_eq!(clamped.now(), 1_200);
}

#[test]
fn a_rolled_back_clock_cannot_resurrect_an_expired_certificate() {
	// Review scenario: the verifier's time source is pushed back after the certificate expired.
	let owner = Authority::owner([4; 32]);
	let channel = ids::channel_id(&owner.public());
	let subject = Identity::generate().unwrap().fingerprint();
	let manual = std::sync::Arc::new(ManualClock::new(10_000));
	struct Shared(std::sync::Arc<ManualClock>);
	impl UnixClock for Shared {
		fn now(&self) -> u64 {
			self.0.now()
		}
	}
	let clock = ClampedClock::new(Shared(manual.clone()));
	let cert = owner.issue(&channel, &subject, clock.now(), 100);
	assert!(cert.verify(&owner.public(), &subject, clock.now()).is_ok());
	manual.advance(200);
	assert!(cert.verify(&owner.public(), &subject, clock.now()).is_err());
	manual.set(10_050);
	assert!(
		cert.verify(&owner.public(), &subject, clock.now()).is_err(),
		"expiry stays strict after a rollback"
	);
}
