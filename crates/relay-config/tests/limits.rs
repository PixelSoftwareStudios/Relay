mod support;

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use relay_config::avatar::{AvatarCache, process_avatar};
use relay_config::history::{
	CallHistory, CallOutcome, Direction, Limits as HistoryLimits, MAX_AGE_MS, MAX_ENTRIES,
	NewCallRecord,
};
use relay_config::prefs::limits::{SPECS, spec};
use relay_config::prefs::{Limits, Preferences, flat};
use support::storage;

const NOW: u64 = 1_800_000_000_000;
const DAY: u64 = 24 * 60 * 60 * 1000;

fn entry(started: u64) -> NewCallRecord {
	NewCallRecord {
		peer_name: "Alice".to_owned(),
		peer_fingerprint: "ab".repeat(32),
		peer_fingerprint_short: "ABAB".to_owned(),
		direction: Direction::Outgoing,
		outcome: CallOutcome::Completed,
		started_at_ms: started,
		duration_ms: 1,
		call_id: [7; 16],
	}
}

fn noise(shade: u8) -> relay_config::avatar::Avatar {
	let img = RgbaImage::from_fn(64, 64, |x, y| {
		Rgba([shade, (x * 4) as u8 ^ shade, (y * 7) as u8 ^ shade, 255])
	});
	let mut out = Vec::new();
	DynamicImage::ImageRgba8(img)
		.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
		.unwrap();
	process_avatar(&out).unwrap()
}

#[test]
fn defaults_match_the_code_constants() {
	let d = Limits::default();
	assert_eq!(
		d.avatar_cache_bytes(),
		relay_config::avatar::limits::CACHE_MAX_BYTES
	);
	assert_eq!(d.history_max_entries as usize, MAX_ENTRIES);
	assert_eq!(d.history_max_age_ms(), MAX_AGE_MS);
	assert_eq!(d.check(), None);
	for (spec, value) in SPECS.iter().zip(d.values()) {
		assert_eq!(spec.default, value, "{}", spec.key);
		assert!(
			(spec.min..=spec.max).contains(&spec.default),
			"{}",
			spec.key
		);
	}
}

#[test]
fn every_key_is_a_settable_preference_with_range_checks() {
	let prefs = Preferences::default();
	let keys = flat::keys();
	for s in &SPECS {
		assert!(keys.contains(&s.key.to_owned()), "{}", s.key);
		assert_eq!(flat::get(&prefs, s.key).unwrap(), s.default.to_string());
		// Inside the range (the cross-field rule is checked separately).
		let edge = |v: u32| flat::set(&prefs, s.key, &v.to_string());
		if s.key != "limits.messages_session_mib" {
			assert!(edge(s.min).is_ok() || s.key == "limits.messages_retained_mib");
		}
		assert!(edge(s.min.max(s.default)).is_ok(), "{}", s.key);
		let too_big = edge(s.max + 1).unwrap_err().to_string();
		assert!(too_big.contains(s.key), "{too_big}");
		if s.min > 0 {
			let too_small = edge(s.min - 1).unwrap_err().to_string();
			assert!(too_small.contains(s.key), "{too_small}");
		}
	}
	assert!(spec("limits.nope").is_none());
	assert!(flat::set(&prefs, "limits.nope", "1").is_err());
}

#[test]
fn retained_budget_must_not_be_smaller_than_the_session_budget() {
	let prefs = Preferences::default();
	let error = flat::set(&prefs, "limits.messages_retained_mib", "32")
		.unwrap_err()
		.to_string();
	assert!(error.contains("limits.messages_retained_mib"), "{error}");
	assert!(error.contains("limits.messages_session_mib"), "{error}");
	let error = flat::set(&prefs, "limits.messages_session_mib", "512")
		.unwrap_err()
		.to_string();
	assert!(error.contains("must not be smaller"), "{error}");
}

#[test]
fn set_many_is_order_independent_for_the_pair() {
	let prefs = Preferences::default();
	let lower = [
		("limits.messages_retained_mib".to_owned(), "16".to_owned()),
		("limits.messages_session_mib".to_owned(), "8".to_owned()),
	];
	let (next, answers) = flat::set_many(&prefs, &lower);
	assert_eq!(answers, vec![None, None]);
	assert_eq!(next.limits.messages_retained_mib, 16);
	assert_eq!(next.limits.messages_session_mib, 8);

	let (next, answers) = flat::set_many(
		&prefs,
		&[
			("limits.history_max_age_days".to_owned(), "0".to_owned()),
			("limits.history_max_entries".to_owned(), "10".to_owned()),
		],
	);
	assert!(
		answers[0]
			.as_deref()
			.unwrap()
			.contains("limits.history_max_age_days")
	);
	assert_eq!(answers[1], None);
	assert_eq!(next.limits.history_max_entries, 10);
	assert_eq!(next.limits.history_max_age_days, 90);
}

