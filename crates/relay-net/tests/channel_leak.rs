//! A trusted peer that is not in a channel must learn nothing about it from the whole link-up
//! exchange, even though every message is produced by the real proof functions.

mod channel_support;

use channel_support::*;
use prost::Message;
use relay_net::channel::{
	ChannelId, ChannelRecord, SessionBinding, check_proof, check_ref, make_proof, make_ref,
	new_channel,
};
use relay_net::identity::Fingerprint;
use relay_net::proto::ChannelMessage;
use relay_net::proto::channel_message::Body;

struct Side {
	records: Vec<ChannelRecord>,
	binding: SessionBinding,
	remote: Fingerprint,
	/// Channels for which we already sent our certificate on this link.
	sent_cert: Vec<ChannelId>,
	proven: Vec<ChannelId>,
}

impl Side {
	fn new(records: Vec<ChannelRecord>, binding: SessionBinding, remote: Fingerprint) -> Self {
		Self {
			records,
			binding,
			remote,
			sent_cert: Vec::new(),
			proven: Vec::new(),
		}
	}

	/// Link-up: one reference per channel we hold a certificate for.
	fn start(&self) -> Vec<ChannelMessage> {
		self.records
			.iter()
			.filter(|r| r.cert.is_some())
			.map(|r| make_ref(r, &self.binding))
			.collect()
	}

	fn receive(&mut self, message: &ChannelMessage) -> Vec<ChannelMessage> {
		let mut out = Vec::new();
		if let Some(id) = check_ref(&self.records, &self.binding, message)
			&& !self.sent_cert.contains(&id)
			&& let Some(record) = self.records.iter().find(|r| r.id == id)
			&& let Some(proof) = make_proof(record, &self.binding, NOW)
		{
			self.sent_cert.push(id);
			out.push(proof);
		}
		let outcome = check_proof(&self.records, &self.binding, &self.remote, message, NOW);
		self.proven.extend(outcome.proven);
		out
	}
}

/// Runs both sides to quiescence; returns everything each side put on the wire.
fn run(a: &mut Side, b: &mut Side) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
	let (mut from_a, mut from_b) = (Vec::new(), Vec::new());
	let mut to_b: Vec<ChannelMessage> = a.start();
	let mut to_a: Vec<ChannelMessage> = b.start();
	from_a.extend(to_b.iter().map(Message::encode_to_vec));
	from_b.extend(to_a.iter().map(Message::encode_to_vec));
	for _ in 0..8 {
		let mut next_to_a = Vec::new();
		let mut next_to_b = Vec::new();
		for m in &to_b {
			next_to_a.extend(b.receive(m));
		}
		for m in &to_a {
			next_to_b.extend(a.receive(m));
		}
		from_a.extend(next_to_b.iter().map(Message::encode_to_vec));
		from_b.extend(next_to_a.iter().map(Message::encode_to_vec));
		(to_a, to_b) = (next_to_a, next_to_b);
		if to_a.is_empty() && to_b.is_empty() {
			break;
		}
	}
	(from_a, from_b)
}

fn contains(haystack: &[Vec<u8>], needle: &[u8]) -> bool {
	haystack
		.iter()
		.any(|m| m.windows(needle.len()).any(|w| w == needle))
}

struct Fixture {
	member_records: Vec<ChannelRecord>,
	pair: Pair,
}

/// A member holding two channels, on a link to `peer`.
fn fixture() -> Fixture {
	let pair = Pair::new();
	let fp = pair.joiner_fp();
	let records = (0..2)
		.map(|i| new_channel(&format!("secret {i}"), OPEN, None, &fp, NOW).unwrap())
		.collect();
	Fixture {
		member_records: records,
		pair,
	}
}

