mod support;

use relay_config::prefs::{
	DeviceChoice, DeviceSelection, PreferenceStore, Preferences, export_toml, flat, import_toml,
};
use relay_config::{ConfigError, Paths};
use support::{chosen_mic, storage};

fn store(dir: &tempfile::TempDir) -> (PreferenceStore, relay_config::Storage) {
	let s = storage(dir.path());
	(PreferenceStore::open(&s.database).unwrap(), s)
}

#[test]
fn first_run_gives_safe_defaults_without_notices() {
	let dir = tempfile::tempdir().unwrap();
	let (store, s) = store(&dir);
	assert!(s.notices.is_empty());
	let loaded = store.load().unwrap();
	assert!(loaded.notices.is_empty());
	assert_eq!(
		loaded.preferences.audio.input_selection,
		DeviceSelection::Explicit
	);
	assert!(loaded.preferences.audio.preferred_input_id.is_empty());
	assert!(loaded.preferences.privacy.new_calls_receive_only);
	assert!(!loaded.preferences.diagnostics.record_raw_audio);
}

#[test]
fn save_then_load_round_trips_across_reopen() {
	let dir = tempfile::tempdir().unwrap();
	let mut prefs = chosen_mic();
	prefs.audio.microphone_gain = 0.1;
	prefs.profile.display_name = "Zoë \"quoted\"".into();
	{
		let (store, _) = store(&dir);
		store.save(&prefs).unwrap();
	}
	let (store, _) = store(&dir);
	assert_eq!(store.load().unwrap().preferences, prefs);
}

#[test]
fn invalid_values_are_rejected_on_save() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	let mut p = Preferences::default();
	p.audio.microphone_gain = f32::NAN;
	assert!(store.save(&p).is_err());
	p.audio.microphone_gain = 1.0;
	p.profile.display_name = "  ".into();
	assert!(store.save(&p).is_err());
	assert_eq!(store.load().unwrap().preferences, Preferences::default());
}

#[test]
fn second_writer_is_refused_until_first_drops() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	let first = relay_config::Storage::open(&paths).unwrap();
	assert!(matches!(
		relay_config::Storage::open(&paths),
		Err(ConfigError::Locked(_))
	));
	drop(first);
	relay_config::Storage::open(&paths).unwrap();
}

#[test]
fn missing_explicit_mic_never_falls_back() {
	let prefs = chosen_mic();
	assert_eq!(
		prefs.audio.resolve_input(&["built-in", "usb-mic"]),
		DeviceChoice::Device("usb-mic".into())
	);
	assert_eq!(
		prefs.audio.resolve_input(&["built-in"]),
		DeviceChoice::Unavailable("usb-mic".into())
	);
	assert_eq!(
		Preferences::default().audio.resolve_input(&["built-in"]),
		DeviceChoice::NotSelected
	);
	assert_eq!(
		Preferences::default().audio.resolve_output(&[]),
		DeviceChoice::SystemDefault
	);
}

#[test]
fn get_and_set_are_typed_and_validated() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	assert_eq!(store.get("audio.microphone_gain").unwrap(), "1.0");
	store.set("audio.microphone_gain", "2.5").unwrap();
	store.set("audio.noise_backend", "rnnoise").unwrap();
	store.set("audio.preferred_input_id", "123").unwrap();
	store
		.set("privacy.new_calls_receive_only", "false")
		.unwrap();
	let prefs = store.load().unwrap().preferences;
	assert_eq!(prefs.audio.microphone_gain, 2.5);
	assert_eq!(prefs.audio.preferred_input_id, "123");
	assert!(!prefs.privacy.new_calls_receive_only);
	assert_eq!(store.get("audio.preferred_input_id").unwrap(), "123");

	assert!(store.set("audio.microphone_gain", "99").is_err());
	assert!(store.set("audio.microphone_gain", "loud").is_err());
	assert!(
		store
			.set("privacy.new_calls_receive_only", "maybe")
			.is_err()
	);
	assert!(store.set("audio.nope", "1").is_err());
	assert!(store.set("config_version", "9").is_err());
	assert!(store.get("nope").is_err());
	assert_eq!(store.load().unwrap().preferences, prefs);
	assert!(flat::keys().contains(&"audio.noise_backend".to_owned()));
	assert!(!flat::keys().contains(&"config_version".to_owned()));
}