#[test]
fn limits_persist_export_and_import() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let store = relay_config::prefs::PreferenceStore::open(&s.database).unwrap();
	let mut prefs = Preferences::default();
	prefs.limits.avatar_cache_mib = 32;
	prefs.limits.history_max_entries = 7;
	store.save(&prefs).unwrap();
	let text = store.export_toml().unwrap();
	assert!(text.contains("avatar_cache_mib = 32"), "{text}");
	let imported = relay_config::prefs::import_toml(&text).unwrap();
	assert_eq!(imported.limits, prefs.limits);
	assert!(relay_config::prefs::import_toml("[limits]\navatar_cache_mib = 1\n").is_err());
}

#[test]
fn shrinking_the_avatar_cache_evicts_at_once_and_keeps_the_local_avatar() {
	let dir = tempfile::tempdir().unwrap();
	let cache = AvatarCache::open(dir.path()).unwrap();
	let avatars: Vec<_> = (1..=6).map(noise).collect();
	for a in &avatars {
		cache.put(a).unwrap();
	}
	cache.set_local(&avatars[0].hash).unwrap();
	let each = avatars.iter().map(|a| a.image.len() as u64).max().unwrap();
	let total: u64 = avatars.iter().map(|a| a.image.len() as u64).sum();
	assert_eq!(cache.size().unwrap(), total);
	// Room for the pinned avatar and the newest one, not the rest.
	let room = (avatars[0].image.len() + avatars[5].image.len()) as u64 + 10;
	cache.set_limit(room).unwrap();
	let after = cache.size().unwrap();
	assert!(after <= room, "{after}");
	assert!(cache.local().unwrap().is_some(), "pinned entry survives");
	assert!(
		cache.get(&avatars[5].hash).unwrap().is_some(),
		"newest survives"
	);
	assert!(
		cache.get(&avatars[1].hash).unwrap().is_none(),
		"oldest evicted"
	);
	// A clone shares the limit, and growing allows more again.
	let clone = cache.clone();
	assert_eq!(clone.limit(), room);
	clone.set_limit(each * 10).unwrap();
	assert_eq!(cache.limit(), each * 10);
	for a in &avatars {
		cache.put(a).unwrap();
	}
	assert_eq!(cache.size().unwrap(), total);
	// Below even the pinned entry: only that one stays.
	cache.set_limit(1).unwrap();
	assert_eq!(cache.size().unwrap(), avatars[0].image.len() as u64);
}

#[test]
fn history_limits_come_from_settings_and_apply_to_the_next_write_and_at_once() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let mut prefs = Preferences::default();
	prefs.limits.history_max_entries = 3;
	prefs.limits.history_max_age_days = 10;
	let history = CallHistory::with_limits(
		&s.database,
		HistoryLimits {
			max_entries: prefs.limits.history_max_entries as usize,
			max_age_ms: prefs.limits.history_max_age_ms(),
		},
	);
	for i in 0..5 {
		history.record(&entry(NOW - i), NOW).unwrap();
	}
	assert_eq!(history.count().unwrap(), 3);
	history.record(&entry(NOW - 11 * DAY), NOW).unwrap();
	assert_eq!(
		history.count().unwrap(),
		3,
		"older than 10 days is not kept"
	);

	// Raising the limits keeps what is stored and allows more from now on.
	let clone = history.clone();
	clone
		.set_limits(
			HistoryLimits {
				max_entries: 5,
				max_age_ms: 30 * DAY,
			},
			NOW,
		)
		.unwrap();
	for i in 5..9 {
		history.record(&entry(NOW - i), NOW).unwrap();
	}
	assert_eq!(history.count().unwrap(), 5);

	// Lowering prunes immediately, without a new call.
	history
		.set_limits(
			HistoryLimits {
				max_entries: 2,
				max_age_ms: 30 * DAY,
			},
			NOW,
		)
		.unwrap();
	assert_eq!(history.count().unwrap(), 2);
	history
		.set_limits(
			HistoryLimits {
				max_entries: 0,
				max_age_ms: DAY,
			},
			NOW,
		)
		.unwrap();
	assert_eq!(history.count().unwrap(), 0);
	assert_eq!(history.limits().max_entries, 0);
}
