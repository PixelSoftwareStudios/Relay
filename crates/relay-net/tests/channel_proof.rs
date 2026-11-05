mod channel_support;

use channel_support::*;
use relay_core::channel::{Evidence, InboundPolicy, Memberships, PeerScope, RingDecision, visible};
use relay_net::channel::{ChannelRecord, Credential, check_proof, make_proof};
use relay_net::identity::Identity;

struct Group {
	world: World,
	owner: Identity,
	alice: Identity,
	alice_record: ChannelRecord,
}

/// An owner device plus Alice, who joined an open hidden channel.
fn group() -> Group {
	let owner = Identity::generate().unwrap();
	let alice = Identity::generate().unwrap();
	let world = world(OPEN, None, &owner.fingerprint());
	let pair = Pair::with(
		Identity::from_secret_bytes(*alice.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let alice_record = run_join(
		&pair,
		&world,
		&Credential::open(&world.channel),
		None,
		mono(1),
	)
	.unwrap();
	Group {
		world,
		owner,
		alice,
		alice_record,
	}
}

fn owner_record(g: &Group) -> ChannelRecord {
	g.world.store.get(&g.world.channel).unwrap().unwrap()
}

use relay_net::channel::ChannelStore;

#[test]
fn members_prove_membership_to_each_other_in_both_directions() {
	let g = group();
	let link = Pair::with(
		Identity::from_secret_bytes(*g.alice.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let owner = owner_record(&g);
	let proof = make_proof(&g.alice_record, &link.joiner_binding, NOW).unwrap();
	let seen = check_proof(
		std::slice::from_ref(&owner),
		&link.admitter_binding,
		&g.alice.fingerprint(),
		&proof,
		NOW,
	);
	assert_eq!(seen.proven, Some(g.world.channel));

	// The owner answers only after verifying; Alice verifies the reply.
	let reply = make_proof(&owner, &link.admitter_binding, NOW).unwrap();
	let back = check_proof(
		std::slice::from_ref(&g.alice_record),
		&link.joiner_binding,
		&g.owner.fingerprint(),
		&reply,
		NOW,
	);
	assert_eq!(back.proven, Some(g.world.channel));
}

#[test]
fn a_copied_proof_is_useless_on_another_session_or_for_another_key() {
	let g = group();
	let link = Pair::with(
		Identity::from_secret_bytes(*g.alice.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let proof = make_proof(&g.alice_record, &link.joiner_binding, NOW).unwrap();
	let owner = owner_record(&g);

	// Mallory replays Alice's recorded message on his own link to the owner.
	let mallory = Identity::generate().unwrap();
	let other = Pair::with(
		Identity::from_secret_bytes(*mallory.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let out = check_proof(
		std::slice::from_ref(&owner),
		&other.admitter_binding,
		&mallory.fingerprint(),
		&proof,
		NOW,
	);
	assert!(out.proven.is_none() && out.matched.is_none());

	// Even with a correct reference for his own session, Alice's certificate names Alice's key.
	let mut stolen = g.alice_record.clone();
	stolen.cert = g.alice_record.cert.clone();
	let forged_ref = make_proof(&stolen, &other.joiner_binding, NOW).unwrap();
	let out = check_proof(
		std::slice::from_ref(&owner),
		&other.admitter_binding,
		&mallory.fingerprint(),
		&forged_ref,
		NOW,
	);
	assert!(out.matched.is_some());
	assert!(out.proven.is_none());
}

#[test]
fn someone_who_knows_the_id_but_has_no_certificate_gets_nothing() {
	let g = group();
	let link = Pair::new();
	let mut keyless = g.alice_record.clone();
	keyless.cert = None;
	assert!(make_proof(&keyless, &link.joiner_binding, NOW).is_none());
}

#[test]
fn expired_certificates_are_not_sent_or_accepted() {
	let g = group();
	let link = Pair::with(
		Identity::from_secret_bytes(*g.alice.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let late = NOW + 31 * 86_400;
	assert!(make_proof(&g.alice_record, &link.joiner_binding, late).is_none());
	let proof = make_proof(&g.alice_record, &link.joiner_binding, NOW).unwrap();
	let out = check_proof(
		&[owner_record(&g)],
		&link.admitter_binding,
		&g.alice.fingerprint(),
		&proof,
		late,
	);
	assert!(out.proven.is_none());
}

#[test]
fn revoked_members_are_refused_and_revocations_spread_through_proofs() {
	let g = group();
	let link = Pair::with(
		Identity::from_secret_bytes(*g.alice.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let mut owner = owner_record(&g);
	let revocation = owner.authority.as_ref().unwrap().revoke(
		&owner.id,
		&g.alice.fingerprint(),
		NOW + 86_400 * 30,
	);
	owner.add_revocation(revocation);

	// The owner refuses Alice.
	let proof = make_proof(&g.alice_record, &link.joiner_binding, NOW).unwrap();
	let out = check_proof(
		std::slice::from_ref(&owner),
		&link.admitter_binding,
		&g.alice.fingerprint(),
		&proof,
		NOW,
	);
	assert!(out.proven.is_none());
	// A third member, Bob, learns the revocation from a proof the owner sends him.
	let bob = Identity::generate().unwrap();
	let bob_pair = Pair::with(
		Identity::from_secret_bytes(*bob.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let bob_record = run_join(
		&bob_pair,
		&g.world,
		&Credential::open(&g.world.channel),
		None,
		mono(5),
	)
	.unwrap();
	let owner_msg = make_proof(&owner, &bob_pair.admitter_binding, NOW).unwrap();
	let learned = check_proof(
		std::slice::from_ref(&bob_record),
		&bob_pair.joiner_binding,
		&g.owner.fingerprint(),
		&owner_msg,
		NOW,
	);
	assert_eq!(learned.proven, Some(g.world.channel));
	assert_eq!(learned.new_revocations.len(), 1);
	let mut bob_record = bob_record;
	for r in learned.new_revocations {
		bob_record.add_revocation(r);
	}
	assert!(bob_record.is_revoked(&g.alice.fingerprint(), NOW));
	// The revocation covers only its window.
	assert!(!bob_record.is_revoked(&g.alice.fingerprint(), NOW + 86_400 * 31));
}

#[test]
fn revoked_member_cannot_rejoin_with_the_same_key() {
	let g = group();
	let mut owner = owner_record(&g);
	let revocation =
		owner
			.authority
			.as_ref()
			.unwrap()
			.revoke(&owner.id, &g.alice.fingerprint(), NOW + 86_400);
	owner.add_revocation(revocation);
	g.world.store.put(owner).unwrap();
	let pair = Pair::with(
		Identity::from_secret_bytes(*g.alice.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	assert!(
		run_join(
			&pair,
			&g.world,
			&Credential::open(&g.world.channel),
			None,
			mono(9)
		)
		.is_err()
	);
}

#[test]
fn proof_drives_visibility_and_the_ring_decision_but_never_acceptance() {
	let g = group();
	let mut local = Memberships::new();
	local
		.insert(g.world.channel, g.alice_record.entry())
		.unwrap();
	let mut scope = PeerScope::new();
	// A tag match alone is only a candidate: visible, but not shared for authorization.
	scope.note(&local, g.world.channel, Evidence::Candidate);
	assert!(visible(&local, &scope, false));
	assert_eq!(
		InboundPolicy::SharedChannelOnly.decide(&local, &scope),
		RingDecision::Drop
	);
	scope.note(&local, g.world.channel, Evidence::Proven);
	assert_eq!(
		InboundPolicy::SharedChannelOnly.decide(&local, &scope),
		RingDecision::Ring
	);
	// An outsider who advertised nothing about channels stays invisible in hidden mode.
	assert!(!visible(&local, &PeerScope::new(), false));
}

#[test]
fn renewal_extends_a_certificate_and_refuses_the_revoked() {
	use relay_net::channel::join::{apply_renewal, renewal_request};
	let g = group();
	let link = Pair::with(
		Identity::from_secret_bytes(*g.alice.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	// Ten days later the certificate is still valid; renew then.
	let later = NOW + 10 * 86_400;
	let request = renewal_request(&g.alice_record, &link.joiner_binding).unwrap();
	let grant = g.world.service.renew(
		&link.admitter_binding,
		&g.alice.fingerprint(),
		&request,
		mono(1),
		later,
	);
	let mut record = g.alice_record.clone();
	let old_expiry = record.cert.as_ref().unwrap().expires_at;
	apply_renewal(&mut record, &grant, &g.alice.fingerprint(), later).unwrap();
	assert!(record.cert.as_ref().unwrap().expires_at > old_expiry);

	// Another device cannot renew Alice's certificate.
	let mallory = Identity::generate().unwrap();
	let other = Pair::with(
		Identity::from_secret_bytes(*mallory.secret_bytes()).unwrap(),
		Identity::from_secret_bytes(*g.owner.secret_bytes()).unwrap(),
		relay_net::session::LinkEpoch::random(),
	);
	let request = renewal_request(&g.alice_record, &other.joiner_binding).unwrap();
	let refused = g.world.service.renew(
		&other.admitter_binding,
		&mallory.fingerprint(),
		&request,
		mono(2),
		later,
	);
	assert!(
		apply_renewal(
			&mut g.alice_record.clone(),
			&refused,
			&mallory.fingerprint(),
			later
		)
		.is_err()
	);

	// Once revoked, even her own valid certificate cannot be renewed.
	let mut owner = owner_record(&g);
	let revocation =
		owner
			.authority
			.as_ref()
			.unwrap()
			.revoke(&owner.id, &g.alice.fingerprint(), later + 86_400);
	owner.add_revocation(revocation);
	g.world.store.put(owner).unwrap();
	let request = renewal_request(&g.alice_record, &link.joiner_binding).unwrap();
	let refused = g.world.service.renew(
		&link.admitter_binding,
		&g.alice.fingerprint(),
		&request,
		mono(3),
		later,
	);
	assert!(
		apply_renewal(
			&mut g.alice_record.clone(),
			&refused,
			&g.alice.fingerprint(),
			later
		)
		.is_err()
	);
}
