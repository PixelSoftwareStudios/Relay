use relay_net::channel::cert::{Authority, Delegation, MembershipCert, Revocation};
use relay_net::channel::{CertError, ids};
use relay_net::identity::Identity;

const NOW: u64 = 1_800_000_000;
const DAY: u64 = 86_400;

struct Fixture {
	owner: Authority,
	channel: relay_net::channel::ChannelId,
}

fn fixture() -> Fixture {
	let owner = Authority::owner([11; 32]);
	let channel = ids::channel_id(&owner.public());
	Fixture { owner, channel }
}

fn member() -> relay_net::identity::Fingerprint {
	Identity::generate().unwrap().fingerprint()
}

#[test]
fn issued_certificate_verifies_for_its_subject_only() {
	let f = fixture();
	let (alice, mallory) = (member(), member());
	let cert = f.owner.issue(&f.channel, &alice, NOW, 14 * DAY);
	cert.verify(&f.owner.public(), &alice, NOW).unwrap();
	assert_eq!(
		cert.verify(&f.owner.public(), &mallory, NOW),
		Err(CertError::WrongSubject)
	);
}

#[test]
fn certificate_expires_and_lifetime_is_capped() {
	let f = fixture();
	let alice = member();
	let cert = f.owner.issue(&f.channel, &alice, NOW, 365 * DAY);
	assert_eq!(cert.expires_at, NOW + 30 * DAY);
	assert!(
		cert.verify(&f.owner.public(), &alice, NOW + 30 * DAY - 1)
			.is_ok()
	);
	assert_eq!(
		cert.verify(&f.owner.public(), &alice, NOW + 30 * DAY),
		Err(CertError::Expired)
	);
	// Renewal grace accepts a recently expired one, but not a long expired one.
	assert!(
		cert.verify_with_grace(&f.owner.public(), &alice, NOW + 31 * DAY, 7 * DAY)
			.is_ok()
	);
	assert!(
		cert.verify_with_grace(&f.owner.public(), &alice, NOW + 38 * DAY, 7 * DAY)
			.is_err()
	);
}

#[test]
fn forged_and_tampered_certificates_fail() {
	let f = fixture();
	let alice = member();
	let cert = f.owner.issue(&f.channel, &alice, NOW, DAY);
	// Signed by someone who is not the owner.
	let rogue = Authority::owner([99; 32]);
	let forged = rogue.issue(&f.channel, &alice, NOW, DAY);
	assert!(forged.verify(&f.owner.public(), &alice, NOW).is_err());
	// Every single-bit flip of the encoding is rejected.
	let bytes = cert.to_bytes();
	for i in 0..bytes.len() {
		let mut bad = bytes.clone();
		bad[i] ^= 1;
		let ok = MembershipCert::from_bytes(&bad)
			.is_ok_and(|c| c.verify(&f.owner.public(), &alice, NOW).is_ok());
		assert!(!ok, "bit flip at byte {i} was accepted");
	}
}

#[test]
fn certificate_for_another_channel_fails() {
	let f = fixture();
	let alice = member();
	let other = Authority::owner([12; 32]);
	let other_channel = ids::channel_id(&other.public());
	let cert = other.issue(&other_channel, &alice, NOW, DAY);
	assert!(cert.verify(&f.owner.public(), &alice, NOW).is_err());
}

#[test]
fn delegated_admin_can_issue_until_the_delegation_lapses() {
	let f = fixture();
	let (alice, bob) = (member(), member());
	let admin_seed = [21; 32];
	let admin_public = Authority::owner(admin_seed).public();
	let delegation = f
		.owner
		.delegate(&f.channel, admin_public, NOW, 10 * DAY)
		.unwrap();
	let admin = Authority::admin(admin_seed, delegation);
	let cert = admin.issue(&f.channel, &alice, NOW, 14 * DAY);
	// The certificate cannot outlive the delegation.
	assert_eq!(cert.expires_at, NOW + 10 * DAY);
	assert!(cert.verify(&f.owner.public(), &alice, NOW).is_ok());
	assert!(
		cert.verify(&f.owner.public(), &alice, NOW + 10 * DAY)
			.is_err()
	);
	// An admin cannot delegate onward.
	assert!(admin.delegate(&f.channel, [1; 32], NOW, DAY).is_none());
	// A self-declared admin without an owner-signed delegation is rejected.
	let fake_admin = Authority::admin(
		[22; 32],
		Delegation::from_bytes(&[0u8; relay_net::channel::cert::DELEGATION_BYTES]).unwrap(),
	);
	assert!(
		fake_admin
			.issue(&f.channel, &bob, NOW, DAY)
			.verify(&f.owner.public(), &bob, NOW)
			.is_err()
	);
}

#[test]
fn delegation_cannot_be_moved_to_another_admin_key() {
	let f = fixture();
	let alice = member();
	let admin_public = Authority::owner([21; 32]).public();
	let delegation = f
		.owner
		.delegate(&f.channel, admin_public, NOW, DAY)
		.unwrap();
	// Another key tries to use the stolen delegation.
	let thief = Authority::admin([23; 32], delegation);
	let cert = thief.issue(&f.channel, &alice, NOW, DAY);
	assert!(cert.verify(&f.owner.public(), &alice, NOW).is_err());
}

#[test]
fn revocations_verify_and_expire() {
	let f = fixture();
	let alice = member();
	let revocation = f.owner.revoke(&f.channel, &alice, NOW + 30 * DAY);
	revocation.verify(&f.owner.public(), NOW).unwrap();
	assert!(revocation.covers(&alice, NOW));
	assert!(!revocation.covers(&alice, NOW + 30 * DAY));
	assert!(!revocation.covers(&member(), NOW));
	let rogue = Authority::owner([99; 32]).revoke(&f.channel, &alice, NOW + DAY);
	assert!(rogue.verify(&f.owner.public(), NOW).is_err());
	let again = Revocation::from_bytes(&revocation.to_bytes()).unwrap();
	assert_eq!(again, revocation);
}

#[test]
fn channel_id_is_self_certifying() {
	let a = Authority::owner([1; 32]).public();
	let b = Authority::owner([2; 32]).public();
	assert_ne!(ids::channel_id(&a), ids::channel_id(&b));
	assert_eq!(ids::channel_id(&a), ids::channel_id(&a));
}