#[test]
fn toml_export_import_round_trips_without_secrets() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	let mut prefs = chosen_mic();
	prefs.profile.display_name = "Hallway".into();
	store.save(&prefs).unwrap();
	let text = store.export_toml().unwrap();
	assert!(text.contains("display_name = \"Hallway\""));
	assert!(!text.to_lowercase().contains("secret"));
	assert_eq!(import_toml(&text).unwrap(), prefs);

	let other = tempfile::tempdir().unwrap();
	let (target, _) = self::store(&other);
	assert_eq!(target.import_toml(&text).unwrap(), prefs);
	assert_eq!(target.load().unwrap().preferences, prefs);
	assert_eq!(export_toml(&prefs).unwrap(), text);
}

#[test]
fn import_rejects_typos_bad_values_and_newer_versions() {
	assert!(import_toml("[audio]\nmicrophone_gane = 1.0\n").is_err());
	assert!(import_toml("[audio]\nmicrophone_gain = 40.0\n").is_err());
	assert!(import_toml("this is = = not toml").is_err());
	assert!(matches!(
		import_toml("config_version = 99\n"),
		Err(ConfigError::UnsupportedVersion { found: 99, .. })
	));
}

#[test]
fn read_only_peek_needs_no_lock_and_writes_nothing() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	assert_eq!(PreferenceStore::read_only(&paths).unwrap(), None);
	let (store, _s) = store(&dir);
	assert_eq!(PreferenceStore::read_only(&paths).unwrap(), None);
	store.save(&chosen_mic()).unwrap();
	// The writer (and its lock) is still alive.
	assert_eq!(
		PreferenceStore::read_only(&paths).unwrap(),
		Some(chosen_mic())
	);
}

#[test]
fn overlay_manual_entry_is_off_until_the_user_enables_it() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	assert_eq!(store.get("network.allow_overlay_manual").unwrap(), "false");
	assert!(
		!store
			.load()
			.unwrap()
			.preferences
			.network
			.allow_overlay_manual
	);
	store.set("network.allow_overlay_manual", "true").unwrap();
	assert!(
		store
			.load()
			.unwrap()
			.preferences
			.network
			.allow_overlay_manual
	);
	assert!(store.set("network.allow_overlay_manual", "yes").is_err());
	assert_eq!(store.get("network.listen_port").unwrap(), "47800");
}

#[test]
fn privacy_choices_default_open_and_persist() {
	let dir = tempfile::tempdir().unwrap();
	{
		let (store, _) = store(&dir);
		assert_eq!(store.get("privacy.inbound_policy").unwrap(), "anyone");
		assert_eq!(store.get("privacy.advert_mode").unwrap(), "visible");
		store
			.set("privacy.inbound_policy", "shared_channel_only")
			.unwrap();
		store.set("privacy.advert_mode", "hidden").unwrap();
		assert!(store.set("privacy.advert_mode", "invisible").is_err());
	}
	let (store, _) = store(&dir);
	let prefs = store.load().unwrap().preferences;
	assert_eq!(
		prefs.privacy.inbound_policy,
		relay_config::prefs::InboundPolicy::SharedChannelOnly
	);
	assert_eq!(
		prefs.privacy.advert_mode,
		relay_config::prefs::AdvertMode::Hidden
	);
}

#[test]
fn push_to_talk_hotkey_is_a_validated_text_setting() {
	let prefs = Preferences::default();
	assert_eq!(
		flat::get(&prefs, "hotkeys.push_to_talk").unwrap(),
		"Ctrl+Alt+Space"
	);
	let changed = flat::set(&prefs, "hotkeys.push_to_talk", "Ctrl+Shift+F9").unwrap();
	assert_eq!(changed.hotkeys.push_to_talk, "Ctrl+Shift+F9");
	let disabled = flat::set(&prefs, "hotkeys.push_to_talk", "").unwrap();
	assert!(disabled.hotkeys.push_to_talk.is_empty());
	let long = "x".repeat(65);
	assert!(flat::set(&prefs, "hotkeys.push_to_talk", &long).is_err());
}