#[test]
fn a_trusted_non_member_learns_nothing_from_the_whole_link_up() {
	let f = fixture();
	let ids: Vec<ChannelId> = f.member_records.iter().map(|r| r.id).collect();
	let mut member = Side::new(
		f.member_records.clone(),
		f.pair.joiner_binding,
		f.pair.admitter.fingerprint(),
	);
	// The stranger holds an unrelated channel and an unrelated certificate.
	let unrelated = new_channel("other", OPEN, None, &f.pair.admitter.fingerprint(), NOW).unwrap();
	let mut stranger = Side::new(vec![unrelated], f.pair.admitter_binding, f.pair.joiner_fp());
	let (from_member, from_stranger) = run(&mut member, &mut stranger);

	assert!(!from_member.is_empty(), "the member did speak (references)");
	for record in &f.member_records {
		let cert = record.cert.as_ref().unwrap().to_bytes();
		for secret in [
			&record.id.as_bytes()[..],
			&record.owner_public[..],
			&cert[..],
			record.name.as_str().as_bytes(),
		] {
			assert!(
				!contains(&from_member, secret),
				"channel material leaked to a non-member"
			);
		}
	}
	// Only opaque references went out: no certificate message at all.
	for bytes in &from_member {
		let message = ChannelMessage::decode(bytes.as_slice()).unwrap();
		assert!(matches!(message.body, Some(Body::MemberRef(_))));
	}
	assert!(
		from_stranger
			.iter()
			.all(|m| !contains(std::slice::from_ref(m), &ids[0].as_bytes()[..]))
	);
	assert!(stranger.proven.is_empty() && member.proven.is_empty());
}

#[test]
fn members_still_prove_each_other_and_a_partial_overlap_leaks_only_the_shared_channel() {
	let f = fixture();
	let mut other = f.member_records[0].clone();
	// The second device holds only the first channel (its own certificate for its own key).
	other.cert = Some(other.authority.as_ref().unwrap().issue(
		&other.id,
		&f.pair.admitter.fingerprint(),
		NOW,
		86_400,
	));
	let mut member = Side::new(
		f.member_records.clone(),
		f.pair.joiner_binding,
		f.pair.admitter.fingerprint(),
	);
	let mut peer = Side::new(vec![other], f.pair.admitter_binding, f.pair.joiner_fp());
	let (from_member, _) = run(&mut member, &mut peer);
	assert_eq!(member.proven, vec![f.member_records[0].id]);
	assert_eq!(peer.proven, vec![f.member_records[0].id]);
	// The channel the peer does not hold stays secret.
	let second = &f.member_records[1];
	assert!(!contains(&from_member, &second.id.as_bytes()[..]));
	assert!(!contains(&from_member, &second.owner_public[..]));
}

#[test]
fn a_reflected_reference_is_not_accepted() {
	let f = fixture();
	let own = make_ref(&f.member_records[0], &f.pair.joiner_binding);
	assert!(check_ref(&f.member_records, &f.pair.joiner_binding, &own).is_none());
	// And a reference computed on another link does not match.
	let other = Pair::new();
	assert!(check_ref(&f.member_records, &other.admitter_binding, &own).is_none());
	let theirs = make_ref(&f.member_records[0], &f.pair.admitter_binding);
	assert!(check_ref(&f.member_records, &f.pair.joiner_binding, &theirs).is_some());
}

#[test]
fn guessing_the_certificate_exchange_without_a_reference_yields_nothing() {
	// A peer that skips step 1 and just sends a proof gets no certificate back from the
	// protocol functions: only check_ref gates make_proof, and it never matched.
	let f = fixture();
	let mut member = Side::new(
		f.member_records.clone(),
		f.pair.joiner_binding,
		f.pair.admitter.fingerprint(),
	);
	let bogus = ChannelMessage {
		body: Some(Body::Proof(relay_net::proto::ChannelProof {
			channel_ref: vec![0; 16],
			cert: vec![1; 40],
			revocations: Vec::new(),
		})),
	};
	assert!(member.receive(&bogus).is_empty());
}
