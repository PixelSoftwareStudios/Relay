use super::*;

fn id(n: u8) -> ChannelId {
	ChannelId::from_bytes([n; 16])
}

fn entry(role: Role) -> ChannelEntry {
	ChannelEntry {
		name: ChannelName::new("Floor 3").unwrap(),
		policy: Policy {
			listed: false,
			password: true,
			invite_only: false,
		},
		role,
	}
}

fn local(ids: &[u8]) -> Memberships {
	let mut m = Memberships::new();
	for n in ids {
		m.insert(id(*n), entry(Role::Member)).unwrap();
	}
	m
}

#[test]
fn names_are_validated() {
	assert!(ChannelName::new("  Dorm A ").is_ok());
	assert_eq!(ChannelName::new("  ").unwrap_err(), NameError::Length);
	assert_eq!(
		ChannelName::new(&"x".repeat(49)).unwrap_err(),
		NameError::Length
	);
	assert_eq!(
		ChannelName::new("a\u{202E}b").unwrap_err(),
		NameError::Character
	);
	assert_eq!(ChannelName::new("<b>").unwrap_err(), NameError::Character);
}

#[test]
fn membership_is_bounded() {
	let mut m = Memberships::new();
	for n in 0..CHANNELS_MAX as u8 {
		m.insert(id(n), entry(Role::Member)).unwrap();
	}
	assert!(m.insert(id(200), entry(Role::Member)).is_err());
	// Updating an existing channel is not a new slot.
	assert!(m.insert(id(0), entry(Role::Admin)).is_ok());
	assert!(m.remove(&id(0)));
	assert!(m.insert(id(200), entry(Role::Member)).is_ok());
}

#[test]
fn unjoined_channels_never_enter_peer_scope() {
	let me = local(&[1]);
	let mut scope = PeerScope::new();
	scope.note(&me, id(9), Evidence::Proven);
	assert_eq!(scope.candidates().count(), 0);
}

#[test]
fn evidence_never_weakens_and_candidate_is_not_proof() {
	let me = local(&[1]);
	let mut scope = PeerScope::new();
	scope.note(&me, id(1), Evidence::Candidate);
	assert!(!scope.shares_proven(&me));
	scope.note(&me, id(1), Evidence::Proven);
	scope.note(&me, id(1), Evidence::Candidate);
	assert!(scope.shares_proven(&me));
	scope.forget(&id(1));
	assert!(!scope.shares_proven(&me));
}

#[test]
fn shared_channel_only_drops_strangers_and_candidates() {
	let me = local(&[1, 2]);
	let mut scope = PeerScope::new();
	assert_eq!(
		InboundPolicy::SharedChannelOnly.decide(&me, &scope),
		RingDecision::Drop
	);
	scope.note(&me, id(2), Evidence::Candidate);
	assert_eq!(
		InboundPolicy::SharedChannelOnly.decide(&me, &scope),
		RingDecision::Drop
	);
	scope.note(&me, id(2), Evidence::Proven);
	assert_eq!(
		InboundPolicy::SharedChannelOnly.decide(&me, &scope),
		RingDecision::Ring
	);
	assert_eq!(
		InboundPolicy::Anyone.decide(&me, &PeerScope::new()),
		RingDecision::Ring
	);
}

#[test]
fn hidden_peers_are_visible_only_with_a_shared_channel() {
	let me = local(&[1]);
	let mut scope = PeerScope::new();
	assert!(!visible(&me, &scope, false));
	assert!(visible(&me, &scope, true));
	scope.note(&me, id(1), Evidence::Candidate);
	assert!(visible(&me, &scope, false));
}

#[test]
fn peer_scope_is_bounded() {
	let mut me = Memberships::new();
	for n in 0..CHANNELS_MAX as u8 {
		me.insert(id(n), entry(Role::Member)).unwrap();
	}
	let mut scope = PeerScope::new();
	for n in 0..CHANNELS_MAX as u8 {
		scope.note(&me, id(n), Evidence::Proven);
	}
	assert!(scope.candidates().count() <= PEER_CHANNELS_MAX);
}

#[test]
fn debug_output_does_not_print_the_full_id() {
	let text = format!("{:?}", id(0xab));
	assert!(text.len() < 24);
}
