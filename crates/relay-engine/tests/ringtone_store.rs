//! The ringtone on disk: copied into the data directory, replaced whole, never half-written.

use relay_audio::ringtone::RingtoneLimits;
use relay_engine::daemon::RingtoneStore;
use relay_engine::service::ports::PortError;

const LIMITS: RingtoneLimits = RingtoneLimits {
	max_bytes: 1 << 20,
	max_seconds: 30,
};

fn fixture(name: &str) -> Vec<u8> {
	std::fs::read(format!(
		"{}/../relay-audio/tests/fixtures/ringtone/{name}",
		env!("CARGO_MANIFEST_DIR")
	))
	.expect("fixture")
}

#[test]
fn a_saved_ring_survives_a_restart_inside_its_own_folder() {
	let dir = tempfile::tempdir().unwrap();
	let store = RingtoneStore::new(dir.path().join("ringtone"));
	assert!(store.load().is_none());
	assert_eq!(store.name(), "");

	let ring = store
		.save("chime.flac", &fixture("tone.flac"), LIMITS)
		.unwrap();
	let reopened = RingtoneStore::new(dir.path().join("ringtone"));
	let (name, loaded) = reopened.load().expect("stored");
	assert_eq!(name, "chime.flac");
	assert_eq!(loaded.len(), ring.len());
	let size = std::fs::metadata(dir.path().join("ringtone/ring.opus"))
		.unwrap()
		.len();
	assert!(size < ring.len() as u64 / 4, "stored as Opus, {size} bytes");
	assert!(
		loaded
			.iter()
			.zip(ring.iter())
			.all(|(a, b)| (a - b).abs() < 1e-6)
	);
	assert_eq!(reopened.name(), "chime.flac");

	let mut files: Vec<_> = std::fs::read_dir(dir.path().join("ringtone"))
		.unwrap()
		.map(|e| e.unwrap().file_name().into_string().unwrap())
		.collect();
	files.sort();
	assert_eq!(
		files,
		["name.txt", "ring.opus"],
		"no temporary file is left behind"
	);
}

#[test]
fn a_refused_file_keeps_the_old_ring() {
	let dir = tempfile::tempdir().unwrap();
	let store = RingtoneStore::new(dir.path().join("ringtone"));
	store.save("a.mp3", &fixture("tone.mp3"), LIMITS).unwrap();
	let small = RingtoneLimits {
		max_bytes: 100,
		..LIMITS
	};
	match store.save("b.mp3", &fixture("tone.mp3"), small) {
		Err(PortError::Rejected { code, .. }) => assert_eq!(code, "RINGTONE_TOO_LARGE"),
		other => panic!("{other:?}"),
	}
	assert_eq!(store.load().expect("old ring").0, "a.mp3");
}

#[test]
fn clearing_is_idempotent() {
	let dir = tempfile::tempdir().unwrap();
	let store = RingtoneStore::new(dir.path().join("ringtone"));
	store.clear().unwrap();
	store.save("a.aac", &fixture("tone.aac"), LIMITS).unwrap();
	store.clear().unwrap();
	store.clear().unwrap();
	assert!(store.load().is_none());
	assert_eq!(store.name(), "");
}
