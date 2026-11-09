//! The compiled-in key.

use relay_update::verify::{PUBLIC_KEY, TEST_ONLY_PUBLIC_KEY, public_key, verify_bytes};
use relay_update::{Arch, Error, InstallKind, ManualReason, Os, Platform, Updater};

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
	// The seed derives the public key stored in the fixture and used by the test-only constructors.
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

const RELEASE_KEY_ID: &str = "92AC5C640F5EEC91";

#[test]
fn the_production_key_is_exactly_the_committed_file_and_not_the_test_key() {
	let committed =
		std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/update-key.pub")).unwrap();
	assert_eq!(PUBLIC_KEY, committed);
	assert!(PUBLIC_KEY.contains(RELEASE_KEY_ID));
	let test = std::fs::read_to_string(format!("{FIXTURES}/TEST-ONLY-update-key.pub")).unwrap();
	assert_ne!(key_line(PUBLIC_KEY), key_line(&test));
	let updater = Updater::new("0.1.0", platform()).unwrap();
	assert_eq!(
		updater.key_comment(),
		public_key().unwrap().untrusted_comment()
	);
	let test_updater = support::updater("0.1.0", platform(), "http://127.0.0.1:1");
	assert_ne!(updater.key_comment(), test_updater.key_comment());
}

fn platform() -> Platform {
	Platform {
		os: Os::Linux,
		arch: Arch::X64,
		install: InstallKind::Manual(ManualReason::Packaged),
	}
}

#[test]
fn a_test_key_signature_is_rejected_by_the_release_key() {
	let version = relay_update::version::parse("0.1.1").unwrap();
	let name = "relay-0.1.1-linux-x64.AppImage";
	let signature = support::sign_asset(b"payload", "0.1.1", name);
	let release = public_key().unwrap();
	assert!(verify_bytes(&release, b"payload", signature.as_bytes(), &version, name).is_err());
	let test = decode(TEST_ONLY_PUBLIC_KEY);
	verify_bytes(&test, b"payload", signature.as_bytes(), &version, name).unwrap();
	assert!(verify_bytes(&test, b"payloaD", signature.as_bytes(), &version, name).is_err());
}

#[test]
fn an_updater_with_the_release_key_refuses_a_test_signed_download() {
	let dir = support::tempdir();
	let image = support::write(dir.path().join("Relay.AppImage"), b"old");
	let server = support::server::Server::start();
	support::publish_appimage(&server, "0.1.1", b"payload bytes");
	let updater = Updater::with_endpoint_release_key(
		"0.1.0",
		support::linux_platform(&image),
		support::endpoint(&server.base),
	)
	.unwrap();
	let available = updater.check().unwrap().unwrap();
	let result = updater.download(&available, &mut |_| std::ops::ControlFlow::Continue(()));
	assert!(
		matches!(result, Err(Error::SignatureInvalid(_))),
		"{result:?}"
	);
}

fn decode(text: &str) -> minisign_verify::PublicKey {
	minisign_verify::PublicKey::decode(text).unwrap()
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
