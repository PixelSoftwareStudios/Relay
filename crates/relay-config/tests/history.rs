mod support;

use relay_config::history::{
	CallHistory, CallOutcome, Direction, MAX_AGE_MS, MAX_ENTRIES, NewCallRecord,
};
use relay_config::prefs::{PreferenceStore, Preferences};
use support::storage;

const DAY: u64 = 24 * 60 * 60 * 1000;
const NOW: u64 = 1_800_000_000_000;

fn entry(name: &str, started: u64) -> NewCallRecord {
	NewCallRecord {
		peer_name: name.to_owned(),
		peer_fingerprint: "ab".repeat(32),
		peer_fingerprint_short: "ABAB-ABAB-ABAB-ABAB-ABAB".to_owned(),
		direction: Direction::Outgoing,
		outcome: CallOutcome::Completed,
		started_at_ms: started,
		duration_ms: 61_000,
		call_id: [7; 16],
	}
}

#[test]
fn insert_and_read_back_every_field() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let history = CallHistory::open(&s.database);
	let mut e = entry("Alice\u{7}", NOW - 1000);
	e.direction = Direction::Incoming;
	e.outcome = CallOutcome::Missed;
	e.duration_ms = 0;
	history.record(&e, NOW).unwrap();
	let rows = history.list(10, None).unwrap();
	assert_eq!(rows.len(), 1);
	assert_eq!(rows[0].peer_name, "Alice");
	assert_eq!(rows[0].peer_fingerprint, "ab".repeat(32));
	assert_eq!(rows[0].peer_fingerprint_short, e.peer_fingerprint_short);
	assert_eq!(rows[0].direction, Direction::Incoming);
	assert_eq!(rows[0].outcome, CallOutcome::Missed);
	assert_eq!(rows[0].started_at_ms, NOW - 1000);
	assert_eq!(rows[0].duration_ms, 0);
	assert_eq!(rows[0].call_id, [7; 16]);
}

#[test]
fn survives_reopen_and_rejects_bad_fingerprints() {
	let dir = tempfile::tempdir().unwrap();
	{
		let s = storage(dir.path());
		let history = CallHistory::open(&s.database);
		history.record(&entry("Bob", NOW), NOW).unwrap();
		let mut bad = entry("Eve", NOW);
		bad.peer_fingerprint = "zz".into();
		assert!(history.record(&bad, NOW).is_err());
	}
	let s = storage(dir.path());
	assert_eq!(CallHistory::open(&s.database).count().unwrap(), 1);
}

#[test]
fn lists_newest_first_and_pages_by_cursor() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let history = CallHistory::open(&s.database);
	for i in 0..5 {
		history
			.record(&entry(&format!("p{i}"), NOW - 100 + i), NOW)
			.unwrap();
	}
	let first = history.list(2, None).unwrap();
	assert_eq!(first[0].peer_name, "p4");
	assert_eq!(first[1].peer_name, "p3");
	let next = history.list(10, Some(first[1].id)).unwrap();
	let names: Vec<_> = next.iter().map(|r| r.peer_name.as_str()).collect();
	assert_eq!(names, ["p2", "p1", "p0"]);
	assert!(history.list(10, Some(next[2].id)).unwrap().is_empty());
}

#[test]
fn keeps_only_the_newest_entries() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let history = CallHistory::open(&s.database);
	for i in 0..(MAX_ENTRIES as u64 + 7) {
		history
			.record(&entry(&format!("p{i}"), NOW - 1000 + i), NOW)
			.unwrap();
	}
	assert_eq!(history.count().unwrap(), MAX_ENTRIES);
	let rows = history.list(MAX_ENTRIES, None).unwrap();
	assert_eq!(rows[0].peer_name, format!("p{}", MAX_ENTRIES + 6));
	assert_eq!(rows.last().unwrap().peer_name, "p7");
}

#[test]
fn drops_entries_older_than_ninety_days_on_write() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let history = CallHistory::open(&s.database);
	history
		.record(&entry("old", NOW - MAX_AGE_MS - DAY), NOW - 200 * DAY)
		.unwrap();
	history
		.record(&entry("edge", NOW - MAX_AGE_MS + DAY), NOW - 100 * DAY)
		.unwrap();
	assert_eq!(history.count().unwrap(), 2);
	history.record(&entry("new", NOW), NOW).unwrap();
	let names: Vec<_> = history
		.list(10, None)
		.unwrap()
		.into_iter()
		.map(|r| r.peer_name)
		.collect();
	assert_eq!(names, ["new", "edge"]);
}

#[test]
fn clear_deletes_everything() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let history = CallHistory::open(&s.database);
	for i in 0..3 {
		history.record(&entry("x", NOW + i), NOW).unwrap();
	}
	assert_eq!(history.clear().unwrap(), 3);
	assert!(history.list(10, None).unwrap().is_empty());
	assert_eq!(history.clear().unwrap(), 0);
}

#[test]
fn the_setting_defaults_on_and_turning_it_off_keeps_entries() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let prefs = PreferenceStore::open(&s.database).unwrap();
	assert!(Preferences::default().history.enabled);
	assert_eq!(prefs.get("history.enabled").unwrap(), "true");
	let history = CallHistory::open(&s.database);
	history.record(&entry("kept", NOW), NOW).unwrap();
	let next = prefs.set("history.enabled", "false").unwrap();
	assert!(!next.history.enabled);
	assert_eq!(prefs.get("history.enabled").unwrap(), "false");
	assert_eq!(history.count().unwrap(), 1);
}
