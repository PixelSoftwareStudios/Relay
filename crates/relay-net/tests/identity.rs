use relay_net::identity::{
	Fingerprint, FingerprintParseError, Identity, InMemoryTrustStore, PeerTrust, TrustRecord,
	TrustState, TrustStore, evaluate_peer,
};
use std::time::SystemTime;

fn record(fp: Fingerprint, state: TrustState, name: &str) -> TrustRecord {
	TrustRecord {
		fingerprint: fp,
		state,
		display_name: name.into(),
		trusted_at: SystemTime::UNIX_EPOCH,
	}
}

#[test]
fn identity_roundtrips_through_its_private_key() {
	let a = Identity::generate().unwrap();
	let b = Identity::from_secret_bytes(*a.secret_bytes()).unwrap();
	assert_eq!(a.public(), b.public());
	assert_eq!(a.fingerprint(), b.fingerprint());
	assert_ne!(a.fingerprint(), Identity::generate().unwrap().fingerprint());
}

#[test]
fn debug_output_never_contains_key_material() {
	let id = Identity::generate().unwrap();
	let shown = format!("{id:?}");
	let secret_hex: String = id
		.secret_bytes()
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	assert!(!shown.to_lowercase().contains(&secret_hex));
	assert!(shown.starts_with("Identity("));
}

#[test]
fn fingerprint_formats_are_stable_and_distinct() {
	let fp = Fingerprint::from_bytes(core::array::from_fn(|i| i as u8));
	assert_eq!(fp.short(), "0001-0203-0405-0607-0809");
	assert_eq!(fp.full().split(' ').count(), 16);
	assert!(fp.full().starts_with("0001 0203 0405"));
	assert_eq!(fp.to_string(), fp.full());
}

#[test]
fn fingerprint_parse_accepts_separators_and_case_and_rejects_bad_input() {
	let fp = Fingerprint::from_bytes(core::array::from_fn(|i| (i * 7) as u8));
	assert_eq!(Fingerprint::parse(&fp.full().to_lowercase()).unwrap(), fp);
	assert_eq!(
		Fingerprint::parse(&fp.full().replace(' ', "-")).unwrap(),
		fp
	);
	assert_eq!(
		Fingerprint::parse("abcd"),
		Err(FingerprintParseError::Length)
	);
	assert_eq!(
		Fingerprint::parse(&"zz".repeat(32)),
		Err(FingerprintParseError::Character)
	);
	assert_eq!(
		Fingerprint::parse(&format!("{}00", fp.full())),
		Err(FingerprintParseError::Length)
	);
}

#[test]
fn trust_evaluation_distinguishes_new_unverified_verified_and_key_change() {
	let mut store = InMemoryTrustStore::new();
	let alice = Identity::generate().unwrap().fingerprint();
	let bob = Identity::generate().unwrap().fingerprint();
	assert_eq!(evaluate_peer(&store, &alice, "Alice"), PeerTrust::New);

	store
		.put(record(alice, TrustState::UnverifiedFirstUse, "Alice"))
		.unwrap();
	assert_eq!(
		evaluate_peer(&store, &alice, "Alice"),
		PeerTrust::UnverifiedFirstUse
	);

	// Evaluating never writes: a different key claiming Alice's name is an event, not a replacement.
	let impostor = Identity::generate().unwrap().fingerprint();
	assert!(matches!(
		evaluate_peer(&store, &impostor, "Alice"),
		PeerTrust::KeyChanged { .. }
	));
	assert_eq!(store.len(), 1);
	assert!(store.get(&impostor).is_none());

	store
		.put(record(alice, TrustState::Verified, "Alice"))
		.unwrap();
	assert_eq!(evaluate_peer(&store, &alice, "Alice"), PeerTrust::Verified);
	assert_eq!(evaluate_peer(&store, &bob, "Bob"), PeerTrust::New);
}

#[test]
fn verified_records_are_never_silently_downgraded() {
	let mut store = InMemoryTrustStore::new();
	let fp = Identity::generate().unwrap().fingerprint();
	store.put(record(fp, TrustState::Verified, "A")).unwrap();
	store
		.put(record(fp, TrustState::UnverifiedFirstUse, "A"))
		.unwrap();
	assert_eq!(store.get(&fp).unwrap().state, TrustState::Verified);
	store.remove(&fp).unwrap();
	assert!(store.remove(&fp).is_err());
}

