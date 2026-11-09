mod support;

use relay_config::autoanswer::{AutoAnswer, NOT_VERIFIED};
use relay_config::identity::TrustStore;
use relay_config::prefs::CallAlert;
use support::storage;

fn key(n: u8) -> [u8; 32] {
	[n; 32]
}

#[test]
fn only_verified_contacts_can_be_flagged_and_the_flag_survives_reopen() {
	let dir = tempfile::tempdir().unwrap();
	{
		let s = storage(dir.path());
		let mut trust = TrustStore::open(&s.database).unwrap();
		trust.pin_tofu(&key(1), "Tofu", 1).unwrap();
		trust.pin_tofu(&key(2), "Checked", 2).unwrap();
		trust.mark_verified(&key(2)).unwrap();
		let flags = AutoAnswer::open(&s.database);
		assert_eq!(
			flags.set(&key(1), true).unwrap_err().to_string(),
			NOT_VERIFIED
		);
		assert!(flags.set(&key(9), true).is_err(), "unknown peer");
		flags.set(&key(2), true).unwrap();
		flags.set(&key(2), true).unwrap();
	}
	let s = storage(dir.path());
	assert_eq!(AutoAnswer::open(&s.database).list().unwrap(), vec![key(2)]);
}

#[test]
fn forgetting_or_replacing_the_key_removes_the_flag() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let mut trust = TrustStore::open(&s.database).unwrap();
	let flags = AutoAnswer::open(&s.database);
	for n in [1, 2] {
		trust.pin_tofu(&key(n), "P", 1).unwrap();
		trust.mark_verified(&key(n)).unwrap();
		flags.set(&key(n), true).unwrap();
	}
	trust.remove(&key(1)).unwrap();
	assert_eq!(flags.list().unwrap(), vec![key(2)]);
	let new = trust.replace_key(&key(2), &key(3)).unwrap();
	assert_eq!(new.public_key, hex::encode(key(3)));
	assert!(
		flags.list().unwrap().is_empty(),
		"the new key starts unflagged"
	);
	flags.set(&key(3), true).unwrap();
	flags.set(&key(3), false).unwrap();
	assert!(flags.list().unwrap().is_empty());
}

#[test]
fn background_and_call_settings_have_safe_defaults_and_validate() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let store = relay_config::prefs::PreferenceStore::open(&s.database).unwrap();
	let prefs = store.load().unwrap().preferences;
	// Off on a desktop, on for the phone, which must keep ringing in the background.
	assert_eq!(prefs.background.available, cfg!(target_os = "android"));
	assert_eq!(
		relay_config::prefs::BACKGROUND_AVAILABLE_DEFAULT,
		cfg!(target_os = "android")
	);
	assert_eq!(prefs.calls.alert, CallAlert::System);
	assert!(prefs.calls.auto_answer_enabled);
	store
		.set(
			"background.available",
			&(!prefs.background.available).to_string(),
		)
		.unwrap();
	store.set("calls.alert", "ring_and_vibrate").unwrap();
	store.set("calls.auto_answer_enabled", "false").unwrap();
	let prefs = store.load().unwrap().preferences;
	assert_eq!(prefs.background.available, !cfg!(target_os = "android"));
	assert_eq!(prefs.calls.alert, CallAlert::RingAndVibrate);
	assert!(!prefs.calls.auto_answer_enabled);
	assert_eq!(store.get("calls.alert").unwrap(), "ring_and_vibrate");
	assert!(store.set("calls.alert", "loud").is_err());
	assert!(store.set("background.available", "maybe").is_err());
}
