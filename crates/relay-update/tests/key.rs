//! The compiled-in key.

use relay_update::verify::{PUBLIC_KEY, public_key, verify_bytes};

mod support;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn key_line(text: &str) -> String {
	text.lines().nth(1).unwrap_or_default().trim().to_owned()
}

#[test]
fn the_compiled_in_key_parses() {
	public_key().unwrap();
}

#[test]
fn the_test_fixture_is_a_consistent_keypair() {
	// The seed derives the public key stored in the fixture and used as the interim release key.
	use base64::Engine;
	let key = support::signing_key();
	let fixture = std::fs::read_to_string(format!("{FIXTURES}/TEST-ONLY-update-key.pub")).unwrap();
	let blob = base64::engine::general_purpose::STANDARD
		.decode(key_line(&fixture))
		.unwrap();
	assert_eq!(&blob[..2], b"Ed");
	assert_eq!(&blob[2..10], &support::KEY_ID);
	assert_eq!(&blob[10..], key.verifying_key().as_bytes());
}

#[test]
fn a_signature_made_like_the_minisign_tool_verifies() {
	let signature = support::sign_asset(b"payload", "0.1.1", "relay-0.1.1-linux-x64.AppImage");
	verify_bytes(
		b"payload",
		signature.as_bytes(),
		&relay_update::version::parse("0.1.1").unwrap(),
		"relay-0.1.1-linux-x64.AppImage",
	)
	.unwrap();
	let tampered = verify_bytes(
		b"payloaD",
		signature.as_bytes(),
		&relay_update::version::parse("0.1.1").unwrap(),
		"relay-0.1.1-linux-x64.AppImage",
	);
	assert!(tampered.is_err());
}

/// Run by the release job: `cargo test -p relay-update -- --ignored release_key_is_not_the_test_key`.
/// Fails until `update-key.pub` is replaced with the real key (see UPDATE_KEY.md).
#[test]
#[ignore = "fails by design until the real release key replaces the test key"]
fn release_key_is_not_the_test_key() {
	let compiled = key_line(PUBLIC_KEY);
	let mut tested = 0;
	for entry in std::fs::read_dir(FIXTURES).unwrap().flatten() {
		let path = entry.path();
		if path.extension().is_some_and(|e| e == "pub") {
			assert_ne!(
				compiled,
				key_line(&std::fs::read_to_string(&path).unwrap()),
				"update-key.pub is still a TEST-ONLY key: {}",
				path.display()
			);
			tested += 1;
		}
	}
	assert!(tested > 0, "no test key fixture found to compare against");
}