#[test]
fn ptt_release_delay_defaults_validates_and_round_trips() {
	let prefs = Preferences::default();
	assert_eq!(flat::get(&prefs, "ptt.release_delay_ms").unwrap(), "200");
	let changed = flat::set(&prefs, "ptt.release_delay_ms", "0").unwrap();
	assert_eq!(changed.ptt.release_delay_ms, 0);
	let max = flat::set(&prefs, "ptt.release_delay_ms", "2000").unwrap();
	assert_eq!(max.ptt.release_delay_ms, 2000);
	assert!(flat::set(&prefs, "ptt.release_delay_ms", "2001").is_err());
	assert!(flat::set(&prefs, "ptt.release_delay_ms", "-1").is_err());
	assert!(flat::set(&prefs, "ptt.release_delay_ms", "fast").is_err());
	let text = export_toml(&max).unwrap();
	assert!(text.contains("release_delay_ms = 2000"));
	assert_eq!(import_toml(&text).unwrap(), max);
	assert!(import_toml("[ptt]\nrelease_delay_ms = 5000\n").is_err());
	assert_eq!(
		import_toml("[audio]\nplayback_gain = 1.0\n")
			.unwrap()
			.ptt
			.release_delay_ms,
		200,
		"missing key takes the default"
	);
}

#[test]
fn the_desktop_starts_the_daemon_by_default_and_can_be_told_not_to() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	assert_eq!(store.get("clients.autostart_daemon").unwrap(), "true");
	store.set("clients.autostart_daemon", "false").unwrap();
	assert!(!store.load().unwrap().preferences.clients.autostart_daemon);
	assert!(store.set("clients.autostart_daemon", "sometimes").is_err());
}

#[test]
fn ring_preferences_default_validate_and_round_trip() {
	let prefs = Preferences::default();
	assert_eq!(flat::get(&prefs, "audio.ringtone_enabled").unwrap(), "true");
	assert_eq!(flat::get(&prefs, "audio.ringtone_volume").unwrap(), "0.7");
	assert_eq!(flat::get(&prefs, "audio.ringback_enabled").unwrap(), "true");
	assert_eq!(
		flat::get(&prefs, "audio.call_sounds_enabled").unwrap(),
		"true"
	);
	let changed = flat::set(&prefs, "audio.ringtone_volume", "0.5").unwrap();
	assert!((changed.audio.ringtone_volume - 0.5).abs() < f32::EPSILON);
	assert!(flat::set(&prefs, "audio.ringtone_volume", "1.5").is_err());
	assert!(flat::set(&prefs, "audio.ringtone_volume", "-0.1").is_err());
	assert!(flat::set(&prefs, "audio.ringtone_volume", "loud").is_err());
	let off = flat::set(&prefs, "audio.ringtone_enabled", "false").unwrap();
	assert!(!off.audio.ringtone_enabled);
	assert!(flat::set(&prefs, "audio.ringback_enabled", "maybe").is_err());
	let text = export_toml(&off).unwrap();
	assert!(text.contains("ringtone_enabled = false"));
	assert_eq!(import_toml(&text).unwrap(), off);
	assert!(import_toml("[audio]\nringtone_volume = 2.0\n").is_err());
	let partial = import_toml("[audio]\nringtone_volume = 0.2\n").unwrap();
	assert!(partial.audio.call_sounds_enabled);
}

#[test]
fn messaging_defaults_validate_and_round_trip() {
	let prefs = Preferences::default();
	assert_eq!(flat::get(&prefs, "messages.enabled").unwrap(), "true");
	assert_eq!(
		flat::get(&prefs, "messages.max_offer_size_mib").unwrap(),
		"2048"
	);
	assert_eq!(
		flat::get(&prefs, "messages.transfer_rate_limit_mbps").unwrap(),
		"16"
	);
	assert_eq!(flat::get(&prefs, "network.transfer_port").unwrap(), "0");
	let off = flat::set(&prefs, "messages.enabled", "false").unwrap();
	assert!(!off.messages.enabled);
	let port = flat::set(&prefs, "network.transfer_port", "47801").unwrap();
	assert_eq!(port.network.transfer_port, 47801);
	assert!(flat::set(&prefs, "messages.max_offer_size_mib", "0").is_err());
	assert!(flat::set(&prefs, "messages.max_offer_size_mib", "4097").is_err());
	assert!(flat::set(&prefs, "messages.transfer_rate_limit_mbps", "1025").is_err());
	assert!(flat::set(&prefs, "network.transfer_port", "70000").is_err());
	let unlimited = flat::set(&prefs, "messages.transfer_rate_limit_mbps", "0").unwrap();
	assert_eq!(unlimited.messages.transfer_rate_limit_mbps, 0);
}
