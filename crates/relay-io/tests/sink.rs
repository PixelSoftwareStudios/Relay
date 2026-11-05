use std::fs;

use relay_io::clock::SystemClock;
use relay_io::entropy::OsEntropy;
use relay_io::paths::{log_dir_from, runtime_dir_from};
use relay_io::sink::DirSink;
use relay_messages::sanitize::sanitize_file_name;
use relay_messages::transfer::FileSink;
use relay_messages::{Clock, IdSource};

fn receive(sink: &mut DirSink, name: &str, data: &[u8]) -> String {
	sink.create_temp(&sanitize_file_name(name), data.len() as u64)
		.unwrap();
	sink.write(data).unwrap();
	sink.sync().unwrap();
	sink.commit().unwrap()
}

fn names(dir: &std::path::Path) -> Vec<String> {
	let mut v: Vec<String> = fs::read_dir(dir)
		.unwrap()
		.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
		.collect();
	v.sort();
	v
}

#[test]
fn commit_never_overwrites_and_picks_a_unique_name() {
	let tmp = tempfile::tempdir().unwrap();
	let mut sink = DirSink::new(tmp.path());
	assert_eq!(receive(&mut sink, "photo.png", b"one"), "photo.png");
	assert_eq!(receive(&mut sink, "photo.png", b"two"), "photo (1).png");
	assert_eq!(receive(&mut sink, "photo.png", b"three"), "photo (2).png");
	assert_eq!(fs::read(tmp.path().join("photo.png")).unwrap(), b"one");
	assert_eq!(fs::read(tmp.path().join("photo (1).png")).unwrap(), b"two");
	assert_eq!(receive(&mut sink, "notes", b"x"), "notes");
	assert_eq!(receive(&mut sink, "notes", b"y"), "notes (1)");
	// No temp file is left behind after commits.
	assert!(names(tmp.path()).iter().all(|n| !n.starts_with('.')));
}

#[test]
fn abort_and_drop_remove_the_temp_file() {
	let tmp = tempfile::tempdir().unwrap();
	let mut sink = DirSink::new(tmp.path());
	sink.create_temp(&sanitize_file_name("a.bin"), 3).unwrap();
	sink.write(b"abc").unwrap();
	assert_eq!(names(tmp.path()).len(), 1);
	sink.abort();
	sink.abort();
	assert!(names(tmp.path()).is_empty());
	sink.create_temp(&sanitize_file_name("a.bin"), 3).unwrap();
	drop(sink);
	assert!(names(tmp.path()).is_empty());
}

#[test]
fn writes_without_a_transfer_fail_and_free_space_is_reported() {
	let tmp = tempfile::tempdir().unwrap();
	let mut sink = DirSink::new(tmp.path());
	assert!(sink.write(b"x").is_err());
	assert!(sink.sync().is_err());
	assert!(sink.commit().is_err());
	assert!(sink.free_space().unwrap() > 0);
	assert_eq!(sink.dir(), tmp.path());
}

#[cfg(unix)]
#[test]
fn received_files_are_not_world_readable_before_commit() {
	use std::os::unix::fs::PermissionsExt;
	let tmp = tempfile::tempdir().unwrap();
	let mut sink = DirSink::new(tmp.path());
	sink.create_temp(&sanitize_file_name("a"), 1).unwrap();
	let temp = fs::read_dir(tmp.path()).unwrap().next().unwrap().unwrap();
	assert_eq!(temp.metadata().unwrap().permissions().mode() & 0o077, 0);
}

#[test]
fn clocks_and_entropy_implement_the_pure_crate_ports() {
	let clock = SystemClock::new();
	assert!(Clock::wall_ms(&clock) > 1_600_000_000_000);
	let a = Clock::mono_ms(&clock);
	std::thread::sleep(std::time::Duration::from_millis(5));
	assert!(Clock::mono_ms(&clock) >= a);
	let mut ids = OsEntropy;
	assert_ne!(IdSource::next_u128(&mut ids), IdSource::next_u128(&mut ids));
	let mut buf = [0u8; 32];
	OsEntropy.fill(&mut buf);
	assert_ne!(buf, [0u8; 32]);
	assert!(relay_io::clock::unix_secs() > 1_600_000_000);
}

#[test]
fn per_user_directories_follow_the_documented_rules() {
	let tmp = std::path::Path::new("/tmp");
	assert_eq!(
		runtime_dir_from(Some("/run/user/1000".into()), tmp, 1000),
		std::path::Path::new("/run/user/1000/relay")
	);
	assert_eq!(
		runtime_dir_from(Some("relative".into()), tmp, 501),
		std::path::Path::new("/tmp/relay-501")
	);
	assert_eq!(
		runtime_dir_from(None, tmp, 501),
		std::path::Path::new("/tmp/relay-501")
	);
	let log = log_dir_from(Some("/home/u".into()), None, None, tmp);
	assert!(log.starts_with("/home/u") || cfg!(windows));
	assert!(relay_io::paths::UserDirs::platform().is_ok());
}