#[test]
fn trust_store_is_bounded() {
	let mut store = InMemoryTrustStore::new();
	let mut result = Ok(());
	for i in 0..1100u32 {
		let mut bytes = [0u8; 32];
		bytes[..4].copy_from_slice(&i.to_be_bytes());
		result = store.put(record(
			Fingerprint::from_bytes(bytes),
			TrustState::UnverifiedFirstUse,
			"x",
		));
		if result.is_err() {
			break;
		}
	}
	assert!(result.is_err());
	assert!(store.len() <= 1024);
}

#[test]
fn lookalike_names_of_a_trusted_contact_raise_key_changed() {
	use relay_net::discovery::{name_skeleton, sanitize_display_name};
	let mut store = InMemoryTrustStore::new();
	let alice = Identity::generate().unwrap().fingerprint();
	store
		.put(TrustRecord {
			fingerprint: alice,
			state: TrustState::Verified,
			display_name: "Alice".into(),
			trusted_at: std::time::SystemTime::now(),
		})
		.unwrap();
	// The review's scenario: every one of these used to evaluate as `New`.
	for lookalike in [
		"Alice\u{3164}",
		"Alice\u{FFA0}",
		"Ali\u{034F}ce",
		"Alice\u{FE0F}",
		"Alice\u{E0041}",
		"Alice\u{2800}",
		"Alice\u{180E}",
		"Alice\u{206A}",
		"\u{0410}lice",
		"Alic\u{0435}",
		"ALICE",
		"A l i c e",
		"\u{FF21}lice",
		"Al\u{0131}ce",
	] {
		let stranger = Identity::generate().unwrap().fingerprint();
		assert!(
			matches!(
				evaluate_peer(&store, &stranger, lookalike),
				PeerTrust::KeyChanged { .. }
			),
			"{lookalike:?} -> {:?}",
			sanitize_display_name(lookalike)
		);
	}
	// A genuinely different name is still just new.
	let other = Identity::generate().unwrap().fingerprint();
	assert_eq!(evaluate_peer(&store, &other, "Bob"), PeerTrust::New);
	assert_eq!(name_skeleton("Alice"), name_skeleton("\u{0410}lice"));
}

#[test]
fn invisible_and_format_characters_are_removed_from_names() {
	use relay_net::discovery::sanitize_display_name;
	for ch in [
		'\u{3164}',
		'\u{FFA0}',
		'\u{206A}',
		'\u{206F}',
		'\u{FE0F}',
		'\u{034F}',
		'\u{E0041}',
		'\u{2800}',
		'\u{180E}',
		'\u{200D}',
		'\u{202E}',
		'\u{E000}',
		'\u{FDD0}',
	] {
		assert_eq!(
			sanitize_display_name(&format!("A{ch}B")),
			"AB",
			"{:x}",
			ch as u32
		);
	}
	// Compatibility forms fold, whitespace collapses, leading marks and zalgo are cut.
	assert_eq!(sanitize_display_name("\u{FF21}\u{FF42}"), "Ab");
	assert_eq!(sanitize_display_name("  a \t\n b  "), "a b");
	assert_eq!(sanitize_display_name("\u{0301}x"), "x");
	assert_eq!(
		sanitize_display_name(&format!("e{}", "\u{0301}".repeat(30))),
		"\u{e9}\u{0301}\u{0301}"
	);
	assert_eq!(sanitize_display_name("\u{3164}\u{200B}"), "Unnamed device");
	let long = sanitize_display_name(&"é".repeat(500));
	assert!(long.chars().count() <= 64 && long.len() <= 256);
}

proptest::proptest! {
	#[test]
	fn sanitized_names_hold_no_invisible_characters(raw in ".{0,200}") {
		use relay_net::discovery::sanitize_display_name;
		let clean = sanitize_display_name(&raw);
		proptest::prop_assert!(clean.chars().count() <= 64 && clean.len() <= 256);
		proptest::prop_assert!(!clean.chars().any(|c| c.is_control()
			|| relay_core::channel::is_invisible_or_format(c)
			|| relay_core::channel::is_private_use_or_noncharacter(c)));
		// Idempotent: sanitising twice changes nothing.
		proptest::prop_assert_eq!(sanitize_display_name(&clean), clean);
	}
}
